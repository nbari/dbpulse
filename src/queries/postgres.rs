use anyhow::{Context, Result, anyhow};
use chrono::prelude::*;
use chrono::{DateTime, Utc};
use dsn::DSN;
use rand::Rng;
use sqlx::{
    ConnectOptions, Connection, Row,
    postgres::{PgConnectOptions, PgDatabaseError, PgSslMode},
};
use std::time::Instant;
use uuid::Uuid;

use super::HealthCheckResult;
use crate::metrics::{
    CONNECTION_DURATION, CONNECTIONS_ACTIVE, OPERATION_DURATION, ROWS_AFFECTED, TABLE_ROWS,
    TABLE_SIZE_BYTES, TLS_HANDSHAKE_DURATION,
};
use crate::tls::{TlsConfig, TlsMetadata, TlsMode};

pub async fn test_rw(
    dsn: &DSN,
    now: DateTime<Utc>,
    range: u32,
    tls: &TlsConfig,
) -> Result<HealthCheckResult> {
    test_rw_with_table(dsn, now, range, tls, "dbpulse_rw").await
}

pub async fn test_rw_with_table(
    dsn: &DSN,
    now: DateTime<Utc>,
    range: u32,
    tls: &TlsConfig,
    table_name: &str,
) -> Result<HealthCheckResult> {
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
    CONNECTIONS_ACTIVE.inc();

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
        Err(err) => match err {
            sqlx::Error::Database(db_err) => {
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
                    CONNECTIONS_ACTIVE.dec();
                    return Err(db_err.into());
                }
            }
            _ => {
                CONNECTIONS_ACTIVE.dec();
                return Err(err.into());
            }
        },
    };

    // Get database version
    let version: Option<String> = sqlx::query_scalar("SHOW server_version")
        .fetch_optional(&mut conn)
        .await
        .context("Failed to fetch database version")?;

    // Query to check if the database is in recovery (read-only)
    let is_in_recovery: (bool,) = sqlx::query_as("SELECT pg_is_in_recovery();")
        .fetch_one(&mut conn)
        .await?;

    // can't write to a read-only database
    if is_in_recovery.0 {
        let tls_metadata = if tls.mode.is_enabled() {
            extract_tls_metadata(&mut conn).await.ok()
        } else {
            None
        };
        return Ok(HealthCheckResult {
            version: format!(
                "{} - Database is in recovery mode",
                version.unwrap_or_default()
            ),
            tls_metadata,
        });
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
        r#"
        CREATE TABLE IF NOT EXISTS {} (
            id SERIAL PRIMARY KEY,
            t1 BIGINT NOT NULL,
            t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP,
            uuid UUID NOT NULL,
            CONSTRAINT {}_uuid_unique UNIQUE (uuid)
        )
        "#,
        table_name, table_name
    );

    let create_table_timer = Instant::now();
    if let Err(e) = sqlx::query(&create_table_sql).execute(&mut conn).await {
        // Ignore duplicate table/index/constraint errors from concurrent CREATE TABLE
        if let sqlx::Error::Database(db_err) = &e {
            if !db_err.message().contains("duplicate key")
                && !db_err.message().contains("already exists")
            {
                CONNECTIONS_ACTIVE.dec();
                return Err(e.into());
            }
        } else {
            CONNECTIONS_ACTIVE.dec();
            return Err(e.into());
        }
    }
    OPERATION_DURATION
        .with_label_values(&["postgres", "create_table"])
        .observe(create_table_timer.elapsed().as_secs_f64());

    // Create index on t2 for efficient cleanup (only if doesn't exist)
    let create_index_sql = format!(
        "CREATE INDEX IF NOT EXISTS idx_{}_t2 ON {}(t2)",
        table_name, table_name
    );
    sqlx::query(&create_index_sql).execute(&mut conn).await.ok(); // Ignore errors if index exists

    // write into table
    let id: u32 = rand::rng().random_range(0..range);
    let uuid = Uuid::new_v4();

    // SQL Query
    let insert_sql = format!(
        r#"
        INSERT INTO {} (id, t1, uuid)
        VALUES ($1, $2, $3)
        ON CONFLICT (id)
        DO UPDATE SET t1 = EXCLUDED.t1, uuid = EXCLUDED.uuid
        "#,
        table_name
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
        r#"
        SELECT t1, uuid
        FROM {}
        WHERE id = $1
        "#,
        table_name
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
    let rollback_test_id = (now.timestamp_micros() % 2147483647) as i32;

    let transaction_timer = Instant::now();
    let mut tx = conn.begin().await?;

    // Insert a test record
    let insert_tx_sql = format!(
        "INSERT INTO {} (id, t1, uuid) VALUES ($1, 999, UUID_GENERATE_V4()) ON CONFLICT (id) DO UPDATE SET t1 = 999",
        table_name
    );
    sqlx::query(&insert_tx_sql)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    // Update it within the transaction
    let update_tx_sql = format!("UPDATE {} SET t1 = $1 WHERE id = $2", table_name);
    sqlx::query(&update_tx_sql)
        .bind(0)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    // Verify the update
    let select_tx_sql = format!("SELECT t1 FROM {} WHERE id = $1", table_name);
    let updated_value: Option<i64> = sqlx::query_scalar(&select_tx_sql)
        .bind(rollback_test_id)
        .fetch_optional(tx.as_mut())
        .await?;

    if updated_value != Some(0) {
        return Err(anyhow!(
            "Transaction update failed: expected 0, got {:?}",
            updated_value
        ));
    }

    // Roll back this transaction
    tx.rollback().await?;

    // Verify the rollback worked (value should be 999 or record not exist)
    let select_rollback_sql = format!("SELECT t1 FROM {} WHERE id = $1", table_name);
    let rolled_back_value: Option<i64> = sqlx::query_scalar(&select_rollback_sql)
        .bind(rollback_test_id)
        .fetch_optional(&mut conn)
        .await?;

    if rolled_back_value == Some(0) {
        CONNECTIONS_ACTIVE.dec();
        return Err(anyhow!("Transaction rollback failed: value is still 0"));
    }
    OPERATION_DURATION
        .with_label_values(&["postgres", "transaction_test"])
        .observe(transaction_timer.elapsed().as_secs_f64());

    // Cleanup strategy: Remove old records to prevent unbounded growth
    // Delete records older than 1 hour (keeps table size bounded)
    // Use LIMIT to avoid long-running DELETE operations that could block other queries
    let delete_old_sql = format!(
        "DELETE FROM {} WHERE id IN (SELECT id FROM {} WHERE t2 < NOW() - INTERVAL '1 hour' LIMIT 10000)",
        table_name, table_name
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

    // Periodic full table drop: deterministic cleanup every hour at minute 0
    // This ensures table is recreated fresh periodically
    // Only drop if we're sure it's safe (check table size first)
    if now.minute() == 0 && id < 5 {
        // Check table size before dropping - only drop if it has fewer than 100k rows
        let count_sql = format!("SELECT COUNT(*) FROM {}", table_name);
        if let Ok(Some(row_count)) = sqlx::query_scalar::<_, i64>(&count_sql)
            .fetch_optional(&mut conn)
            .await
        {
            // Record table row count
            TABLE_ROWS
                .with_label_values(&["postgres", table_name])
                .set(row_count);

            // Only drop if table is relatively small to avoid disrupting active monitoring
            if row_count < 100000 {
                let drop_table_sql = format!("DROP TABLE IF EXISTS {}", table_name);
                sqlx::query(&drop_table_sql).execute(&mut conn).await.ok();
            }
        }
    }

    // Query table size in bytes (optional, but useful for monitoring)
    let size_sql = format!("SELECT pg_total_relation_size('{}')", table_name);
    if let Ok(Some(table_bytes)) = sqlx::query_scalar::<_, i64>(&size_sql)
        .fetch_optional(&mut conn)
        .await
    {
        TABLE_SIZE_BYTES
            .with_label_values(&["postgres", table_name])
            .set(table_bytes);
    }

    // Extract TLS metadata if TLS is enabled
    let tls_metadata = if tls.mode.is_enabled() {
        extract_tls_metadata(&mut conn).await.ok()
    } else {
        None
    };

    // Record connection lifecycle metrics
    drop(conn);
    CONNECTION_DURATION.observe(conn_start.elapsed().as_secs_f64());
    CONNECTIONS_ACTIVE.dec();

    Ok(HealthCheckResult {
        version: version.context("Expected database version")?,
        tls_metadata,
    })
}

/// Extract TLS metadata from PostgreSQL connection
async fn extract_tls_metadata(conn: &mut sqlx::PgConnection) -> Result<TlsMetadata> {
    // Query pg_stat_ssl for TLS information
    let row = sqlx::query("SELECT version, cipher FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
        .fetch_optional(conn)
        .await?;

    row.map_or_else(
        || Ok(TlsMetadata::default()),
        |row| {
            let version: Option<String> = row.try_get(0).ok();
            let cipher: Option<String> = row.try_get(1).ok();

            Ok(TlsMetadata {
                version,
                cipher,
                ..Default::default()
            })
        },
    )
}
