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
    },
};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc, prelude::*};
use dsn::DSN;
use rand::RngExt;
use sqlx::{
    AssertSqlSafe, ConnectOptions, Connection, Row,
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
    validate_identifier("table name", table_name)?;
    validate_range(range)?;
    ensure_crypto_provider();
    let options = postgres_connect_options(dsn, tls);
    let conn_start = Instant::now();
    let mut conn = connect_postgres(&options, dsn, tls).await?;
    set_postgres_session_timeouts(&mut conn).await?;

    let health_info = fetch_postgres_health_info(&mut conn).await?;

    // Recorded on every check rather than only while in recovery: a gauge that
    // stops being written keeps its last value, so a replica promoted to
    // primary would otherwise report its final pre-promotion lag forever.
    maybe_record_postgres_replication_lag(&mut conn).await;

    if postgres_is_in_recovery(&mut conn).await? {
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

    monitor_postgres_blocked_sessions(&mut conn).await;
    let id = postgres_read_write_cycle_resilient(&mut conn, now, range, table_name).await?;
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
        read_only: false,
        read_only_reason: None,
    })
}

/// `PostgreSQL` SQLSTATE 42P01: undefined table.
const POSTGRES_ERR_UNDEFINED_TABLE: &str = "42P01";

/// Did this fail only because the monitoring table was not there?
fn is_missing_table_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<sqlx::Error>()
        .and_then(|err| match err {
            sqlx::Error::Database(db_err) => db_err.as_error().downcast_ref::<PgDatabaseError>(),
            _ => None,
        })
        .is_some_and(|db_err| db_err.code() == POSTGRES_ERR_UNDEFINED_TABLE)
}

/// The write/read half of a check: create the table if needed, prove the server
/// accepts a write, read it back, and prove a transaction rolls back.
async fn postgres_read_write_cycle(
    conn: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
    range: u32,
    table_name: &str,
) -> Result<u32> {
    ensure_postgres_table(conn, table_name).await?;
    let id = postgres_insert_and_verify(conn, now, range, table_name).await?;
    postgres_transaction_rollback_test(conn, now, table_name).await?;
    Ok(id)
}

/// Run the read/write cycle, tolerating the table disappearing underneath it.
///
/// dbpulse drops its own table on the hour to exercise DDL. Instances share one
/// table, so a second instance can be mid-check when that DROP lands and see
/// "relation does not exist" for a database that is perfectly healthy.
/// Recreating and retrying once keeps that from being reported as a fault,
/// while a genuine failure still surfaces on the retry. The recovery is counted
/// so it stays visible rather than silent.
async fn postgres_read_write_cycle_resilient(
    conn: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
    range: u32,
    table_name: &str,
) -> Result<u32> {
    match postgres_read_write_cycle(conn, now, range, table_name).await {
        Err(error) if is_missing_table_error(&error) => {
            TABLE_RECREATED.with_label_values(&["postgres"]).inc();
            postgres_read_write_cycle(conn, now, range, table_name).await
        }
        result => result,
    }
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
    // The name is interpolated unquoted, so hold it to the same identifier
    // rules as the table name: anything else used to fail with a bare syntax
    // error from the server.
    let database = dsn.database.clone().unwrap_or_default();
    validate_identifier("database name", &database)?;

    let tmp_options = options.clone().database("postgres");
    let mut tmp_conn = tmp_options.connect().await?;
    sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {database}")))
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

/// Distance a standby is behind its primary, in whole seconds.
///
/// `NULL` on a primary, and on a standby that has never streamed. Exposed so
/// the regression test can substitute the volatile calls and assert the whole
/// truth table against a live server rather than a copy that can drift.
///
/// The walreceiver guard keys off **row existence**, not `status`. Verified
/// against a live streaming pair on PostgreSQL 18:
///
/// | state        | privilege    | rows | status      |
/// |--------------|--------------|------|-------------|
/// | streaming    | superuser    | 1    | `streaming` |
/// | streaming    | unprivileged | 1    | `NULL`      |
/// | disconnected | either       | 0    | --          |
///
/// `status` is privilege-restricted, so testing `status = 'streaming'` alone
/// would report a growing lag for a perfectly healthy standby whenever the
/// monitoring role is not a superuser -- which is the normal setup, and a
/// worse failure than the one being fixed. Accepting a visible row with a
/// `NULL` status keeps that case correct while still catching a receiver the
/// server reports as stopped.
pub const REPLICATION_LAG_SQL: &str = "SELECT CASE
             WHEN NOT pg_is_in_recovery() THEN NULL
             WHEN pg_last_wal_receive_lsn() IS NULL AND pg_last_wal_replay_lsn() IS NULL THEN NULL
             WHEN pg_last_wal_receive_lsn() IS NOT DISTINCT FROM pg_last_wal_replay_lsn()
                  AND EXISTS (
                      SELECT 1 FROM pg_stat_wal_receiver
                      WHERE status IS NULL OR status = 'streaming'
                  ) THEN 0
             ELSE GREATEST(
                 0,
                 CAST(EXTRACT(EPOCH FROM (NOW() - pg_last_xact_replay_timestamp())) AS BIGINT)
             )
         END";

async fn maybe_record_postgres_replication_lag(conn: &mut sqlx::PgConnection) {
    // Rounded server-side: the gauge is integer seconds, and doing the cast in
    // SQL avoids a lossy float -> int conversion in Rust.
    //
    // Decoded as Option<i64> on purpose: the expression is NULL on a primary
    // and on a replica that has never replayed, and asking for a bare i64 turns
    // that into a decode error that is indistinguishable from a real failure.
    //
    // The LSN comparison is what makes the number trustworthy. A time-only
    // `NOW() - pg_last_xact_replay_timestamp()` measures the age of the last
    // replayed transaction, not the distance behind the primary, so on an idle
    // primary a perfectly synchronised standby appears to fall further behind
    // with every passing second and pages someone at 3am for nothing. When the
    // received and replayed LSNs agree there is nothing left to apply, and the
    // lag is exactly zero regardless of how long ago that happened.
    //
    // Both LSNs NULL means the standby has never streamed: nothing has been
    // received or replayed, so there is no distance to measure. Report
    // nothing -- as a primary does -- rather than claiming a lag of exactly 0
    // for a replica in an unknown state.
    //
    // Equal LSNs only mean "caught up" while WAL is still arriving. A standby
    // whose primary died has both LSNs equal the moment replay drains, so
    // without the walreceiver check it reports a lag of exactly 0 forever
    // while serving increasingly stale data -- the one moment the metric has
    // to be believed.
    let lag = sqlx::query_scalar::<_, Option<i64>>(REPLICATION_LAG_SQL)
        .fetch_optional(&mut *conn)
        .await
        .ok()
        .flatten()
        .flatten();

    record_replication_lag("postgres", lag);
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
        version: health_info.version.unwrap_or_default(),
        db_host: health_info.db_host,
        uptime_seconds: health_info.uptime_seconds,
        tls_metadata,
        read_only: true,
        read_only_reason: Some(reason.to_string()),
    })
}

async fn monitor_postgres_blocked_sessions(conn: &mut sqlx::PgConnection) {
    // Absent beats stale. Updating only on success left the gauge frozen at the
    // last good reading, so a server that stopped answering this query kept
    // reporting a reassuring 0 -- or, worse, held a spike forever after the
    // contention had cleared.
    match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock' AND state = 'active'",
    )
    .fetch_optional(&mut *conn)
    .await
    {
        Ok(Some(blocked_count)) => {
            BLOCKED_SESSIONS
                .with_label_values(&["postgres"])
                .set(blocked_count);
        }
        _ => {
            let _ = BLOCKED_SESSIONS.remove_label_values(&["postgres"]);
        }
    }
}

/// Tolerate only the concurrent-creation race: two instances running
/// `CREATE TABLE IF NOT EXISTS` at once lose the race in the system catalog,
/// not in the statement. Match the SQLSTATE (`42P07` `duplicate_table`,
/// `23505` from the `pg_class` unique index) and keep the message check as a
/// fallback for PostgreSQL-compatible servers that phrase the race
/// differently.
fn is_ignorable_postgres_create_error(error: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(db_err) = error {
        let race_by_code = db_err
            .as_error()
            .downcast_ref::<PgDatabaseError>()
            .is_some_and(|e| matches!(e.code(), "42P07" | "23505"));
        race_by_code
            || db_err.message().contains("duplicate key")
            || db_err.message().contains("already exists")
    } else {
        false
    }
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
    if let Err(error) = sqlx::query(AssertSqlSafe(create_table_sql))
        .execute(&mut *conn)
        .await
        && !is_ignorable_postgres_create_error(&error)
    {
        return Err(error.into());
    }
    OPERATION_DURATION
        .with_label_values(&["postgres", "create_table"])
        .observe(create_table_timer.elapsed().as_secs_f64());

    let create_index_sql =
        format!("CREATE INDEX IF NOT EXISTS idx_{table_name}_t2 ON {table_name}(t2)");
    sqlx::query(AssertSqlSafe(create_index_sql))
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

    // t2 must move with every write. MySQL gets that from
    // `ON UPDATE CURRENT_TIMESTAMP`; PostgreSQL has no equivalent, so without
    // an explicit update a hot row keeps its original insert timestamp and the
    // hourly cleanup deletes it an hour after it was first created, while it
    // is still being written -- and a concurrent instance's cleanup can delete
    // it between this upsert and the read-back, failing a healthy check.
    let insert_sql = format!(
        r"
        INSERT INTO {table_name} (id, t1, uuid)
        VALUES ($1, $2, $3)
        ON CONFLICT (id)
        DO UPDATE SET t1 = EXCLUDED.t1, t2 = CURRENT_TIMESTAMP, uuid = EXCLUDED.uuid
        "
    );
    let insert_timer = Instant::now();
    let insert_result = sqlx::query(AssertSqlSafe(insert_sql))
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
    let row: Option<(i64, Uuid)> = sqlx::query_as(AssertSqlSafe(select_sql))
        .bind(id_i32)
        .fetch_optional(&mut *conn)
        .await?;
    OPERATION_DURATION
        .with_label_values(&["postgres", "select"])
        .observe(select_timer.elapsed().as_secs_f64());

    // The row just written is gone: another instance's cleanup or hourly
    // drop removed it between the upsert and the read-back (a drop surfaces
    // as SQLSTATE 42P01 and takes the recovery path instead). Only dbpulse
    // deletes from this table, so like any other concurrent interference on
    // the shared table this is counted, not paged on.
    let Some((t1, v4)) = row else {
        RW_ROW_CONTENTION.with_label_values(&["postgres"]).inc();
        return Ok(id);
    };
    match classify_read_back(now.timestamp(), &uuid.to_string(), t1, &v4.to_string()) {
        ReadBack::Match => {}
        ReadBack::ConcurrentOverwrite => {
            RW_ROW_CONTENTION.with_label_values(&["postgres"]).inc();
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

/// Prove the server honours a rollback.
///
/// The upsert refreshes `t2` like the main one does. Nothing here commits, so
/// it changes no behaviour today -- it keeps the two upserts identical so this
/// one cannot quietly reintroduce the stale-`t2` bug (cleanup deleting rows
/// that are being actively written) if it ever stops rolling back.
async fn postgres_transaction_rollback_test(
    conn: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
    table_name: &str,
) -> Result<()> {
    let rollback_seed = now.timestamp_micros().rem_euclid(i64::from(i32::MAX));
    let rollback_test_id =
        i32::try_from(rollback_seed).context("rollback test id out of range for PostgreSQL INT")?;
    let rollback_uuid = Uuid::new_v4();

    let transaction_timer = Instant::now();
    let mut tx = conn.begin().await?;
    let insert_tx_sql = format!(
        "INSERT INTO {table_name} (id, t1, uuid) VALUES ($1, 999, $2) \
         ON CONFLICT (id) DO UPDATE SET t1 = 999, t2 = CURRENT_TIMESTAMP"
    );
    sqlx::query(AssertSqlSafe(insert_tx_sql))
        .bind(rollback_test_id)
        .bind(rollback_uuid)
        .execute(tx.as_mut())
        .await?;

    let update_tx_sql = format!("UPDATE {table_name} SET t1 = $1 WHERE id = $2");
    sqlx::query(AssertSqlSafe(update_tx_sql))
        .bind(0)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    let select_tx_sql = format!("SELECT t1 FROM {table_name} WHERE id = $1");
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

    let select_rollback_sql = format!("SELECT t1 FROM {table_name} WHERE id = $1");
    let rolled_back_value: Option<i64> = sqlx::query_scalar(AssertSqlSafe(select_rollback_sql))
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
    if let Ok(delete_result) = sqlx::query(AssertSqlSafe(delete_old_sql))
        .execute(&mut *conn)
        .await
    {
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
    // Absent beats stale: a dropped table or a failing query must retire the
    // series rather than leave the last count standing as though current.
    let row_count = sqlx::query_scalar::<_, i64>(AssertSqlSafe(row_count_sql))
        .fetch_optional(&mut *conn)
        .await
        .ok()
        .flatten();
    set_or_retire(&TABLE_ROWS, &["postgres", table_name], row_count);
}

async fn maybe_drop_postgres_table_hourly(
    conn: &mut sqlx::PgConnection,
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
                .with_label_values(&["postgres", "count"])
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
            .with_label_values(&["postgres", "drop"])
            .inc();
    }
}

async fn update_postgres_size_metrics(conn: &mut sqlx::PgConnection, table_name: &str) {
    // Absent beats stale for both of these: a size that can no longer be read
    // must stop being reported rather than pinning the last known value, which
    // would make a growing table look permanently stable.
    let size_sql = format!("SELECT pg_total_relation_size('{table_name}')");
    match sqlx::query_scalar::<_, i64>(AssertSqlSafe(size_sql))
        .fetch_optional(&mut *conn)
        .await
    {
        Ok(Some(table_bytes)) => {
            TABLE_SIZE_BYTES
                .with_label_values(&["postgres", table_name])
                .set(table_bytes);
        }
        _ => {
            let _ = TABLE_SIZE_BYTES.remove_label_values(&["postgres", table_name]);
        }
    }

    match sqlx::query_scalar::<_, i64>("SELECT pg_database_size(current_database())")
        .fetch_optional(&mut *conn)
        .await
    {
        Ok(Some(db_size)) => {
            DATABASE_SIZE_BYTES
                .with_label_values(&["postgres"])
                .set(db_size);
        }
        _ => {
            let _ = DATABASE_SIZE_BYTES.remove_label_values(&["postgres"]);
        }
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
                TLS_CERT_PROBE_ERRORS
                    .with_label_values(&["postgres", classify_cert_probe_error(&err)])
                    .inc();
            }
        }
    }

    Ok(metadata)
}
