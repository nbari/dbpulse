mod common;

use chrono::Utc;
use common::*;
use dbpulse::queries::mysql;
use dbpulse::tls::cache::CertCache;
use dbpulse::tls::{TlsConfig, TlsMode};
use std::process::{Child, Command, Stdio};
use tokio::time::Duration;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_basic_connection() {
    if skip_if_no_mariadb() {
        return;
    }

    let table_name = test_table_name("test_mariadb_basic_connection");
    let result = test_mariadb_connection_with_table(MARIADB_DSN, &table_name).await;
    assert!(result.is_ok(), "Failed to connect to MariaDB: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);
    assert!(
        health.version.contains("MariaDB"),
        "Should contain MariaDB in version"
    );
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_read_write_operations() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn = parse_dsn(MARIADB_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    // Run test multiple times to ensure cleanup works
    for i in 0..5 {
        let table_name = test_table_name(&format!("test_mariadb_read_write_operations_{i}"));
        let result =
            mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
        let health = result.unwrap_or_else(|e| panic!("Iteration {i}: {e:?}"));
        assert_version_and_uptime("MariaDB", &health);
    }
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_transaction_rollback() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn = parse_dsn(MARIADB_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let table_name = test_table_name("test_mariadb_transaction_rollback");
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    // This tests that transaction rollback works correctly
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
    let health = result.unwrap_or_else(|e| panic!("Transaction test failed: {e:?}"));
    assert_version_and_uptime("MariaDB", &health);
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_concurrent_connections() {
    if skip_if_no_mariadb() {
        return;
    }

    // Spawn multiple concurrent health checks with unique table names
    // Each task gets its own table, eliminating all collision possibilities
    let mut handles = vec![];
    for i in 0..10 {
        let table_name = test_table_name(&format!("test_mariadb_concurrent_connections_{i}"));
        let handle = tokio::spawn(async move {
            let dsn = parse_dsn(MARIADB_DSN);
            let tls = TlsConfig::default();
            let now = Utc::now();
            let cert_cache = CertCache::new(std::time::Duration::from_secs(300));
            mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        match result {
            Ok(health) => assert_version_and_uptime("MariaDB", &health),
            Err(e) => panic!("Concurrent test failed: {e:?}"),
        }
    }
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_with_different_ranges() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn = parse_dsn(MARIADB_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    // Test different range values
    for range in [10, 50, 100, 500, 1000] {
        let table_name = test_table_name(&format!("test_mariadb_with_different_ranges_{range}"));
        let result =
            mysql::test_rw_with_table(&dsn, now, range, &tls, &cert_cache, &table_name).await;
        let health = result.unwrap_or_else(|e| panic!("Range {range} failed: {e:?}"));
        assert_version_and_uptime("MariaDB", &health);
    }
}

#[tokio::test]
#[ignore = "requires running MariaDB container with TLS"]
async fn test_mariadb_tls_disable() {
    if skip_if_no_mariadb() {
        return;
    }

    let result = test_mariadb_with_tls(MARIADB_DSN, TlsMode::Disable).await;
    assert!(result.is_ok(), "TLS Disable failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);
    assert!(
        health.tls_metadata.is_none(),
        "TLS metadata should be None when disabled"
    );
}

#[tokio::test]
#[ignore = "requires running MariaDB container with TLS enabled"]
async fn test_mariadb_tls_require() {
    if skip_if_no_mariadb() {
        return;
    }

    let result = test_mariadb_with_tls(MARIADB_DSN, TlsMode::Require).await;

    // This may fail if MariaDB doesn't have TLS configured
    // That's expected in local test environments
    match result {
        Ok(health) => {
            assert_version_and_uptime("MariaDB", &health);
            println!("TLS connection successful");
            if let Some(ref tls_meta) = health.tls_metadata {
                println!("TLS Version: {:?}", tls_meta.version);
                println!("TLS Cipher: {:?}", tls_meta.cipher);
                assert!(
                    tls_meta.version.is_some() || tls_meta.cipher.is_some(),
                    "Should have TLS metadata when TLS is enabled"
                );
            }
        }
        Err(e) => {
            // Expected if MariaDB doesn't have TLS configured
            println!("TLS test skipped (no TLS configured): {e}");
        }
    }
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_database_creation() {
    if skip_if_no_mariadb() {
        return;
    }

    // Test with a non-existent database (should be auto-created)
    // Use root user since dbpulse user doesn't have CREATE DATABASE privilege
    let dsn_str = "mysql://root:secret@tcp(localhost:3306)/dbpulse_test_db";
    let table_name = test_table_name("test_mariadb_database_creation");
    let result = test_mariadb_connection_with_table(dsn_str, &table_name).await;

    // Should succeed by creating the database
    let health = result.unwrap_or_else(|e| panic!("Database auto-creation failed: {e:?}"));
    assert_version_and_uptime("MariaDB", &health);
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_invalid_credentials() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn_str = "mysql://invalid:invalid@tcp(localhost:3306)/testdb";
    let result = test_mariadb_connection(dsn_str).await;

    // Should fail with authentication error
    assert!(result.is_err(), "Should fail with invalid credentials");
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_version_info() {
    if skip_if_no_mariadb() {
        return;
    }

    let table_name = test_table_name("test_mariadb_version_info");
    let result = test_mariadb_connection_with_table(MARIADB_DSN, &table_name).await;
    assert!(result.is_ok());

    let health = result.unwrap();
    println!("MariaDB version: {}", health.version);

    // Version should contain MariaDB and version number
    assert!(health.version.contains("MariaDB"));
    assert!(
        health.version.chars().any(|c| c.is_ascii_digit()),
        "Version should contain version number"
    );
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_read_only_detection() {
    if skip_if_no_mariadb() {
        return;
    }

    // Normal connection should not be in read-only mode
    let table_name = test_table_name("test_mariadb_read_only_detection");
    let result = test_mariadb_connection_with_table(MARIADB_DSN, &table_name).await;
    assert!(result.is_ok());

    let health = result.unwrap();
    assert!(
        !health.version.contains("read-only mode"),
        "Database should not be in read-only mode"
    );
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_reports_backend_host() {
    if skip_if_no_mariadb() {
        return;
    }

    let table_name = test_table_name("test_mariadb_reports_backend_host");
    let result = test_mariadb_connection_with_table(MARIADB_DSN, &table_name).await;
    assert!(result.is_ok());

    let health = result.unwrap();
    let host = health.db_host.unwrap_or_default();
    assert!(
        !host.trim().is_empty(),
        "Expected non-empty MariaDB backend host"
    );
}

#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_metrics_collection() {
    if skip_if_no_mariadb() {
        return;
    }

    let table_name = test_table_name("test_mariadb_metrics_collection");
    let result = test_mariadb_connection_with_table(MARIADB_DSN, &table_name).await;
    assert!(result.is_ok(), "Connection should succeed");

    // Encode metrics
    let metric_families = dbpulse::metrics::REGISTRY.gather();
    let mut buffer = Vec::new();
    let encoder = prometheus::TextEncoder::new();
    prometheus::Encoder::encode(&encoder, &metric_families, &mut buffer)
        .expect("Failed to encode metrics");
    let metrics_output = String::from_utf8(buffer).expect("Metrics should be valid UTF-8");

    // Verify critical metrics are present (metrics populated by test_rw function)
    assert!(
        metrics_output.contains("dbpulse_operation_duration_seconds"),
        "dbpulse_operation_duration_seconds metric should be present"
    );
    assert!(
        metrics_output.contains("dbpulse_rows_affected_total"),
        "dbpulse_rows_affected_total metric should be present"
    );
    assert!(
        metrics_output.contains("dbpulse_connection_duration_seconds"),
        "dbpulse_connection_duration_seconds metric should be present"
    );

    // Verify MariaDB/MySQL-specific metrics
    assert!(
        metrics_output.contains("database=\"mysql\""),
        "Metrics should be labeled with database='mysql'"
    );
    assert!(
        metrics_output.contains("operation=\"connect\"")
            || metrics_output.contains("operation=\\\"connect\\\""),
        "Should have connect operation metrics"
    );
    assert!(
        metrics_output.contains("operation=\"insert\"")
            || metrics_output.contains("operation=\\\"insert\\\""),
        "Should have insert operation metrics"
    );
    assert!(
        metrics_output.contains("operation=\"select\"")
            || metrics_output.contains("operation=\\\"select\\\""),
        "Should have select operation metrics"
    );

    // Verify database size metric (should be present after connection)
    if metrics_output.contains("dbpulse_database_size_bytes") {
        println!("✓ Database size metrics are being collected");
    }

    // Verify table metrics if available (may not be present in all test runs)
    if metrics_output.contains("dbpulse_table_size_bytes") {
        println!("✓ Table size metrics are being collected");
    }

    println!("Metrics verification complete for MariaDB");
}

#[tokio::test]
#[ignore = "requires running dbpulse-mariadb container and podman/docker access"]
async fn test_mariadb_pulse_transition_stop_start() {
    if skip_if_no_mariadb() {
        return;
    }
    if std::env::var("RUN_FAILOVER_TRANSITION_TESTS").as_deref() != Ok("1") {
        println!("Skipping failover transition test (set RUN_FAILOVER_TRANSITION_TESTS=1)");
        return;
    }

    let port = pick_free_port();
    let binary = dbpulse_binary_path();

    let child = Command::new(binary)
        .args([
            "--dsn",
            MARIADB_DSN,
            "--interval",
            "1",
            "--listen",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn dbpulse");
    let _guard = ChildGuard(child);

    assert!(
        wait_for_pulse_value(port, 1, Duration::from_secs(40)).await,
        "Expected initial pulse=1 before failover simulation"
    );

    assert!(
        control_container("stop", "dbpulse-mariadb"),
        "Failed to stop MariaDB container (dbpulse-mariadb)"
    );
    assert!(
        wait_for_pulse_value(port, 0, Duration::from_secs(30)).await,
        "Expected pulse transition to 0 after container stop"
    );

    assert!(
        control_container("start", "dbpulse-mariadb"),
        "Failed to start MariaDB container (dbpulse-mariadb)"
    );
    assert!(
        wait_for_pulse_value(port, 1, Duration::from_secs(60)).await,
        "Expected pulse transition back to 1 after container start"
    );
}
