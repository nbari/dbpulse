use super::{
    HealthCheckResult, ReadBack, classify_cert_probe_error, classify_read_back,
    record_replication_lag, set_or_retire, validate_identifier, validate_range,
};
use crate::{
    metrics::{
        BLOCKED_SESSIONS, CONNECTION_DURATION, DATABASE_SIZE_BYTES, OPERATION_DURATION,
        ROWS_AFFECTED, RW_ROW_CONTENTION, TABLE_MAINTENANCE_ERRORS, TABLE_RECREATED, TABLE_ROWS,
        TABLE_SIZE_BYTES, TLS_CERT_PROBE_ERRORS, TLS_HANDSHAKE_DURATION,
    },
    tls::{
        TlsConfig, TlsMetadata, TlsMode, TlsProbeProtocol,
        cache::{CertCache, get_cert_metadata_cached},
        ensure_crypto_provider,
        probe::expiry_days_from_remaining,
    },
};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc, prelude::*};
use dsn::DSN;
use rand::RngExt;
use sqlx::{
    AssertSqlSafe, ConnectOptions, Connection, Executor, Row,
    mysql::{MySqlConnectOptions, MySqlConnection, MySqlDatabaseError, MySqlSslMode},
};
use std::time::Instant;
use uuid::Uuid;

const MYSQL_SSL_DATE_FORMATS: [&str; 2] = ["%b %e %H:%M:%S %Y GMT", "%Y-%m-%d %H:%M:%S"];

/// Test read/write operations on the default table
///
/// # Errors
///
/// Returns an error if database connection or operations fail
pub async fn test_rw(
    dsn: &DSN,
    now: DateTime<Utc>,
    range: u32,
    tls: &TlsConfig,
    cert_cache: &CertCache,
) -> Result<HealthCheckResult> {
    test_rw_with_table(dsn, now, range, tls, cert_cache, "dbpulse_rw").await
}

/// Test read/write operations on a specified table
///
/// # Errors
///
/// Returns an error if database connection or operations fail
pub async fn test_rw_with_table(
    dsn: &DSN,
    now: DateTime<Utc>,
    range: u32,
    tls: &TlsConfig,
    cert_cache: &CertCache,
    table_name: &str,
) -> Result<HealthCheckResult> {
    validate_identifier("table name", table_name)?;
    validate_range(range)?;
    ensure_crypto_provider();
    let options = mysql_connect_options(dsn, tls);
    let conn_start = Instant::now();
    let mut conn = connect_mysql(&options, dsn, tls).await?;
    set_mysql_session_timeouts(&mut conn).await?;

    let health_info = fetch_mysql_health_info(&mut conn).await?;

    // Recorded on every check rather than only while read-only: a gauge that
    // stops being written keeps its last value, so a replica promoted to
    // primary would otherwise report its final pre-promotion lag forever.
    maybe_record_mysql_replication_lag(&mut conn).await;

    if mysql_is_read_only(&mut conn).await? {
        return mysql_read_only_result(dsn, tls, &mut conn, cert_cache, health_info).await;
    }

    monitor_mysql_blocked_sessions(&mut conn).await;
    let id = mysql_read_write_cycle_resilient(&mut conn, now, range, table_name).await?;
    mysql_cleanup_old_records(&mut conn, table_name).await;
    update_mysql_table_rows_metric(&mut conn, table_name).await;
    maybe_drop_mysql_table_hourly(&mut conn, now, id, table_name).await;
    update_mysql_size_metrics(&mut conn, table_name).await;

    let tls_metadata = maybe_extract_mysql_tls(dsn, tls, &mut conn, cert_cache).await;
    let _ = conn.close().await;
    CONNECTION_DURATION.observe(conn_start.elapsed().as_secs_f64());

    Ok(HealthCheckResult {
        version: health_info.version.context("Expected database version")?,
        db_host: health_info.db_host,
        uptime_seconds: health_info.uptime_seconds,
        tls_metadata,
        read_only: false,
        read_only_reason: None,
    })
}

/// MySQL error 1146: the table referenced by the statement does not exist.
const MYSQL_ERR_NO_SUCH_TABLE: u16 = 1146;

/// Did this fail only because the monitoring table was not there?
fn is_missing_table_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<sqlx::Error>()
        .and_then(|err| match err {
            sqlx::Error::Database(db_err) => db_err.as_error().downcast_ref::<MySqlDatabaseError>(),
            _ => None,
        })
        .is_some_and(|db_err| db_err.number() == MYSQL_ERR_NO_SUCH_TABLE)
}

/// The write/read half of a check: create the table if needed, prove the server
/// accepts a write, read it back, and prove a transaction rolls back.
async fn mysql_read_write_cycle(
    conn: &mut MySqlConnection,
    now: DateTime<Utc>,
    range: u32,
    table_name: &str,
) -> Result<u32> {
    ensure_mysql_table(conn, table_name).await?;
    let id = mysql_insert_and_verify(conn, now, range, table_name).await?;
    mysql_transaction_rollback_test(conn, now, table_name).await?;
    Ok(id)
}

/// Run the read/write cycle, tolerating the table disappearing underneath it.
///
/// dbpulse drops its own table on the hour to exercise DDL (a Galera cluster
/// that stalls on DDL is exactly what this tool exists to catch). Instances
/// share one table, so a second instance can be mid-check when that DROP lands
/// and see "table doesn't exist" for a database that is perfectly healthy.
/// Recreating and retrying once keeps that from being reported as a fault,
/// while a genuine failure still surfaces on the retry. The recovery is counted
/// so it stays visible rather than silent.
async fn mysql_read_write_cycle_resilient(
    conn: &mut MySqlConnection,
    now: DateTime<Utc>,
    range: u32,
    table_name: &str,
) -> Result<u32> {
    match mysql_read_write_cycle(conn, now, range, table_name).await {
        Err(error) if is_missing_table_error(&error) => {
            TABLE_RECREATED.with_label_values(&["mysql"]).inc();
            mysql_read_write_cycle(conn, now, range, table_name).await
        }
        result => result,
    }
}

struct MySqlHealthInfo {
    version: Option<String>,
    db_host: Option<String>,
    uptime_seconds: Option<i64>,
}

fn mysql_connect_options(dsn: &DSN, tls: &TlsConfig) -> MySqlConnectOptions {
    let mut options = MySqlConnectOptions::new()
        .username(dsn.username.clone().unwrap_or_default().as_ref())
        .password(dsn.password.clone().unwrap_or_default().as_str())
        .database(dsn.database.clone().unwrap_or_default().as_ref());

    if let Some(host) = &dsn.host {
        options = options.host(host.as_str()).port(dsn.port.unwrap_or(3306));
    } else if let Some(socket) = &dsn.socket {
        options = options.socket(socket.as_str());
    }

    options = match tls.mode {
        TlsMode::Disable => options.ssl_mode(MySqlSslMode::Disabled),
        TlsMode::Require => options.ssl_mode(MySqlSslMode::Required),
        TlsMode::VerifyCA => {
            let mut opts = options.ssl_mode(MySqlSslMode::VerifyCa);
            if let Some(ca_path) = &tls.ca {
                opts = opts.ssl_ca(ca_path);
            }
            opts
        }
        TlsMode::VerifyFull => {
            let mut opts = options.ssl_mode(MySqlSslMode::VerifyIdentity);
            if let Some(ca_path) = &tls.ca {
                opts = opts.ssl_ca(ca_path);
            }
            opts
        }
    };

    if let (Some(cert_path), Some(key_path)) = (&tls.cert, &tls.key) {
        options = options.ssl_client_cert(cert_path).ssl_client_key(key_path);
    }

    options
}

fn record_mysql_connect_metrics(tls: &TlsConfig, connect_timer: Instant) {
    let connect_duration = connect_timer.elapsed().as_secs_f64();
    OPERATION_DURATION
        .with_label_values(&["mysql", "connect"])
        .observe(connect_duration);
    if tls.mode.is_enabled() {
        TLS_HANDSHAKE_DURATION
            .with_label_values(&["mysql"])
            .observe(connect_duration);
    }
}

async fn connect_mysql(
    options: &MySqlConnectOptions,
    dsn: &DSN,
    tls: &TlsConfig,
) -> Result<MySqlConnection> {
    let connect_timer = Instant::now();
    match options.connect().await {
        Ok(conn) => {
            record_mysql_connect_metrics(tls, connect_timer);
            Ok(conn)
        }
        Err(err) => {
            if let sqlx::Error::Database(db_err) = err {
                if db_err
                    .as_error()
                    .downcast_ref::<MySqlDatabaseError>()
                    .map(MySqlDatabaseError::number)
                    == Some(1049)
                {
                    create_mysql_database(options, dsn).await?;
                    let conn = options.connect().await?;
                    record_mysql_connect_metrics(tls, connect_timer);
                    Ok(conn)
                } else {
                    Err(db_err.into())
                }
            } else {
                Err(err.into())
            }
        }
    }
}

async fn create_mysql_database(options: &MySqlConnectOptions, dsn: &DSN) -> Result<()> {
    // The name is interpolated unquoted, so hold it to the same identifier
    // rules as the table name: anything else used to fail with a bare syntax
    // error from the server.
    let database = dsn.database.clone().unwrap_or_default();
    validate_identifier("database name", &database)?;

    let tmp_options = options.clone().database("mysql");
    let mut tmp_conn = tmp_options.connect().await?;
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .execute(&mut tmp_conn)
        .await?;
    Ok(())
}

async fn set_mysql_session_timeouts(conn: &mut MySqlConnection) -> Result<()> {
    if sqlx::query("SET SESSION max_execution_time = 5000")
        .execute(&mut *conn)
        .await
        .is_err()
    {
        let _ = sqlx::query("SET SESSION max_statement_time = 5")
            .execute(&mut *conn)
            .await;
    }

    sqlx::query("SET SESSION innodb_lock_wait_timeout = 2")
        .execute(&mut *conn)
        .await
        .context("Failed to set innodb_lock_wait_timeout")?;
    Ok(())
}

async fn fetch_mysql_health_info(conn: &mut MySqlConnection) -> Result<MySqlHealthInfo> {
    let version: Option<String> = sqlx::query_scalar("SELECT VERSION()")
        .fetch_optional(&mut *conn)
        .await
        .context("Failed to fetch database version")?;
    let db_host: Option<String> = sqlx::query_scalar("SELECT @@hostname")
        .fetch_optional(&mut *conn)
        .await
        .ok()
        .flatten();
    let uptime_seconds = sqlx::query("SHOW GLOBAL STATUS LIKE 'Uptime'")
        .fetch_optional(&mut *conn)
        .await
        .ok()
        .flatten()
        .and_then(|row| {
            row.try_get::<String, _>("Value")
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
        });

    Ok(MySqlHealthInfo {
        version,
        db_host,
        uptime_seconds,
    })
}

async fn mysql_is_read_only(conn: &mut MySqlConnection) -> Result<bool> {
    let row = sqlx::query("SELECT @@read_only;")
        .fetch_one(&mut *conn)
        .await
        .context("Failed to check if the database is in read-only mode")?;

    Ok(row.try_get::<i64, _>(0).map_or_else(
        |_| {
            row.try_get::<String, _>(0)
                .is_ok_and(|val| val.to_uppercase() == "ON" || val == "1")
        },
        |val| val != 0,
    ))
}

/// Column reporting replica delay.
///
/// MySQL renamed this to `Seconds_Behind_Source` in 8.0.22; MariaDB added
/// `SHOW REPLICA STATUS` as an alias but kept the original column name, so
/// reading only the new name silently reports no lag on every MariaDB replica.
const REPLICA_LAG_COLUMNS: [&str; 2] = ["Seconds_Behind_Source", "Seconds_Behind_Master"];

/// MariaDB only learned `SHOW REPLICA STATUS` in 10.5, and MySQL deprecated
/// `SHOW SLAVE STATUS` in 8.0.22. Neither name covers every supported server,
/// so try the modern spelling first and fall back on the older one when the
/// server rejects it outright.
const REPLICA_STATUS_STATEMENTS: [&str; 2] = ["SHOW REPLICA STATUS", "SHOW SLAVE STATUS"];

async fn maybe_record_mysql_replication_lag(conn: &mut MySqlConnection) {
    let mut lag = None;

    for statement in REPLICA_STATUS_STATEMENTS {
        match sqlx::query(statement).fetch_optional(&mut *conn).await {
            // Understood, and this server is a replica.
            Ok(Some(row)) => {
                lag = REPLICA_LAG_COLUMNS
                    .iter()
                    .find_map(|column| row.try_get::<Option<i64>, _>(*column).ok().flatten());
                break;
            }
            // Understood, but no replica configured: nothing to report.
            Ok(None) => break,
            // Statement unknown on this server version; try the other spelling.
            Err(_) => {}
        }
    }

    // A NULL or absent value means replication is not running. Report nothing
    // rather than leaving the previous sample frozen in place, which would
    // describe a halted replica as healthy for as long as the process lives.
    record_replication_lag("mysql", lag);
}

async fn mysql_read_only_result(
    dsn: &DSN,
    tls: &TlsConfig,
    conn: &mut MySqlConnection,
    cert_cache: &CertCache,
    health_info: MySqlHealthInfo,
) -> Result<HealthCheckResult> {
    let tls_metadata = maybe_extract_mysql_tls(dsn, tls, conn, cert_cache).await;
    Ok(HealthCheckResult {
        version: health_info.version.unwrap_or_default(),
        db_host: health_info.db_host,
        uptime_seconds: health_info.uptime_seconds,
        tls_metadata,
        read_only: true,
        read_only_reason: Some("Database is in read-only mode".to_string()),
    })
}

async fn monitor_mysql_blocked_sessions(conn: &mut MySqlConnection) {
    // Absent beats stale. Updating only on success left the gauge frozen at the
    // last good reading, so a server that stopped answering this query kept
    // reporting a reassuring 0 -- or, worse, held a spike forever after the
    // contention had cleared.
    //
    // `LIKE '%lock%'` is already case-insensitive under MySQL's default
    // collation, so the `'%Locked%'` alternative it was OR-ed with matched
    // nothing the first pattern had not matched already.
    match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM information_schema.processlist WHERE state LIKE '%lock%'",
    )
    .fetch_optional(&mut *conn)
    .await
    {
        Ok(Some(blocked_count)) => {
            BLOCKED_SESSIONS
                .with_label_values(&["mysql"])
                .set(blocked_count);
        }
        _ => {
            let _ = BLOCKED_SESSIONS.remove_label_values(&["mysql"]);
        }
    }
}

async fn ensure_mysql_table(conn: &mut MySqlConnection, table_name: &str) -> Result<()> {
    let create_table_sql = format!(
        r"
        CREATE TABLE IF NOT EXISTS {table_name} (
            id INT NOT NULL,
            t1 BIGINT NOT NULL,
            t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            uuid CHAR(36) CHARACTER SET ascii,
            PRIMARY KEY(id),
            UNIQUE KEY(uuid),
            INDEX idx_t2 (t2)
        ) ENGINE=InnoDB
        "
    );
    let create_table_timer = Instant::now();
    conn.execute(AssertSqlSafe(create_table_sql)).await?;
    OPERATION_DURATION
        .with_label_values(&["mysql", "create_table"])
        .observe(create_table_timer.elapsed().as_secs_f64());
    Ok(())
}

async fn mysql_insert_and_verify(
    conn: &mut MySqlConnection,
    now: DateTime<Utc>,
    range: u32,
    table_name: &str,
) -> Result<u32> {
    let id: u32 = rand::rng().random_range(0..range);
    let uuid = Uuid::new_v4();

    let insert_sql = format!(
        r"
        INSERT INTO {table_name} (id, t1, uuid)
        VALUES (?, ?, ?)
        ON DUPLICATE KEY UPDATE
        t1 = VALUES(t1), uuid = VALUES(uuid)
        "
    );
    let insert_timer = Instant::now();
    let insert_result = sqlx::query(AssertSqlSafe(insert_sql))
        .bind(id)
        .bind(now.timestamp())
        .bind(uuid.to_string())
        .execute(&mut *conn)
        .await?;
    OPERATION_DURATION
        .with_label_values(&["mysql", "insert"])
        .observe(insert_timer.elapsed().as_secs_f64());
    ROWS_AFFECTED
        .with_label_values(&["mysql", "insert"])
        .inc_by(insert_result.rows_affected());

    let select_sql = format!(
        r"
        SELECT t1, uuid
        FROM {table_name}
        WHERE id = ?
        "
    );
    let select_timer = Instant::now();
    let row: Option<(i64, String)> = sqlx::query_as(AssertSqlSafe(select_sql))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .context("Failed to query the database")?;
    OPERATION_DURATION
        .with_label_values(&["mysql", "select"])
        .observe(select_timer.elapsed().as_secs_f64());

    // The row just written is gone: another instance's cleanup or hourly drop
    // removed it between the upsert and the read-back (a drop surfaces as
    // error 1146 and takes the recovery path instead). Only dbpulse deletes
    // from this table, so like any other concurrent interference on the shared
    // table this is counted, not paged on.
    let Some((t1, v4)) = row else {
        RW_ROW_CONTENTION.with_label_values(&["mysql"]).inc();
        return Ok(id);
    };
    match classify_read_back(now.timestamp(), &uuid.to_string(), t1, &v4) {
        ReadBack::Match => {}
        ReadBack::ConcurrentOverwrite => {
            RW_ROW_CONTENTION.with_label_values(&["mysql"]).inc();
        }
        ReadBack::Mismatch => {
            return Err(anyhow!(
                "Records don't match: expected ({}, {}), got ({}, {})",
                now.timestamp(),
                uuid,
                t1,
                v4
            ));
        }
    }

    Ok(id)
}

async fn mysql_transaction_rollback_test(
    conn: &mut MySqlConnection,
    now: DateTime<Utc>,
    table_name: &str,
) -> Result<()> {
    let rollback_seed = now.timestamp_micros().rem_euclid(i64::from(i32::MAX));
    let rollback_test_id =
        i32::try_from(rollback_seed).context("rollback test id out of range for MySQL INT")?;
    let rollback_uuid = Uuid::new_v4().to_string();

    let transaction_timer = Instant::now();
    let mut tx = conn.begin().await?;
    let insert_tx_sql = format!(
        "INSERT INTO {table_name} (id, t1, uuid) VALUES (?, 999, ?) ON DUPLICATE KEY UPDATE t1 = 999"
    );
    sqlx::query(AssertSqlSafe(insert_tx_sql))
        .bind(rollback_test_id)
        .bind(rollback_uuid)
        .execute(tx.as_mut())
        .await?;

    let update_tx_sql = format!("UPDATE {table_name} SET t1 = ? WHERE id = ?");
    sqlx::query(AssertSqlSafe(update_tx_sql))
        .bind(0)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    let select_tx_sql = format!("SELECT t1 FROM {table_name} WHERE id = ?");
    let updated_value: Option<i64> = sqlx::query_scalar(AssertSqlSafe(select_tx_sql))
        .bind(rollback_test_id)
        .fetch_optional(tx.as_mut())
        .await?;
    if updated_value != Some(0) {
        return Err(anyhow!(
            "Transaction update failed: expected 0, got {updated_value:?}"
        ));
    }
    tx.rollback().await?;

    let select_rollback_sql = format!("SELECT t1 FROM {table_name} WHERE id = ?");
    let rolled_back_value: Option<i64> = sqlx::query_scalar(AssertSqlSafe(select_rollback_sql))
        .bind(rollback_test_id)
        .fetch_optional(&mut *conn)
        .await?;
    if rolled_back_value == Some(0) {
        return Err(anyhow!("Transaction rollback failed: value is still 0"));
    }

    OPERATION_DURATION
        .with_label_values(&["mysql", "transaction_test"])
        .observe(transaction_timer.elapsed().as_secs_f64());
    Ok(())
}

async fn mysql_cleanup_old_records(conn: &mut MySqlConnection, table_name: &str) {
    // Cutoff computed by the server. Binding an RFC3339 string here made MySQL
    // emit "Truncated incorrect datetime value" and silently drop the timezone
    // offset, so the window was interpreted in the session timezone and deleted
    // rows far newer than an hour old on any server not running in UTC.
    let delete_old_sql =
        format!("DELETE FROM {table_name} WHERE t2 < NOW() - INTERVAL 1 HOUR LIMIT 10000");
    let cleanup_timer = Instant::now();
    if let Ok(delete_result) = sqlx::query(AssertSqlSafe(delete_old_sql))
        .execute(&mut *conn)
        .await
    {
        ROWS_AFFECTED
            .with_label_values(&["mysql", "delete"])
            .inc_by(delete_result.rows_affected());
    }
    OPERATION_DURATION
        .with_label_values(&["mysql", "cleanup"])
        .observe(cleanup_timer.elapsed().as_secs_f64());
}

async fn update_mysql_table_rows_metric(conn: &mut MySqlConnection, table_name: &str) {
    let row_count_sql = format!(
        "SELECT CAST(table_rows AS SIGNED) FROM information_schema.TABLES WHERE table_schema = DATABASE() AND table_name = '{table_name}'"
    );
    match sqlx::query_scalar::<_, Option<i64>>(AssertSqlSafe(row_count_sql))
        .fetch_optional(&mut *conn)
        .await
    {
        Ok(Some(Some(row_count))) => {
            TABLE_ROWS
                .with_label_values(&["mysql", table_name])
                .set(row_count);
        }
        Ok(Some(None) | None) => {
            // information_schema has no estimate (or no row at all, meaning the
            // table is gone). Fall back to an exact count, and retire the series
            // if even that yields nothing rather than leaving a stale figure.
            let count_sql = format!("SELECT COUNT(*) FROM {table_name}");
            let exact = sqlx::query_scalar::<_, i64>(AssertSqlSafe(count_sql))
                .fetch_optional(&mut *conn)
                .await
                .ok()
                .flatten();
            set_or_retire(&TABLE_ROWS, &["mysql", table_name], exact);
        }
        Err(e) => {
            eprintln!("Error querying table_rows for '{table_name}': {e}");
            set_or_retire(&TABLE_ROWS, &["mysql", table_name], None);
        }
    }
}

async fn maybe_drop_mysql_table_hourly(
    conn: &mut MySqlConnection,
    now: DateTime<Utc>,
    id: u32,
    table_name: &str,
) {
    if now.minute() != 0 || id >= 5 {
        return;
    }

    let count_sql = format!("SELECT COUNT(*) FROM {table_name}");
    let exact_count = match sqlx::query_scalar::<_, i64>(AssertSqlSafe(count_sql))
        .fetch_optional(&mut *conn)
        .await
    {
        Ok(Some(count)) => count,
        // No row means the table is gone; another instance already dropped it,
        // which is the outcome this function was trying to reach.
        Ok(None) => return,
        Err(_) => {
            TABLE_MAINTENANCE_ERRORS
                .with_label_values(&["mysql", "count"])
                .inc();
            return;
        }
    };

    if exact_count >= 100_000 {
        return;
    }

    let drop_table_sql = format!("DROP TABLE IF EXISTS {table_name}");
    if sqlx::query(AssertSqlSafe(drop_table_sql))
        .execute(&mut *conn)
        .await
        .is_err()
    {
        TABLE_MAINTENANCE_ERRORS
            .with_label_values(&["mysql", "drop"])
            .inc();
    }
}

async fn update_mysql_size_metrics(conn: &mut MySqlConnection, table_name: &str) {
    let size_sql = format!(
        "SELECT CAST(COALESCE(data_length, 0) + COALESCE(index_length, 0) AS SIGNED) FROM information_schema.TABLES WHERE table_schema = DATABASE() AND table_name = '{table_name}'"
    );
    match sqlx::query_scalar::<_, i64>(AssertSqlSafe(size_sql))
        .fetch_optional(&mut *conn)
        .await
    {
        Ok(Some(table_bytes)) => {
            TABLE_SIZE_BYTES
                .with_label_values(&["mysql", table_name])
                .set(table_bytes);
        }
        Ok(None) => {
            // The table is absent from information_schema, so it is gone --
            // and its size series must be too, matching the PostgreSQL path.
            // Reporting 0 would freeze a wrong-but-plausible value in place.
            let _ = TABLE_SIZE_BYTES.remove_label_values(&["mysql", table_name]);
        }
        Err(e) => {
            eprintln!("Error querying table size for '{table_name}': {e}");
            let _ = TABLE_SIZE_BYTES.remove_label_values(&["mysql", table_name]);
        }
    }

    match sqlx::query_scalar::<_, i64>(
        "SELECT CAST(SUM(COALESCE(data_length, 0) + COALESCE(index_length, 0)) AS SIGNED) FROM information_schema.TABLES WHERE table_schema = DATABASE()",
    )
    .fetch_optional(&mut *conn)
    .await
    {
        Ok(Some(db_size)) => {
            DATABASE_SIZE_BYTES.with_label_values(&["mysql"]).set(db_size);
        }
        _ => {
            let _ = DATABASE_SIZE_BYTES.remove_label_values(&["mysql"]);
        }
    }
}

async fn maybe_extract_mysql_tls(
    dsn: &DSN,
    tls: &TlsConfig,
    conn: &mut MySqlConnection,
    cert_cache: &CertCache,
) -> Option<TlsMetadata> {
    if tls.mode.is_enabled() {
        extract_tls_metadata(dsn, tls, conn, cert_cache).await.ok()
    } else {
        None
    }
}

/// Extract TLS metadata from `MySQL` connection
async fn extract_tls_metadata(
    dsn: &DSN,
    tls: &TlsConfig,
    conn: &mut sqlx::MySqlConnection,
    cert_cache: &CertCache,
) -> Result<TlsMetadata> {
    let mut cert_subject: Option<String> = None;
    let mut cert_issuer: Option<String> = None;
    let mut cert_expiry_days: Option<i64> = None;

    if tls.mode.is_enabled() {
        match get_cert_metadata_cached(dsn, 3306, TlsProbeProtocol::Mysql, tls, cert_cache).await {
            Ok(Some(probe_metadata)) => {
                cert_subject = probe_metadata.cert_subject;
                cert_issuer = probe_metadata.cert_issuer;
                cert_expiry_days = probe_metadata.cert_expiry_days;
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!("failed to probe MySQL TLS certificate: {err}");
                TLS_CERT_PROBE_ERRORS
                    .with_label_values(&["mysql", classify_cert_probe_error(&err)])
                    .inc();
            }
        }
    }

    // Query SSL status variables
    let rows = sqlx::query("SHOW STATUS LIKE 'Ssl%'")
        .fetch_all(conn)
        .await?;

    let mut tls_version: Option<String> = None;
    let mut tls_cipher: Option<String> = None;

    for row in rows {
        let variable_name: String = row.try_get(0)?;
        let value: String = row.try_get(1)?;

        match variable_name.as_str() {
            "Ssl_version" if !value.is_empty() => {
                tls_version = Some(value);
            }
            "Ssl_cipher" if !value.is_empty() => {
                tls_cipher = Some(value);
            }
            "Ssl_server_not_after" => {
                if cert_expiry_days.is_none()
                    && let Some(days) = parse_mysql_ssl_expiry(&value)
                {
                    cert_expiry_days = Some(days);
                }
            }
            _ => {}
        }
    }

    Ok(TlsMetadata {
        version: tls_version,
        cipher: tls_cipher,
        cert_subject,
        cert_issuer,
        cert_expiry_days,
    })
}

fn parse_mysql_ssl_expiry(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "0000-00-00 00:00:00" {
        return None;
    }

    for fmt in &MYSQL_SSL_DATE_FORMATS {
        if let Ok(ts) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            let expiry = DateTime::<Utc>::from_naive_utc_and_offset(ts, Utc);
            // Floored via the shared helper, exactly like the probe path:
            // `Duration::num_days` truncates toward zero, so a certificate
            // expired less than 24h ago reported `0`, which matches neither
            // the "expiring" alert (`< 30 and > 0`) nor the "expired" one
            // (`< 0`). This fallback is what unix-socket connections and
            // probe failures rely on.
            return Some(expiry_days_from_remaining(expiry - Utc::now()));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_parse_mysql_ssl_expiry_valid_formats() {
        assert!(parse_mysql_ssl_expiry("Jan  1 00:00:00 2100 GMT").is_some());
        assert!(parse_mysql_ssl_expiry("2100-01-01 00:00:00").is_some());
    }

    #[test]
    fn test_parse_mysql_ssl_expiry_invalid_formats() {
        assert_eq!(parse_mysql_ssl_expiry(""), None);
        assert_eq!(parse_mysql_ssl_expiry("0000-00-00 00:00:00"), None);
        assert_eq!(parse_mysql_ssl_expiry("not a date"), None);
    }

    /// Regression: this fallback used `Duration::num_days`, which truncates
    /// toward zero, so a certificate expired less than 24 hours ago reported
    /// `0` days -- matching neither the documented "expiring" alert
    /// (`< 30 and > 0`) nor the "expired" one (`< 0`). This is the path that
    /// feeds `dbpulse_tls_cert_expiry_days` when the probe returns nothing
    /// (unix-socket connections, probe failures).
    #[test]
    fn a_just_expired_certificate_reports_a_negative_day_count() {
        for hours_ago in [1, 6, 23] {
            let then = (Utc::now() - chrono::Duration::hours(hours_ago))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
            let days =
                parse_mysql_ssl_expiry(&then).unwrap_or_else(|| panic!("failed to parse `{then}`"));
            assert!(
                days < 0,
                "expired {hours_ago}h ago must report a negative day count, got {days}"
            );
        }
    }

    /// The other half of the boundary: still valid, however briefly, must not
    /// be reported as expired.
    #[test]
    fn a_certificate_with_less_than_a_day_left_reports_zero() {
        let soon = (Utc::now() + chrono::Duration::hours(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        assert_eq!(
            parse_mysql_ssl_expiry(&soon),
            Some(0),
            "1h left must floor to 0"
        );
    }
}
