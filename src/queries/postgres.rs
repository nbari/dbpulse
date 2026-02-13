use super::HealthCheckResult;
use crate::{
    metrics::{
        BLOCKING_QUERIES, CONNECTION_DURATION, DATABASE_SIZE_BYTES, OPERATION_DURATION,
        REPLICATION_LAG, ROWS_AFFECTED, TABLE_ROWS, TABLE_SIZE_BYTES, TLS_CERT_PROBE_ERRORS,
        TLS_HANDSHAKE_DURATION,
    },
    tls::{
        TlsConfig, TlsMetadata, TlsMode, TlsProbeProtocol,
        cache::{CertCache, get_cert_metadata_cached},
        ensure_crypto_provider,
    },
};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc, prelude::*};
use dsn::DSN;
use rand::Rng;
use sqlx::{
    ConnectOptions, Connection, Row,
    postgres::{PgConnectOptions, PgDatabaseError, PgSslMode},
};
use std::time::Instant;
use uuid::Uuid;

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
    ensure_crypto_provider();
    let options = postgres_connect_options(dsn, tls);
    let conn_start = Instant::now();
    let mut conn = connect_postgres(&options, dsn, tls).await?;
    set_postgres_session_timeouts(&mut conn).await?;

    let health_info = fetch_postgres_health_info(&mut conn).await?;
    if postgres_is_in_recovery(&mut conn).await? {
        maybe_record_postgres_replication_lag(&mut conn).await;
        return postgres_read_only_result(
            dsn,
            tls,
            &mut conn,
            cert_cache,
            health_info,
            "Database is in recovery mode",
        )
        .await;
    }
    if postgres_transaction_is_read_only(&mut conn).await? {
        return postgres_read_only_result(
            dsn,
            tls,
            &mut conn,
            cert_cache,
            health_info,
            "Transaction read-only mode enabled",
        )
        .await;
    }

    monitor_postgres_blocking_queries(&mut conn).await;
    ensure_postgres_uuid_extension(&mut conn).await?;
    ensure_postgres_table(&mut conn, table_name).await?;
    let id = postgres_insert_and_verify(&mut conn, now, range, table_name).await?;
    postgres_transaction_rollback_test(&mut conn, now, table_name).await?;
    postgres_cleanup_old_records(&mut conn, table_name).await;
    update_postgres_table_rows_metric(&mut conn, table_name).await;
    maybe_drop_postgres_table_hourly(&mut conn, now, id, table_name).await;
    update_postgres_size_metrics(&mut conn, table_name).await;

    let tls_metadata = maybe_extract_postgres_tls(dsn, tls, &mut conn, cert_cache).await;
    let _ = conn.close().await;
    CONNECTION_DURATION.observe(conn_start.elapsed().as_secs_f64());

    Ok(HealthCheckResult {
        version: health_info.version.context("Expected database version")?,
        db_host: health_info.db_host,
        uptime_seconds: health_info.uptime_seconds,
        tls_metadata,
    })
}

struct PostgresHealthInfo {
    version: Option<String>,
    db_host: Option<String>,
    uptime_seconds: Option<i64>,
}

fn postgres_connect_options(dsn: &DSN, tls: &TlsConfig) -> PgConnectOptions {
    let mut options = PgConnectOptions::new()
        .username(dsn.username.clone().unwrap_or_default().as_ref())
        .password(dsn.password.clone().unwrap_or_default().as_str())
        .database(dsn.database.clone().unwrap_or_default().as_ref());

    if let Some(host) = &dsn.host {
        options = options.host(host.as_str()).port(dsn.port.unwrap_or(5432));
    } else if let Some(socket) = &dsn.socket {
        options = options.socket(socket.as_str());
    }

    options = match tls.mode {
        TlsMode::Disable => options.ssl_mode(PgSslMode::Disable),
        TlsMode::Require => options.ssl_mode(PgSslMode::Require),
        TlsMode::VerifyCA => {
            let mut opts = options.ssl_mode(PgSslMode::VerifyCa);
            if let Some(ca_path) = &tls.ca {
                opts = opts.ssl_root_cert(ca_path);
            }
            opts
        }
        TlsMode::VerifyFull => {
            let mut opts = options.ssl_mode(PgSslMode::VerifyFull);
            if let Some(ca_path) = &tls.ca {
                opts = opts.ssl_root_cert(ca_path);
            }
            opts
        }
    };

    if let (Some(cert_path), Some(key_path)) = (&tls.cert, &tls.key) {
        options = options.ssl_client_cert(cert_path).ssl_client_key(key_path);
    }

    options
}

fn record_postgres_connect_metrics(tls: &TlsConfig, connect_timer: Instant) {
    let connect_duration = connect_timer.elapsed().as_secs_f64();
    OPERATION_DURATION
        .with_label_values(&["postgres", "connect"])
        .observe(connect_duration);
    if tls.mode.is_enabled() {
        TLS_HANDSHAKE_DURATION
            .with_label_values(&["postgres"])
            .observe(connect_duration);
    }
}

async fn connect_postgres(
    options: &PgConnectOptions,
    dsn: &DSN,
    tls: &TlsConfig,
) -> Result<sqlx::PgConnection> {
    let connect_timer = Instant::now();
    match options.connect().await {
        Ok(conn) => {
            record_postgres_connect_metrics(tls, connect_timer);
            Ok(conn)
        }
        Err(err) => {
            if let sqlx::Error::Database(db_err) = err {
                if db_err
                    .as_error()
                    .downcast_ref::<PgDatabaseError>()
                    .map(PgDatabaseError::code)
                    == Some("3D000")
                {
                    create_postgres_database(options, dsn).await?;
                    let conn = options.connect().await?;
                    record_postgres_connect_metrics(tls, connect_timer);
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

async fn create_postgres_database(options: &PgConnectOptions, dsn: &DSN) -> Result<()> {
    let tmp_options = options.clone().database("postgres");
    let mut tmp_conn = tmp_options.connect().await?;
    sqlx::query(&format!(
        "CREATE DATABASE {}",
        dsn.database.clone().unwrap_or_default()
    ))
    .execute(&mut tmp_conn)
    .await?;
    Ok(())
}

async fn set_postgres_session_timeouts(conn: &mut sqlx::PgConnection) -> Result<()> {
    sqlx::query("SET statement_timeout = '5s'")
        .execute(&mut *conn)
        .await
        .context("Failed to set statement timeout")?;
    sqlx::query("SET lock_timeout = '2s'")
        .execute(&mut *conn)
        .await
        .context("Failed to set lock timeout")?;
    Ok(())
}

async fn fetch_postgres_health_info(conn: &mut sqlx::PgConnection) -> Result<PostgresHealthInfo> {
    let version: Option<String> = sqlx::query_scalar("SHOW server_version")
        .fetch_optional(&mut *conn)
        .await
        .context("Failed to fetch database version")?;
    let db_host: Option<String> =
        sqlx::query_scalar("SELECT COALESCE(inet_server_addr()::text, 'local')")
            .fetch_optional(&mut *conn)
            .await
            .ok()
            .flatten();
    let uptime_seconds = sqlx::query_scalar::<_, i64>(
        "SELECT EXTRACT(EPOCH FROM NOW() - pg_postmaster_start_time())::bigint",
    )
    .fetch_optional(&mut *conn)
    .await
    .ok()
    .flatten();

    Ok(PostgresHealthInfo {
        version,
        db_host,
        uptime_seconds,
    })
}

async fn postgres_is_in_recovery(conn: &mut sqlx::PgConnection) -> Result<bool> {
    let (is_in_recovery,): (bool,) = sqlx::query_as("SELECT pg_is_in_recovery();")
        .fetch_one(&mut *conn)
        .await?;
    Ok(is_in_recovery)
}

async fn postgres_transaction_is_read_only(conn: &mut sqlx::PgConnection) -> Result<bool> {
    let (tx_read_only,): (String,) = sqlx::query_as("SHOW transaction_read_only;")
        .fetch_one(&mut *conn)
        .await?;
    Ok(tx_read_only.eq_ignore_ascii_case("on"))
}

async fn maybe_record_postgres_replication_lag(conn: &mut sqlx::PgConnection) {
    if let Ok(Some(lag_seconds)) = sqlx::query_scalar::<_, f64>(
        "SELECT EXTRACT(EPOCH FROM (NOW() - pg_last_xact_replay_timestamp()))",
    )
    .fetch_optional(&mut *conn)
    .await
    {
        REPLICATION_LAG
            .with_label_values(&["postgres"])
            .observe(lag_seconds);
    }
}

async fn postgres_read_only_result(
    dsn: &DSN,
    tls: &TlsConfig,
    conn: &mut sqlx::PgConnection,
    cert_cache: &CertCache,
    health_info: PostgresHealthInfo,
    reason: &str,
) -> Result<HealthCheckResult> {
    let tls_metadata = maybe_extract_postgres_tls(dsn, tls, conn, cert_cache).await;
    Ok(HealthCheckResult {
        version: format!("{} - {reason}", health_info.version.unwrap_or_default()),
        db_host: health_info.db_host,
        uptime_seconds: health_info.uptime_seconds,
        tls_metadata,
    })
}

async fn monitor_postgres_blocking_queries(conn: &mut sqlx::PgConnection) {
    if let Ok(Some(blocking_count)) = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock' AND state = 'active'",
    )
    .fetch_optional(&mut *conn)
    .await
    {
        BLOCKING_QUERIES
            .with_label_values(&["postgres"])
            .set(blocking_count);
    }
}

fn is_ignorable_postgres_create_error(error: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = error {
        db_err.message().contains("duplicate key") || db_err.message().contains("already exists")
    } else {
        false
    }
}

async fn ensure_postgres_uuid_extension(conn: &mut sqlx::PgConnection) -> Result<()> {
    if let Err(error) = sqlx::query("CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\"")
        .execute(&mut *conn)
        .await
    {
        if let sqlx::Error::Database(db_err) = &error {
            let code = db_err
                .as_error()
                .downcast_ref::<PgDatabaseError>()
                .map(PgDatabaseError::code);
            if code != Some("42710") && !db_err.message().contains("duplicate key") {
                return Err(error.into());
            }
        } else {
            return Err(error.into());
        }
    }
    Ok(())
}

async fn ensure_postgres_table(conn: &mut sqlx::PgConnection, table_name: &str) -> Result<()> {
    let create_table_sql = format!(
        r"
        CREATE TABLE IF NOT EXISTS {table_name} (
            id INT NOT NULL PRIMARY KEY,
            t1 BIGINT NOT NULL,
            t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP,
            uuid UUID NOT NULL,
            CONSTRAINT {table_name}_uuid_unique UNIQUE (uuid)
        )
        "
    );
    let create_table_timer = Instant::now();
    if let Err(error) = sqlx::query(&create_table_sql).execute(&mut *conn).await
        && !is_ignorable_postgres_create_error(&error)
    {
        return Err(error.into());
    }
    OPERATION_DURATION
        .with_label_values(&["postgres", "create_table"])
        .observe(create_table_timer.elapsed().as_secs_f64());

    let create_index_sql =
        format!("CREATE INDEX IF NOT EXISTS idx_{table_name}_t2 ON {table_name}(t2)");
    sqlx::query(&create_index_sql)
        .execute(&mut *conn)
        .await
        .ok();
    Ok(())
}

async fn postgres_insert_and_verify(
    conn: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
    range: u32,
    table_name: &str,
) -> Result<u32> {
    let id: u32 = rand::rng().random_range(0..range);
    let id_i32 = i32::try_from(id).context("generated id out of range for PostgreSQL INT")?;
    let uuid = Uuid::new_v4();

    let insert_sql = format!(
        r"
        INSERT INTO {table_name} (id, t1, uuid)
        VALUES ($1, $2, $3)
        ON CONFLICT (id)
        DO UPDATE SET t1 = EXCLUDED.t1, uuid = EXCLUDED.uuid
        "
    );
    let insert_timer = Instant::now();
    let insert_result = sqlx::query(&insert_sql)
        .bind(id_i32)
        .bind(now.timestamp())
        .bind(uuid)
        .execute(&mut *conn)
        .await?;
    OPERATION_DURATION
        .with_label_values(&["postgres", "insert"])
        .observe(insert_timer.elapsed().as_secs_f64());
    ROWS_AFFECTED
        .with_label_values(&["postgres", "insert"])
        .inc_by(insert_result.rows_affected());

    let select_sql = format!("SELECT t1, uuid FROM {table_name} WHERE id = $1");
    let select_timer = Instant::now();
    let row: Option<(i64, Uuid)> = sqlx::query_as(&select_sql)
        .bind(id_i32)
        .fetch_optional(&mut *conn)
        .await?;
    OPERATION_DURATION
        .with_label_values(&["postgres", "select"])
        .observe(select_timer.elapsed().as_secs_f64());

    let (t1, v4) = row.context("Expected records")?;
    if now.timestamp() != t1 || uuid != v4 {
        return Err(anyhow!(
            "Records don't match: expected ({}, {}), got ({}, {})",
            now.timestamp(),
            uuid,
            t1,
            v4
        ));
    }

    Ok(id)
}

async fn postgres_transaction_rollback_test(
    conn: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
    table_name: &str,
) -> Result<()> {
    let rollback_seed = now.timestamp_micros().rem_euclid(i64::from(i32::MAX));
    let rollback_test_id =
        i32::try_from(rollback_seed).context("rollback test id out of range for PostgreSQL INT")?;

    let transaction_timer = Instant::now();
    let mut tx = conn.begin().await?;
    let insert_tx_sql = format!(
        "INSERT INTO {table_name} (id, t1, uuid) VALUES ($1, 999, UUID_GENERATE_V4()) ON CONFLICT (id) DO UPDATE SET t1 = 999"
    );
    sqlx::query(&insert_tx_sql)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    let update_tx_sql = format!("UPDATE {table_name} SET t1 = $1 WHERE id = $2");
    sqlx::query(&update_tx_sql)
        .bind(0)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    let select_tx_sql = format!("SELECT t1 FROM {table_name} WHERE id = $1");
    let updated_value: Option<i64> = sqlx::query_scalar(&select_tx_sql)
        .bind(rollback_test_id)
        .fetch_optional(tx.as_mut())
        .await?;
    if updated_value != Some(0) {
        return Err(anyhow!(
            "Transaction update failed: expected 0, got {updated_value:?}"
        ));
    }
    tx.rollback().await?;

    let select_rollback_sql = format!("SELECT t1 FROM {table_name} WHERE id = $1");
    let rolled_back_value: Option<i64> = sqlx::query_scalar(&select_rollback_sql)
        .bind(rollback_test_id)
        .fetch_optional(&mut *conn)
        .await?;
    if rolled_back_value == Some(0) {
        return Err(anyhow!("Transaction rollback failed: value is still 0"));
    }

    OPERATION_DURATION
        .with_label_values(&["postgres", "transaction_test"])
        .observe(transaction_timer.elapsed().as_secs_f64());
    Ok(())
}

async fn postgres_cleanup_old_records(conn: &mut sqlx::PgConnection, table_name: &str) {
    let delete_old_sql = format!(
        "DELETE FROM {table_name} WHERE id IN (SELECT id FROM {table_name} WHERE t2 < NOW() - INTERVAL '1 hour' LIMIT 10000)"
    );
    let cleanup_timer = Instant::now();
    if let Ok(delete_result) = sqlx::query(&delete_old_sql).execute(&mut *conn).await {
        ROWS_AFFECTED
            .with_label_values(&["postgres", "delete"])
            .inc_by(delete_result.rows_affected());
    }
    OPERATION_DURATION
        .with_label_values(&["postgres", "cleanup"])
        .observe(cleanup_timer.elapsed().as_secs_f64());
}

async fn update_postgres_table_rows_metric(conn: &mut sqlx::PgConnection, table_name: &str) {
    let row_count_sql = format!(
        "SELECT c.reltuples::bigint FROM pg_class c \
         JOIN pg_namespace n ON c.relnamespace = n.oid \
         WHERE c.relname = '{table_name}' AND n.nspname = CURRENT_SCHEMA()"
    );
    if let Ok(Some(row_count)) = sqlx::query_scalar::<_, i64>(&row_count_sql)
        .fetch_optional(&mut *conn)
        .await
    {
        TABLE_ROWS
            .with_label_values(&["postgres", table_name])
            .set(row_count);
    }
}

async fn maybe_drop_postgres_table_hourly(
    conn: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
    id: u32,
    table_name: &str,
) {
    if now.minute() == 0 && id < 5 {
        let count_sql = format!("SELECT COUNT(*) FROM {table_name}");
        if let Ok(Some(exact_count)) = sqlx::query_scalar::<_, i64>(&count_sql)
            .fetch_optional(&mut *conn)
            .await
            && exact_count < 100_000
        {
            let drop_table_sql = format!("DROP TABLE IF EXISTS {table_name}");
            sqlx::query(&drop_table_sql).execute(&mut *conn).await.ok();
        }
    }
}

async fn update_postgres_size_metrics(conn: &mut sqlx::PgConnection, table_name: &str) {
    let size_sql = format!("SELECT pg_total_relation_size('{table_name}')");
    if let Ok(Some(table_bytes)) = sqlx::query_scalar::<_, i64>(&size_sql)
        .fetch_optional(&mut *conn)
        .await
    {
        TABLE_SIZE_BYTES
            .with_label_values(&["postgres", table_name])
            .set(table_bytes);
    }

    if let Ok(Some(db_size)) =
        sqlx::query_scalar::<_, i64>("SELECT pg_database_size(current_database())")
            .fetch_optional(&mut *conn)
            .await
    {
        DATABASE_SIZE_BYTES
            .with_label_values(&["postgres"])
            .set(db_size);
    }
}

async fn maybe_extract_postgres_tls(
    dsn: &DSN,
    tls: &TlsConfig,
    conn: &mut sqlx::PgConnection,
    cert_cache: &CertCache,
) -> Option<TlsMetadata> {
    if tls.mode.is_enabled() {
        extract_tls_metadata(dsn, tls, conn, cert_cache).await.ok()
    } else {
        None
    }
}

/// Extract TLS metadata from `PostgreSQL` connection
async fn extract_tls_metadata(
    dsn: &DSN,
    tls: &TlsConfig,
    conn: &mut sqlx::PgConnection,
    cert_cache: &CertCache,
) -> Result<TlsMetadata> {
    // Query pg_stat_ssl for TLS information
    let row = sqlx::query("SELECT version, cipher FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
        .fetch_optional(conn)
        .await?;

    let mut metadata = row.map_or_else(TlsMetadata::default, |row| {
        let version: Option<String> = row.try_get(0).ok();
        let cipher: Option<String> = row.try_get(1).ok();

        TlsMetadata {
            version,
            cipher,
            ..Default::default()
        }
    });

    if tls.mode.is_enabled() {
        match get_cert_metadata_cached(dsn, 5432, TlsProbeProtocol::Postgres, tls, cert_cache).await
        {
            Ok(Some(probe_metadata)) => {
                // Merge probe metadata (subject, issuer, expiry) with connection metadata (version, cipher)
                metadata.cert_subject = probe_metadata.cert_subject;
                metadata.cert_issuer = probe_metadata.cert_issuer;
                metadata.cert_expiry_days = probe_metadata.cert_expiry_days;
            }
            Ok(None) => {}
            Err(err) => {
                eprintln!("failed to probe PostgreSQL TLS certificate: {err}");
                // Track certificate probe errors by type
                let error_type = if err.to_string().contains("connect")
                    || err.to_string().contains("Connection")
                {
                    "connection"
                } else if err.to_string().contains("handshake") || err.to_string().contains("TLS") {
                    "handshake"
                } else if err.to_string().contains("timeout") {
                    "timeout"
                } else if err.to_string().contains("parse")
                    || err.to_string().contains("certificate")
                {
                    "parse"
                } else {
                    "unknown"
                };
                TLS_CERT_PROBE_ERRORS
                    .with_label_values(&["postgres", error_type])
                    .inc();
            }
        }
    }

    Ok(metadata)
}
