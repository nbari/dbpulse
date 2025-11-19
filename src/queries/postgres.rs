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
#[allow(clippy::too_many_lines, clippy::cast_possible_wrap)]
pub async fn test_rw_with_table(
    dsn: &DSN,
    now: DateTime<Utc>,
    range: u32,
    tls: &TlsConfig,
    cert_cache: &CertCache,
    table_name: &str,
) -> Result<HealthCheckResult> {
    ensure_crypto_provider();
    let mut options = PgConnectOptions::new()
        .username(dsn.username.clone().unwrap_or_default().as_ref())
        .password(dsn.password.clone().unwrap_or_default().as_str())
        .database(dsn.database.clone().unwrap_or_default().as_ref());

    if let Some(host) = &dsn.host {
        options = options.host(host.as_str()).port(dsn.port.unwrap_or(5432));
    } else if let Some(socket) = &dsn.socket {
        options = options.socket(socket.as_str());
    }

    // Apply TLS configuration
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

    // Apply client certificate if provided
    if let (Some(cert_path), Some(key_path)) = (&tls.cert, &tls.key) {
        options = options.ssl_client_cert(cert_path).ssl_client_key(key_path);
    }

    // Track connection establishment
    let conn_start = Instant::now();

    let connect_timer = Instant::now();
    let mut conn = match options.connect().await {
        Ok(conn) => {
            let connect_duration = connect_timer.elapsed().as_secs_f64();
            OPERATION_DURATION
                .with_label_values(&["postgres", "connect"])
                .observe(connect_duration);
            // Record TLS handshake duration if TLS is enabled
            if tls.mode.is_enabled() {
                TLS_HANDSHAKE_DURATION
                    .with_label_values(&["postgres"])
                    .observe(connect_duration);
            }
            conn
        }
        Err(err) => {
            if let sqlx::Error::Database(db_err) = err {
                if db_err
                    .as_error()
                    .downcast_ref::<PgDatabaseError>()
                    .map(PgDatabaseError::code)
                    == Some("3D000")
                {
                    let tmp_options = options.clone().database("postgres");
                    let mut tmp_conn = tmp_options.connect().await?;
                    sqlx::query(&format!(
                        "CREATE DATABASE {}",
                        dsn.database.clone().unwrap_or_default()
                    ))
                    .execute(&mut tmp_conn)
                    .await?;
                    drop(tmp_conn);
                    let conn = options.connect().await?;
                    let connect_duration = connect_timer.elapsed().as_secs_f64();
                    OPERATION_DURATION
                        .with_label_values(&["postgres", "connect"])
                        .observe(connect_duration);
                    // Record TLS handshake duration if TLS is enabled
                    if tls.mode.is_enabled() {
                        TLS_HANDSHAKE_DURATION
                            .with_label_values(&["postgres"])
                            .observe(connect_duration);
                    }
                    conn
                } else {
                    return Err(db_err.into());
                }
            } else {
                return Err(err.into());
            }
        }
    };

    // Set query timeouts to prevent hanging on locked tables
    sqlx::query("SET statement_timeout = '5s'")
        .execute(&mut conn)
        .await
        .context("Failed to set statement timeout")?;
    sqlx::query("SET lock_timeout = '2s'")
        .execute(&mut conn)
        .await
        .context("Failed to set lock timeout")?;

    // Get database version
    let version: Option<String> = sqlx::query_scalar("SHOW server_version")
        .fetch_optional(&mut conn)
        .await
        .context("Failed to fetch database version")?;

    // Get database uptime (seconds since postmaster start)
    let uptime_seconds = sqlx::query_scalar::<_, i64>(
        "SELECT EXTRACT(EPOCH FROM NOW() - pg_postmaster_start_time())::bigint",
    )
    .fetch_optional(&mut conn)
    .await
    .ok()
    .flatten();

    // Query to check if the database is in recovery (read-only)
    let is_in_recovery: (bool,) = sqlx::query_as("SELECT pg_is_in_recovery();")
        .fetch_one(&mut conn)
        .await?;

    // Also check transaction read-only status
    let tx_read_only: (String,) = sqlx::query_as("SHOW transaction_read_only;")
        .fetch_one(&mut conn)
        .await?;

    // Monitor replication lag if this is a replica
    if is_in_recovery.0 {
        // Try to get replication lag (in seconds, won't exceed f64 precision)
        if let Ok(Some(lag_seconds)) = sqlx::query_scalar::<_, f64>(
            "SELECT EXTRACT(EPOCH FROM (NOW() - pg_last_xact_replay_timestamp()))",
        )
        .fetch_optional(&mut conn)
        .await
        {
            REPLICATION_LAG
                .with_label_values(&["postgres"])
                .observe(lag_seconds);
        }

        let tls_metadata = if tls.mode.is_enabled() {
            extract_tls_metadata(dsn, tls, &mut conn, cert_cache)
                .await
                .ok()
        } else {
            None
        };
        return Ok(HealthCheckResult {
            version: format!(
                "{} - Database is in recovery mode",
                version.unwrap_or_default()
            ),
            uptime_seconds,
            tls_metadata,
        });
    }

    // Check if transaction is read-only (even if not in recovery)
    if tx_read_only.0.to_lowercase() == "on" {
        let tls_metadata = if tls.mode.is_enabled() {
            extract_tls_metadata(dsn, tls, &mut conn, cert_cache)
                .await
                .ok()
        } else {
            None
        };
        return Ok(HealthCheckResult {
            version: format!(
                "{} - Transaction read-only mode enabled",
                version.unwrap_or_default()
            ),
            uptime_seconds,
            tls_metadata,
        });
    }

    // Monitor blocking queries
    if let Ok(Some(blocking_count)) = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock' AND state = 'active'",
    )
    .fetch_optional(&mut conn)
    .await
    {
        BLOCKING_QUERIES
            .with_label_values(&["postgres"])
            .set(blocking_count);
    }

    // for UUID - ignore duplicate key error if extension already exists (race condition)
    if let Err(e) = sqlx::query("CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\"")
        .execute(&mut conn)
        .await
    {
        // Ignore duplicate extension errors (42710 or duplicate key constraint)
        if let sqlx::Error::Database(db_err) = &e {
            let code = db_err
                .as_error()
                .downcast_ref::<PgDatabaseError>()
                .map(PgDatabaseError::code);
            // 42710 = extension already exists
            // Also ignore constraint violations from concurrent CREATE EXTENSION
            if code != Some("42710") && !db_err.message().contains("duplicate key") {
                return Err(e.into());
            }
        } else {
            return Err(e.into());
        }
    }

    // create table with optimized schema - ignore duplicate errors from concurrent creation
    let create_table_sql = format!(
        r"
        CREATE TABLE IF NOT EXISTS {table_name} (
            id SERIAL PRIMARY KEY,
            t1 BIGINT NOT NULL,
            t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP,
            uuid UUID NOT NULL,
            CONSTRAINT {table_name}_uuid_unique UNIQUE (uuid)
        )
        "
    );

    let create_table_timer = Instant::now();
    if let Err(e) = sqlx::query(&create_table_sql).execute(&mut conn).await {
        // Ignore duplicate table/index/constraint errors from concurrent CREATE TABLE
        if let sqlx::Error::Database(db_err) = &e {
            if !db_err.message().contains("duplicate key")
                && !db_err.message().contains("already exists")
            {
                return Err(e.into());
            }
        } else {
            return Err(e.into());
        }
    }
    OPERATION_DURATION
        .with_label_values(&["postgres", "create_table"])
        .observe(create_table_timer.elapsed().as_secs_f64());

    // Create index on t2 for efficient cleanup (only if doesn't exist)
    let create_index_sql =
        format!("CREATE INDEX IF NOT EXISTS idx_{table_name}_t2 ON {table_name}(t2)");
    sqlx::query(&create_index_sql).execute(&mut conn).await.ok(); // Ignore errors if index exists

    // write into table
    let id: u32 = rand::rng().random_range(0..range);
    let uuid = Uuid::new_v4();

    // SQL Query
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
        .bind(id as i32)
        .bind(now.timestamp())
        .bind(uuid)
        .execute(&mut conn) // Ensure we're using PgConnection here
        .await?;
    OPERATION_DURATION
        .with_label_values(&["postgres", "insert"])
        .observe(insert_timer.elapsed().as_secs_f64());
    ROWS_AFFECTED
        .with_label_values(&["postgres", "insert"])
        .inc_by(insert_result.rows_affected());

    // Check if stored record matches
    let select_sql = format!(
        r"
        SELECT t1, uuid
        FROM {table_name}
        WHERE id = $1
        "
    );
    let select_timer = Instant::now();
    let row: Option<(i64, Uuid)> = sqlx::query_as(&select_sql)
        .bind(id as i32)
        .fetch_optional(&mut conn)
        .await?;
    OPERATION_DURATION
        .with_label_values(&["postgres", "select"])
        .observe(select_timer.elapsed().as_secs_f64());

    // Ensure the row exists and matches
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

    // Test transaction rollback with a unique ID to avoid conflicts with parallel tests
    // Use timestamp-based ID that won't conflict with normal operations
    let rollback_test_id = (now.timestamp_micros() % 2_147_483_647) as i32;

    let transaction_timer = Instant::now();
    let mut tx = conn.begin().await?;

    // Insert a test record
    let insert_tx_sql = format!(
        "INSERT INTO {table_name} (id, t1, uuid) VALUES ($1, 999, UUID_GENERATE_V4()) ON CONFLICT (id) DO UPDATE SET t1 = 999"
    );
    sqlx::query(&insert_tx_sql)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    // Update it within the transaction
    let update_tx_sql = format!("UPDATE {table_name} SET t1 = $1 WHERE id = $2");
    sqlx::query(&update_tx_sql)
        .bind(0)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    // Verify the update
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

    // Roll back this transaction
    tx.rollback().await?;

    // Verify the rollback worked (value should be 999 or record not exist)
    let select_rollback_sql = format!("SELECT t1 FROM {table_name} WHERE id = $1");
    let rolled_back_value: Option<i64> = sqlx::query_scalar(&select_rollback_sql)
        .bind(rollback_test_id)
        .fetch_optional(&mut conn)
        .await?;

    if rolled_back_value == Some(0) {
        return Err(anyhow!("Transaction rollback failed: value is still 0"));
    }
    OPERATION_DURATION
        .with_label_values(&["postgres", "transaction_test"])
        .observe(transaction_timer.elapsed().as_secs_f64());

    // Cleanup strategy: Remove old records to prevent unbounded growth
    // Delete records older than 1 hour (keeps table size bounded)
    // Use LIMIT to avoid long-running DELETE operations that could block other queries
    let delete_old_sql = format!(
        "DELETE FROM {table_name} WHERE id IN (SELECT id FROM {table_name} WHERE t2 < NOW() - INTERVAL '1 hour' LIMIT 10000)"
    );
    let cleanup_timer = Instant::now();
    if let Ok(delete_result) = sqlx::query(&delete_old_sql).execute(&mut conn).await {
        ROWS_AFFECTED
            .with_label_values(&["postgres", "delete"])
            .inc_by(delete_result.rows_affected());
    }
    OPERATION_DURATION
        .with_label_values(&["postgres", "cleanup"])
        .observe(cleanup_timer.elapsed().as_secs_f64());

    // Query approximate table row count (faster than COUNT(*) for large tables)
    // Use pg_class.reltuples for quick estimate
    let row_count_sql =
        format!("SELECT reltuples::bigint FROM pg_class WHERE relname = '{table_name}'");
    if let Ok(Some(row_count)) = sqlx::query_scalar::<_, i64>(&row_count_sql)
        .fetch_optional(&mut conn)
        .await
    {
        TABLE_ROWS
            .with_label_values(&["postgres", table_name])
            .set(row_count);
    }

    // Periodic full table drop: probabilistic cleanup at minute 0 of each hour
    // Only drops when id < 5 (5/range probability) to avoid all instances dropping simultaneously
    // This ensures table is recreated fresh periodically without coordination between instances
    if now.minute() == 0 && id < 5 {
        // Use exact count for drop decision
        let count_sql = format!("SELECT COUNT(*) FROM {table_name}");
        if let Ok(Some(exact_count)) = sqlx::query_scalar::<_, i64>(&count_sql)
            .fetch_optional(&mut conn)
            .await
        {
            // Only drop if table is relatively small to avoid disrupting active monitoring
            if exact_count < 100_000 {
                let drop_table_sql = format!("DROP TABLE IF EXISTS {table_name}");
                sqlx::query(&drop_table_sql).execute(&mut conn).await.ok();
            }
        }
    }

    // Query table size in bytes (optional, but useful for monitoring)
    let size_sql = format!("SELECT pg_total_relation_size('{table_name}')");
    if let Ok(Some(table_bytes)) = sqlx::query_scalar::<_, i64>(&size_sql)
        .fetch_optional(&mut conn)
        .await
    {
        TABLE_SIZE_BYTES
            .with_label_values(&["postgres", table_name])
            .set(table_bytes);
    }

    // Query total database size in bytes
    if let Ok(Some(db_size)) =
        sqlx::query_scalar::<_, i64>("SELECT pg_database_size(current_database())")
            .fetch_optional(&mut conn)
            .await
    {
        DATABASE_SIZE_BYTES
            .with_label_values(&["postgres"])
            .set(db_size);
    }

    // Extract TLS metadata if TLS is enabled
    let tls_metadata = if tls.mode.is_enabled() {
        extract_tls_metadata(dsn, tls, &mut conn, cert_cache)
            .await
            .ok()
    } else {
        None
    };

    // Gracefully close connection to avoid "Connection reset by peer" errors in server logs
    let _ = conn.close().await;
    CONNECTION_DURATION.observe(conn_start.elapsed().as_secs_f64());

    Ok(HealthCheckResult {
        version: version.context("Expected database version")?,
        uptime_seconds,
        tls_metadata,
    })
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
