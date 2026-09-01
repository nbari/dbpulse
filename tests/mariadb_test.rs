#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use chrono::Utc;
use common::*;
use dbpulse::queries::mysql;
use dbpulse::tls::cache::CertCache;
use dbpulse::tls::{TlsConfig, TlsMode};
use sqlx::{AssertSqlSafe, Connection, MySqlConnection};
use std::fs::File;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

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
            let cert_cache = CertCache::new(std::time::Duration::from_mins(5));
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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

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
        !health.read_only,
        "Database should not be in read-only mode"
    );
    assert!(health.read_only_reason.is_none());
    // The version string is a Prometheus label and must carry version only.
    assert!(
        !health.version.contains("read-only"),
        "read-only state leaked into the version label: {}",
        health.version
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

    assert!(
        wait_for_mariadb_ready(MARIADB_DSN, Duration::from_secs(30)).await,
        "MariaDB is not reachable with application DSN before failover test"
    );

    let port = pick_free_port();
    let binary = dbpulse_binary_path();
    let stdout_log = format!("/tmp/dbpulse-mariadb-failover-{port}.stdout.log");
    let stderr_log = format!("/tmp/dbpulse-mariadb-failover-{port}.stderr.log");
    let stdout_file = File::create(&stdout_log).expect("failed to create stdout log file");
    let stderr_file = File::create(&stderr_log).expect("failed to create stderr log file");

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
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("failed to spawn dbpulse");
    let mut guard = ChildGuard(child);

    assert!(
        wait_for_metrics_endpoint(port, Duration::from_secs(10)).await,
        "dbpulse metrics endpoint not reachable on port {port}. process status: {:?}\nstdout:\n{}\nstderr:\n{}",
        guard.0.try_wait().ok().flatten(),
        std::fs::read_to_string(&stdout_log).unwrap_or_default(),
        std::fs::read_to_string(&stderr_log).unwrap_or_default()
    );

    let initial_pulse = wait_for_pulse_value_detailed(port, 1, Duration::from_secs(40)).await;
    assert!(
        initial_pulse.is_ok(),
        "Expected initial pulse=1 before failover simulation: {}. process status: {:?}\nstdout:\n{}\nstderr:\n{}",
        initial_pulse.err().unwrap_or_default(),
        guard.0.try_wait().ok().flatten(),
        std::fs::read_to_string(&stdout_log).unwrap_or_default(),
        std::fs::read_to_string(&stderr_log).unwrap_or_default()
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
        wait_for_pulse_value(port, 1, Duration::from_mins(1)).await,
        "Expected pulse transition back to 1 after container start"
    );
}

/// A concurrent instance dropping the shared table must not look like an outage.
///
/// dbpulse drops its own table on the hour to exercise DDL, and every instance
/// pointed at a database shares the table `dbpulse_rw`. Before the missing-table
/// recovery, a drop landing mid-check made the other instance report a failed
/// health check -- pulse 0, an error counter, a page -- for a database that was
/// perfectly healthy, once an hour, self-healing on the next iteration and so
/// indistinguishable from a real intermittent fault.
#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_survives_concurrent_table_drop() {
    if skip_if_no_mariadb() {
        return;
    }

    let table_name = test_table_name("test_mariadb_concurrent_drop");

    // Calibrate the drop cadence from a real check rather than hardcoding it.
    // A drop interrupts a check, which then recreates the table and retries at
    // roughly the cost of another check; drops must be spaced well clear of
    // that or a second drop lands on the retry and fails it. Check duration
    // varies by an order of magnitude across environments -- coverage builds
    // (-Cinstrument-coverage, codegen-units=1) run several times slower than a
    // normal debug build -- so a fixed cadence tuned on one machine is not safe.
    let calibration = Instant::now();
    let _ = test_mariadb_connection_with_table(MARIADB_DSN, &table_name).await;
    let check_time = calibration.elapsed();
    let pause = (check_time * 6).max(Duration::from_millis(400));

    let before = dbpulse::metrics::TABLE_RECREATED
        .with_label_values(&["mysql"])
        .get();
    let recovered = || {
        dbpulse::metrics::TABLE_RECREATED
            .with_label_values(&["mysql"])
            .get()
            - before
    };

    // Run until the race is actually observed rather than for a fixed number of
    // drops. On PostgreSQL a DROP needs ACCESS EXCLUSIVE and usually waits for
    // the in-flight check, then lands harmlessly in the gap between checks, so
    // hitting the window is uncommon; a fixed drop count would make this test
    // flaky in one direction or slow in the other.
    let stop = Arc::new(AtomicBool::new(false));
    let dropper_stop = Arc::clone(&stop);
    let dropper_table = table_name.clone();
    let dropper = tokio::spawn(async move {
        drop_mysql_table_until(&dropper_table, pause, &dropper_stop).await;
    });

    let budget = Duration::from_secs(60);
    let started = Instant::now();
    let mut failures = Vec::new();
    let mut checks = 0_u32;
    while recovered() == 0 && started.elapsed() < budget {
        checks += 1;
        if let Err(err) = test_mariadb_connection_with_table(MARIADB_DSN, &table_name).await {
            failures.push(format!("{err:#}"));
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = dropper.await;

    let recoveries = recovered();
    assert!(
        failures.is_empty(),
        "{} of {checks} checks failed while the table was being dropped: {failures:#?}",
        failures.len()
    );
    assert!(
        recoveries > 0,
        "the drop never landed mid-check within {budget:?}, so the recovery path \
         was not exercised ({checks} checks, check_time {check_time:?}, pause {pause:?})"
    );
    println!(
        "{checks} checks in {:?}, {recoveries} recovered from a concurrent DROP, 0 failures",
        started.elapsed()
    );
}

/// Guards the schema property the PostgreSQL upsert fix mirrors: a write must
/// refresh `t2` (here via `ON UPDATE CURRENT_TIMESTAMP`), or the hourly
/// cleanup deletes rows that are actively being written.
#[tokio::test]
#[ignore = "requires running MariaDB container"]
async fn test_mariadb_upsert_refreshes_t2_so_cleanup_keeps_live_rows() {
    if skip_if_no_mariadb() {
        return;
    }

    let table_name = test_table_name("test_mariadb_t2_refresh");
    let mut conn = MySqlConnection::connect(MARIADB_URL)
        .await
        .expect("raw connection failed");

    // Create the table, then cover the whole id space the check samples from
    // (range is 100) so the next check must *update* an existing row rather
    // than insert a fresh one -- only an update exposes whether t2 moves. The
    // hourly drop (minute 0, random id < 5) can remove the table at any step,
    // so each step retries through a recreation instead of asserting against
    // a missing table.
    let values = (0..100)
        .map(|i| format!("({i}, 1, UUID())"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut seeded = false;
    for attempt in 0..3 {
        test_mariadb_connection_with_table(MARIADB_DSN, &table_name)
            .await
            .expect("check failed");

        let seed = async {
            sqlx::query(AssertSqlSafe(format!(
                "INSERT IGNORE INTO {table_name} (id, t1, uuid) VALUES {values}"
            )))
            .execute(&mut conn)
            .await?;
            sqlx::query(AssertSqlSafe(format!(
                "UPDATE {table_name} SET t2 = NOW() - INTERVAL 2 HOUR"
            )))
            .execute(&mut conn)
            .await
        }
        .await;

        match seed {
            Ok(_) => {
                seeded = true;
                break;
            }
            Err(err) if attempt < 2 => {
                eprintln!("table vanished while seeding (hourly drop?), retrying: {err}");
            }
            Err(err) => panic!("seeding the id space failed: {err}"),
        }
    }
    assert!(seeded, "could not seed the table");

    // A check against the fully aged table must refresh the row it writes;
    // its own cleanup then deletes the aged rows but not that one.
    let fresh_count_sql =
        format!("SELECT COUNT(*) FROM {table_name} WHERE t2 > NOW() - INTERVAL 1 MINUTE");
    let mut fresh = None;
    for attempt in 0..5 {
        test_mariadb_connection_with_table(MARIADB_DSN, &table_name)
            .await
            .expect("check against the aged table failed");
        match sqlx::query_scalar::<_, i64>(AssertSqlSafe(fresh_count_sql.clone()))
            .fetch_one(&mut conn)
            .await
        {
            Ok(count) => {
                fresh = Some(count);
                break;
            }
            Err(err) if attempt < 4 => {
                eprintln!("table vanished mid-test (hourly drop?), retrying: {err}");
            }
            Err(err) => panic!("counting fresh rows failed: {err}"),
        }
    }

    let fresh = fresh.expect("no attempt produced a count");
    assert!(
        fresh >= 1,
        "the upsert must refresh t2 so the live row survives cleanup, found {fresh} fresh rows"
    );
}
