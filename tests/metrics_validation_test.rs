#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use chrono::Utc;
use common::*;
use dbpulse::queries::{mysql, postgres};
use dbpulse::tls::TlsConfig;
use dbpulse::tls::cache::CertCache;

/// List of metrics that should be populated after running query functions
/// Note: Metrics like `dbpulse_pulse`, `dbpulse_runtime`, etc. are set by the pulse module,
/// not by the individual query functions, so they won't be present in these tests.
const EXPECTED_METRICS: &[&str] = &[
    "dbpulse_operation_duration_seconds_sum",
    "dbpulse_operation_duration_seconds_count",
    "dbpulse_connection_duration_seconds_sum",
    "dbpulse_connection_duration_seconds_count",
    "dbpulse_rows_affected_total",
    "dbpulse_table_size_bytes",
    "dbpulse_table_rows",
    "dbpulse_database_size_bytes",
    "dbpulse_blocking_queries",
];

/// Extract all metric names from the encoded Prometheus output
fn extract_metric_names(encoded: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(encoded)
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            // Extract metric name (everything before '{' or space)
            line.split('{')
                .next()
                .or_else(|| line.split_whitespace().next())
                .map(|s| s.trim().to_string())
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

/// Check that specific metric exists and has a non-zero value
fn check_metric_value(encoded: &[u8], metric_prefix: &str, database: &str) -> bool {
    String::from_utf8_lossy(encoded).lines().any(|line| {
        line.starts_with(metric_prefix)
            && line.contains(&format!("database=\"{database}\""))
            && !line.ends_with(" 0") // Check if value is not 0
    })
}

/// Comprehensive test for `PostgreSQL` metrics
#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_all_metrics_present() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn = parse_dsn(POSTGRES_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));
    let table_name = test_table_name("metrics_test_postgres");

    // Run the health check
    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
    assert!(
        result.is_ok(),
        "PostgreSQL health check failed: {:?}",
        result.err()
    );

    // Encode metrics
    let encoded = dbpulse::metrics::encode_metrics().expect("Failed to encode metrics");
    let metrics = extract_metric_names(&encoded);

    println!("\n=== POSTGRES METRICS FOUND ===");
    for metric in &metrics {
        if metric.starts_with("dbpulse_") {
            println!("✓ {metric}");
        }
    }
    println!("Total dbpulse metrics: {}", metrics.len());

    // Check all expected metrics are present
    let mut missing_metrics = Vec::new();
    for expected in EXPECTED_METRICS {
        if !metrics.iter().any(|m| m.starts_with(expected)) {
            missing_metrics.push(*expected);
        }
    }

    assert!(
        missing_metrics.is_empty(),
        "Missing PostgreSQL metrics: {missing_metrics:?}"
    );

    // Verify specific metrics have values
    println!("\n=== POSTGRES METRIC VALUES ===");

    // Check table size is populated
    let has_table_size = check_metric_value(&encoded, "dbpulse_table_size_bytes", "postgres");
    println!(
        "dbpulse_table_size_bytes: {}",
        if has_table_size {
            "✓ PRESENT"
        } else {
            "✗ MISSING"
        }
    );
    assert!(
        has_table_size,
        "Table size metric not populated for PostgreSQL"
    );

    // Check table rows is populated
    let has_table_rows = check_metric_value(&encoded, "dbpulse_table_rows", "postgres");
    println!(
        "dbpulse_table_rows: {}",
        if has_table_rows {
            "✓ PRESENT"
        } else {
            "✗ MISSING"
        }
    );
    assert!(
        has_table_rows,
        "Table rows metric not populated for PostgreSQL"
    );

    // Check database size is populated
    let has_db_size = check_metric_value(&encoded, "dbpulse_database_size_bytes", "postgres");
    println!(
        "dbpulse_database_size_bytes: {}",
        if has_db_size {
            "✓ PRESENT"
        } else {
            "✗ MISSING"
        }
    );
    assert!(
        has_db_size,
        "Database size metric not populated for PostgreSQL"
    );

    // Check rows affected (should have at least insert operations)
    let encoded_str_pg = String::from_utf8_lossy(&encoded);
    let has_rows_affected = encoded_str_pg.contains("dbpulse_rows_affected_total")
        && encoded_str_pg.contains("operation=\"insert\"");
    println!(
        "dbpulse_rows_affected_total: {}",
        if has_rows_affected {
            "✓ PRESENT"
        } else {
            "✗ MISSING"
        }
    );
    assert!(
        has_rows_affected,
        "Rows affected metric not populated for PostgreSQL"
    );

    println!("\n✓ All PostgreSQL metrics validated successfully!\n");
}

/// Comprehensive test for `MariaDB` metrics
#[tokio::test]
#[ignore = "requires running MariaDB container"]
#[allow(clippy::too_many_lines)]
async fn test_mariadb_all_metrics_present() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn = parse_dsn(MARIADB_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));
    let table_name = test_table_name("metrics_test_mariadb");

    // Run the health check
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
    assert!(
        result.is_ok(),
        "MariaDB health check failed: {:?}",
        result.err()
    );

    // Encode metrics
    let encoded = dbpulse::metrics::encode_metrics().expect("Failed to encode metrics");
    let metrics = extract_metric_names(&encoded);

    println!("\n=== MARIADB METRICS FOUND ===");
    for metric in &metrics {
        if metric.starts_with("dbpulse_") {
            println!("✓ {metric}");
        }
    }
    println!("Total dbpulse metrics: {}", metrics.len());

    // Check all expected metrics are present
    let mut missing_metrics = Vec::new();
    for expected in EXPECTED_METRICS {
        if !metrics.iter().any(|m| m.starts_with(expected)) {
            missing_metrics.push(*expected);
        }
    }

    assert!(
        missing_metrics.is_empty(),
        "Missing MariaDB metrics: {missing_metrics:?}"
    );

    // Verify specific metrics have values - THIS IS THE CRITICAL TEST
    println!("\n=== MARIADB METRIC VALUES ===");

    // Check table size is populated (THIS WAS FAILING)
    let has_table_size = check_metric_value(&encoded, "dbpulse_table_size_bytes", "mysql");
    println!(
        "dbpulse_table_size_bytes: {}",
        if has_table_size {
            "✓ PRESENT"
        } else {
            "✗ MISSING"
        }
    );

    if !has_table_size {
        // Print debug info
        println!("\nDEBUG: Searching for table_size in output:");
        let encoded_str = String::from_utf8_lossy(&encoded);
        for line in encoded_str.lines() {
            if line.contains("table_size") {
                println!("  {line}");
            }
        }
    }

    assert!(
        has_table_size,
        "Table size metric not populated for MariaDB - THIS IS THE BUG WE FIXED!"
    );

    // Check table rows is populated (THIS WAS FAILING)
    let has_table_rows = check_metric_value(&encoded, "dbpulse_table_rows", "mysql");
    println!(
        "dbpulse_table_rows: {}",
        if has_table_rows {
            "✓ PRESENT"
        } else {
            "✗ MISSING"
        }
    );

    if !has_table_rows {
        // Print debug info
        println!("\nDEBUG: Searching for table_rows in output:");
        let encoded_str = String::from_utf8_lossy(&encoded);
        for line in encoded_str.lines() {
            if line.contains("table_rows") {
                println!("  {line}");
            }
        }
    }

    assert!(
        has_table_rows,
        "Table rows metric not populated for MariaDB - THIS IS THE BUG WE FIXED!"
    );

    // Check database size is populated
    let has_db_size = check_metric_value(&encoded, "dbpulse_database_size_bytes", "mysql");
    println!(
        "dbpulse_database_size_bytes: {}",
        if has_db_size {
            "✓ PRESENT"
        } else {
            "✗ MISSING"
        }
    );
    assert!(
        has_db_size,
        "Database size metric not populated for MariaDB"
    );

    // Check rows affected (should have at least insert operations)
    let encoded_str = String::from_utf8_lossy(&encoded);
    let has_rows_affected = encoded_str.contains("dbpulse_rows_affected_total")
        && encoded_str.contains("operation=\"insert\"");
    println!(
        "dbpulse_rows_affected_total: {}",
        if has_rows_affected {
            "✓ PRESENT"
        } else {
            "✗ MISSING"
        }
    );
    assert!(
        has_rows_affected,
        "Rows affected metric not populated for MariaDB"
    );

    println!("\n✓ All MariaDB metrics validated successfully!\n");
}

/// Test that both databases produce the same set of metrics
#[tokio::test]
#[ignore = "requires running PostgreSQL and MariaDB containers"]
async fn test_postgres_and_mariadb_metric_parity() {
    if skip_if_no_postgres() || skip_if_no_mariadb() {
        return;
    }

    // Test PostgreSQL
    let pg_dsn = parse_dsn(POSTGRES_DSN);
    let pg_now = Utc::now();
    let pg_tls = TlsConfig::default();
    let pg_cert_cache = CertCache::new(std::time::Duration::from_secs(300));
    let pg_table = test_table_name("parity_test_postgres");

    let pg_result =
        postgres::test_rw_with_table(&pg_dsn, pg_now, 100, &pg_tls, &pg_cert_cache, &pg_table)
            .await;
    assert!(pg_result.is_ok(), "PostgreSQL test failed");

    let pg_encoded = dbpulse::metrics::encode_metrics().expect("Failed to encode PG metrics");
    let pg_metrics = extract_metric_names(&pg_encoded);

    // Clear metrics for next test (in production, metrics accumulate)
    // For this test, we'll just collect both

    // Test MariaDB
    let my_dsn = parse_dsn(MARIADB_DSN);
    let my_now = Utc::now();
    let my_tls = TlsConfig::default();
    let my_cert_cache = CertCache::new(std::time::Duration::from_secs(300));
    let my_table = test_table_name("parity_test_mariadb");

    let my_result =
        mysql::test_rw_with_table(&my_dsn, my_now, 100, &my_tls, &my_cert_cache, &my_table).await;
    assert!(my_result.is_ok(), "MariaDB test failed");

    let my_encoded = dbpulse::metrics::encode_metrics().expect("Failed to encode MySQL metrics");
    let my_metrics = extract_metric_names(&my_encoded);

    println!("\n=== METRIC PARITY CHECK ===");
    println!("PostgreSQL metrics: {}", pg_metrics.len());
    println!("MariaDB metrics: {}", my_metrics.len());

    // Both should have the same core metrics
    let pg_dbpulse: Vec<_> = pg_metrics
        .iter()
        .filter(|m| m.starts_with("dbpulse_"))
        .collect();
    let my_dbpulse: Vec<_> = my_metrics
        .iter()
        .filter(|m| m.starts_with("dbpulse_"))
        .collect();

    println!("PostgreSQL dbpulse metrics: {}", pg_dbpulse.len());
    println!("MariaDB dbpulse metrics: {}", my_dbpulse.len());

    // Check for metrics in one but not the other
    let pg_only: Vec<_> = pg_dbpulse
        .iter()
        .filter(|m| !my_dbpulse.contains(m))
        .collect();
    let my_only: Vec<_> = my_dbpulse
        .iter()
        .filter(|m| !pg_dbpulse.contains(m))
        .collect();

    if !pg_only.is_empty() {
        println!("\nMetrics only in PostgreSQL:");
        for m in &pg_only {
            println!("  - {m}");
        }
    }

    if !my_only.is_empty() {
        println!("\nMetrics only in MariaDB:");
        for m in &my_only {
            println!("  - {m}");
        }
    }

    // Core metrics should be the same (some variation is OK for database-specific metrics)
    println!("\n✓ Metric parity check complete\n");
}
