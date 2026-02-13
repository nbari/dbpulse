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
    ConnectOptions, Connection, Executor, Row,
    mysql::{MySqlConnectOptions, MySqlDatabaseError, MySqlSslMode},
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
#[allow(clippy::too_many_lines)]
pub async fn test_rw_with_table(
    dsn: &DSN,
    now: DateTime<Utc>,
    range: u32,
    tls: &TlsConfig,
    cert_cache: &CertCache,
    table_name: &str,
) -> Result<HealthCheckResult> {
    ensure_crypto_provider();
    let mut options = MySqlConnectOptions::new()
        .username(dsn.username.clone().unwrap_or_default().as_ref())
        .password(dsn.password.clone().unwrap_or_default().as_str())
        .database(dsn.database.clone().unwrap_or_default().as_ref());

    if let Some(host) = &dsn.host {
        options = options.host(host.as_str()).port(dsn.port.unwrap_or(3306));
    } else if let Some(socket) = &dsn.socket {
        options = options.socket(socket.as_str());
    }

    // Apply TLS configuration
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
                .with_label_values(&["mysql", "connect"])
                .observe(connect_duration);
            // Record TLS handshake duration if TLS is enabled
            if tls.mode.is_enabled() {
                TLS_HANDSHAKE_DURATION
                    .with_label_values(&["mysql"])
                    .observe(connect_duration);
            }
            conn
        }
        Err(err) => {
            if let sqlx::Error::Database(db_err) = err {
                if db_err
                    .as_error()
                    .downcast_ref::<MySqlDatabaseError>()
                    .map(MySqlDatabaseError::number)
                    == Some(1049)
                {
                    let tmp_options = options.clone().database("mysql");
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
                        .with_label_values(&["mysql", "connect"])
                        .observe(connect_duration);
                    // Record TLS handshake duration if TLS is enabled
                    if tls.mode.is_enabled() {
                        TLS_HANDSHAKE_DURATION
                            .with_label_values(&["mysql"])
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
    // Try MySQL variable first (max_execution_time in milliseconds)
    // If that fails, try MariaDB variable (max_statement_time in seconds)
    if sqlx::query("SET SESSION max_execution_time = 5000")
        .execute(&mut conn)
        .await
        .is_err()
    {
        // MariaDB uses max_statement_time in seconds instead
        let _ = sqlx::query("SET SESSION max_statement_time = 5")
            .execute(&mut conn)
            .await;
    }

    // Set lock wait timeout (common to both MySQL and MariaDB)
    sqlx::query("SET SESSION innodb_lock_wait_timeout = 2")
        .execute(&mut conn)
        .await
        .context("Failed to set innodb_lock_wait_timeout")?;

    // Get database version
    let version: Option<String> = sqlx::query_scalar("SELECT VERSION()")
        .fetch_optional(&mut conn)
        .await
        .context("Failed to fetch database version")?;

    // Get backend host serving this connection
    let db_host: Option<String> = sqlx::query_scalar("SELECT @@hostname")
        .fetch_optional(&mut conn)
        .await
        .ok()
        .flatten();

    // Get database uptime (SHOW GLOBAL STATUS LIKE 'Uptime')
    let uptime_seconds = sqlx::query("SHOW GLOBAL STATUS LIKE 'Uptime'")
        .fetch_optional(&mut conn)
        .await
        .ok()
        .flatten()
        .and_then(|row| {
            row.try_get::<String, _>("Value")
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
        });

    // check if db is in read-only mode
    // Use raw Row to handle both MariaDB (returns integer) and MySQL (may return string/integer)
    let row = sqlx::query("SELECT @@read_only;")
        .fetch_one(&mut conn)
        .await
        .context("Failed to check if the database is in read-only mode")?;

    // Try to get as i64 first (MariaDB), fallback to string
    let is_read_only = row.try_get::<i64, _>(0).map_or_else(
        |_| {
            row.try_get::<String, _>(0)
                .is_ok_and(|val| val.to_uppercase() == "ON" || val == "1")
        },
        |val| val != 0,
    );

    // Monitor replication lag if this is a replica (read-only)
    if is_read_only {
        // Try to get replication lag from SHOW REPLICA STATUS (MySQL/MariaDB)
        if let Ok(Some(row)) = sqlx::query("SHOW REPLICA STATUS")
            .fetch_optional(&mut conn)
            .await
        {
            // Seconds_Behind_Source column (replication lag in seconds)
            // -1 means not connected, only record if connected
            if let Ok(lag_seconds) = row.try_get::<i64, _>("Seconds_Behind_Source")
                && lag_seconds >= 0
            {
                // Replication lag in seconds won't exceed f64 precision in practice
                #[allow(clippy::cast_precision_loss)]
                REPLICATION_LAG
                    .with_label_values(&["mysql"])
                    .observe(lag_seconds as f64);
            }
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
                "{} - Database is in read-only mode",
                version.unwrap_or_default()
            ),
            db_host: db_host.clone(),
            uptime_seconds,
            tls_metadata,
        });
    }

    // Monitor blocking queries (information_schema.innodb_lock_waits)
    if let Ok(Some(blocking_count)) = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM information_schema.processlist WHERE state LIKE '%lock%' OR state LIKE '%Locked%'",
    )
    .fetch_optional(&mut conn)
    .await
    {
        BLOCKING_QUERIES
            .with_label_values(&["mysql"])
            .set(blocking_count);
    }

    // create table with optimized schema
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
    conn.execute(create_table_sql.as_str()).await?;
    OPERATION_DURATION
        .with_label_values(&["mysql", "create_table"])
        .observe(create_table_timer.elapsed().as_secs_f64());

    // write into table
    let id: u32 = rand::rng().random_range(0..range);
    let uuid = Uuid::new_v4();

    // SQL Query
    let insert_sql = format!(
        r"
        INSERT INTO {table_name} (id, t1, uuid)
        VALUES (?, ?, ?)
        ON DUPLICATE KEY UPDATE
        t1 = VALUES(t1), uuid = VALUES(uuid)
        "
    );
    let insert_timer = Instant::now();
    let insert_result = sqlx::query(&insert_sql)
        .bind(id)
        .bind(now.timestamp())
        .bind(uuid.to_string())
        .execute(&mut conn)
        .await?;
    OPERATION_DURATION
        .with_label_values(&["mysql", "insert"])
        .observe(insert_timer.elapsed().as_secs_f64());
    ROWS_AFFECTED
        .with_label_values(&["mysql", "insert"])
        .inc_by(insert_result.rows_affected());

    // Check if stored record matches
    let select_sql = format!(
        r"
        SELECT t1, uuid
        FROM {table_name}
        WHERE id = ?
        "
    );
    let select_timer = Instant::now();
    let row: Option<(i64, String)> = sqlx::query_as(&select_sql)
        .bind(id)
        .fetch_optional(&mut conn)
        .await
        .context("Failed to query the database")?;
    OPERATION_DURATION
        .with_label_values(&["mysql", "select"])
        .observe(select_timer.elapsed().as_secs_f64());

    // Ensure the row exists and matches
    let (t1, v4) = row.context("Expected records")?;
    if now.timestamp() != t1 || uuid.to_string() != v4 {
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
        "INSERT INTO {table_name} (id, t1, uuid) VALUES (?, 999, UUID()) ON DUPLICATE KEY UPDATE t1 = 999"
    );
    sqlx::query(&insert_tx_sql)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    // Update it within the transaction
    let update_tx_sql = format!("UPDATE {table_name} SET t1 = ? WHERE id = ?");
    sqlx::query(&update_tx_sql)
        .bind(0)
        .bind(rollback_test_id)
        .execute(tx.as_mut())
        .await?;

    // Verify the update
    let select_tx_sql = format!("SELECT t1 FROM {table_name} WHERE id = ?");
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
    let select_rollback_sql = format!("SELECT t1 FROM {table_name} WHERE id = ?");
    let rolled_back_value: Option<i64> = sqlx::query_scalar(&select_rollback_sql)
        .bind(rollback_test_id)
        .fetch_optional(&mut conn)
        .await?;

    if rolled_back_value == Some(0) {
        return Err(anyhow!("Transaction rollback failed: value is still 0"));
    }
    OPERATION_DURATION
        .with_label_values(&["mysql", "transaction_test"])
        .observe(transaction_timer.elapsed().as_secs_f64());

    // Cleanup strategy: Remove old records to prevent unbounded growth
    // Delete records older than 1 hour (keeps table size bounded)
    // Use LIMIT to avoid long-running DELETE operations that could block other queries
    let one_hour_ago = (now - chrono::Duration::hours(1)).to_rfc3339();
    let delete_old_sql = format!("DELETE FROM {table_name} WHERE t2 < ? LIMIT 10000");
    let cleanup_timer = Instant::now();
    if let Ok(delete_result) = sqlx::query(&delete_old_sql)
        .bind(one_hour_ago)
        .execute(&mut conn)
        .await
    {
        ROWS_AFFECTED
            .with_label_values(&["mysql", "delete"])
            .inc_by(delete_result.rows_affected());
    }
    OPERATION_DURATION
        .with_label_values(&["mysql", "cleanup"])
        .observe(cleanup_timer.elapsed().as_secs_f64());

    // Query approximate table row count (faster than COUNT(*) for large tables)
    // Note: MariaDB returns BIGINT UNSIGNED, MySQL returns BIGINT
    let row_count_sql = format!(
        "SELECT CAST(table_rows AS SIGNED) FROM information_schema.TABLES WHERE table_schema = DATABASE() AND table_name = '{table_name}'"
    );
    match sqlx::query_scalar::<_, Option<i64>>(&row_count_sql)
        .fetch_optional(&mut conn)
        .await
    {
        Ok(Some(Some(row_count))) => {
            TABLE_ROWS
                .with_label_values(&["mysql", table_name])
                .set(row_count);
        }
        Ok(Some(None) | None) => {
            // NULL or not found - fall back to COUNT(*) for accurate count
            // This happens when table stats aren't initialized or table is very new
            let count_sql = format!("SELECT COUNT(*) FROM {table_name}");
            if let Ok(Some(exact_count)) = sqlx::query_scalar::<_, i64>(&count_sql)
                .fetch_optional(&mut conn)
                .await
            {
                TABLE_ROWS
                    .with_label_values(&["mysql", table_name])
                    .set(exact_count);
            }
        }
        Err(e) => {
            eprintln!("Error querying table_rows for '{table_name}': {e}");
        }
    }

    // Periodic full table drop: probabilistic cleanup at minute 0 of each hour
    // Only drops when id < 5 (5/range probability) to avoid all instances dropping simultaneously
    // This ensures table is recreated fresh periodically without coordination between instances
    if now.minute() == 0 && id < 5 {
        // Use the row count we just queried (or query again if needed)
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
    // Note: MariaDB returns DECIMAL, MySQL returns BIGINT - cast to ensure compatibility
    let size_sql = format!(
        "SELECT CAST(COALESCE(data_length, 0) + COALESCE(index_length, 0) AS SIGNED) FROM information_schema.TABLES WHERE table_schema = DATABASE() AND table_name = '{table_name}'"
    );
    match sqlx::query_scalar::<_, i64>(&size_sql)
        .fetch_optional(&mut conn)
        .await
    {
        Ok(Some(table_bytes)) => {
            TABLE_SIZE_BYTES
                .with_label_values(&["mysql", table_name])
                .set(table_bytes);
        }
        Ok(None) => {
            // Table not found - set to 0 so metric appears
            TABLE_SIZE_BYTES
                .with_label_values(&["mysql", table_name])
                .set(0);
        }
        Err(e) => {
            eprintln!("Error querying table size for '{table_name}': {e}");
        }
    }

    // Query total database size in bytes
    // Note: MariaDB returns DECIMAL - cast to SIGNED for compatibility
    if let Ok(Some(db_size)) = sqlx::query_scalar::<_, i64>(
        "SELECT CAST(SUM(COALESCE(data_length, 0) + COALESCE(index_length, 0)) AS SIGNED) FROM information_schema.TABLES WHERE table_schema = DATABASE()",
    )
    .fetch_optional(&mut conn)
    .await
    {
        DATABASE_SIZE_BYTES
            .with_label_values(&["mysql"])
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
        db_host,
        uptime_seconds,
        tls_metadata,
    })
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
                    .with_label_values(&["mysql", error_type])
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
            "Ssl_version" => {
                if !value.is_empty() {
                    tls_version = Some(value);
                }
            }
            "Ssl_cipher" => {
                if !value.is_empty() {
                    tls_cipher = Some(value);
                }
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
            return Some((expiry - Utc::now()).num_days());
        }
    }

    None
}

#[cfg(test)]
mod tests {
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
}
