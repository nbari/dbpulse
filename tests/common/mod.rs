#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use chrono::Utc;
use dbpulse::queries::{HealthCheckResult, mysql, postgres};
use dbpulse::tls::{TlsConfig, TlsMode, cache::CertCache};
use dsn::DSN;
use std::{env, path::PathBuf, process::Command};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Duration, Instant, sleep},
};

pub const POSTGRES_DSN: &str = "postgres://postgres:secret@tcp(127.0.0.1:5432)/testdb";
pub const MARIADB_DSN: &str = "mysql://dbpulse:secret@tcp(127.0.0.1:3306)/testdb";

pub fn skip_if_no_postgres() -> bool {
    env::var("SKIP_POSTGRES_TESTS").is_ok()
}

pub fn skip_if_no_mariadb() -> bool {
    env::var("SKIP_MARIADB_TESTS").is_ok()
}

/// Create a test certificate cache with a long TTL (5 minutes)
/// This allows tests to reuse certificate data and reduces test time
pub fn test_cert_cache() -> CertCache {
    CertCache::new(std::time::Duration::from_secs(300))
}

pub async fn test_postgres_connection(dsn_str: &str) -> anyhow::Result<HealthCheckResult> {
    test_postgres_connection_with_table(dsn_str, "dbpulse_rw").await
}

pub async fn test_postgres_connection_with_table(
    dsn_str: &str,
    table_name: &str,
) -> anyhow::Result<HealthCheckResult> {
    let dsn = dsn::parse(dsn_str)?;
    let now = Utc::now();
    let tls = TlsConfig::default();
    let cert_cache = test_cert_cache();
    postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, table_name).await
}

pub async fn test_mariadb_connection(dsn_str: &str) -> anyhow::Result<HealthCheckResult> {
    test_mariadb_connection_with_table(dsn_str, "dbpulse_rw").await
}

pub async fn test_mariadb_connection_with_table(
    dsn_str: &str,
    table_name: &str,
) -> anyhow::Result<HealthCheckResult> {
    let dsn = dsn::parse(dsn_str)?;
    let now = Utc::now();
    let tls = TlsConfig::default();
    let cert_cache = test_cert_cache();
    mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, table_name).await
}

pub async fn test_postgres_with_tls(
    dsn_str: &str,
    tls_mode: TlsMode,
) -> anyhow::Result<HealthCheckResult> {
    test_postgres_with_tls_and_table(dsn_str, tls_mode, "dbpulse_rw").await
}

pub async fn test_postgres_with_tls_and_table(
    dsn_str: &str,
    tls_mode: TlsMode,
    table_name: &str,
) -> anyhow::Result<HealthCheckResult> {
    let dsn = dsn::parse(dsn_str)?;
    let now = Utc::now();
    let tls = TlsConfig {
        mode: tls_mode,
        ca: None,
        cert: None,
        key: None,
    };
    let cert_cache = test_cert_cache();
    postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, table_name).await
}

pub async fn test_mariadb_with_tls(
    dsn_str: &str,
    tls_mode: TlsMode,
) -> anyhow::Result<HealthCheckResult> {
    test_mariadb_with_tls_and_table(dsn_str, tls_mode, "dbpulse_rw").await
}

pub async fn test_mariadb_with_tls_and_table(
    dsn_str: &str,
    tls_mode: TlsMode,
    table_name: &str,
) -> anyhow::Result<HealthCheckResult> {
    let dsn = dsn::parse(dsn_str)?;
    let now = Utc::now();
    let tls = TlsConfig {
        mode: tls_mode,
        ca: None,
        cert: None,
        key: None,
    };
    let cert_cache = test_cert_cache();
    mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, table_name).await
}

pub fn parse_dsn(dsn_str: &str) -> DSN {
    dsn::parse(dsn_str).expect("Failed to parse DSN")
}

pub fn pick_free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("failed to bind random local port")
        .local_addr()
        .expect("failed to read local addr")
        .port()
}

pub fn dbpulse_binary_path() -> PathBuf {
    env::var_os("CARGO_BIN_EXE_dbpulse")
        .map_or_else(|| PathBuf::from("target/debug/dbpulse"), PathBuf::from)
}

pub fn control_container(action: &str, name: &str) -> bool {
    ["podman", "docker"].iter().any(|engine| {
        Command::new(engine)
            .args([action, name])
            .status()
            .is_ok_and(|s| s.success())
    })
}

pub async fn fetch_metrics(port: u16) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
    let request =
        format!("GET /metrics HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.ok()?;

    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
        .await
        .ok()?
        .ok()?;
    let response = String::from_utf8(response).ok()?;
    if let Some((_, body)) = response.split_once("\r\n\r\n") {
        return Some(body.to_string());
    }
    if let Some((_, body)) = response.split_once("\n\n") {
        return Some(body.to_string());
    }
    None
}

pub fn extract_pulse(metrics: &str) -> Option<i64> {
    metrics
        .lines()
        .find(|line| line.starts_with("dbpulse_pulse "))
        .and_then(|line| line.split_whitespace().last())
        .and_then(|value| value.parse::<i64>().ok())
}

pub async fn wait_for_pulse_value(port: u16, expected: i64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(metrics) = fetch_metrics(port).await
            && extract_pulse(&metrics) == Some(expected)
        {
            return true;
        }

        if Instant::now() >= deadline {
            return false;
        }

        sleep(Duration::from_millis(250)).await;
    }
}

pub async fn wait_for_metrics_endpoint(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if fetch_metrics(port).await.is_some() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(250)).await;
    }
}

pub async fn wait_for_pulse_value_detailed(
    port: u16,
    expected: i64,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut last_metrics: Option<String> = None;

    loop {
        if let Some(metrics) = fetch_metrics(port).await {
            if extract_pulse(&metrics) == Some(expected) {
                return Ok(());
            }
            last_metrics = Some(metrics);
        }

        if Instant::now() >= deadline {
            let details = last_metrics.map_or_else(
                || "no metrics response received".to_string(),
                |metrics| summarize_key_metrics(&metrics),
            );
            return Err(format!(
                "Timed out waiting for pulse={expected} on port {port}. {details}"
            ));
        }

        sleep(Duration::from_millis(250)).await;
    }
}

fn summarize_key_metrics(metrics: &str) -> String {
    let important = metrics.lines().filter(|line| {
        line.starts_with("dbpulse_pulse ")
            || line.starts_with("dbpulse_errors_total")
            || line.starts_with("dbpulse_iterations_total")
            || line.starts_with("dbpulse_database_version_info")
    });
    let summary = important.collect::<Vec<_>>().join("\n");
    if summary.is_empty() {
        "metrics received, but no key dbpulse lines were found".to_string()
    } else {
        format!("latest key metrics:\n{summary}")
    }
}

pub async fn wait_for_postgres_ready(dsn_str: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if test_postgres_connection(dsn_str).await.is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(500)).await;
    }
}

pub async fn wait_for_mariadb_ready(dsn_str: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if test_mariadb_connection(dsn_str).await.is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(500)).await;
    }
}

/// Generate a unique table name for a test
/// Uses the test name and thread ID to ensure uniqueness across parallel tests
pub fn test_table_name(test_name: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let thread_id = std::thread::current().id();
    let mut hasher = DefaultHasher::new();
    test_name.hash(&mut hasher);
    format!("{thread_id:?}").hash(&mut hasher);

    format!("dbpulse_rw_test_{:x}", hasher.finish())
}

/// Assert that a health check result contains version and uptime information
pub fn assert_version_and_uptime(db_label: &str, health: &HealthCheckResult) {
    assert!(
        !health.version.is_empty(),
        "{db_label} version should not be empty"
    );
    let uptime = health
        .uptime_seconds
        .unwrap_or_else(|| panic!("{db_label} should report uptime_seconds"));
    assert!(
        uptime >= 0,
        "{db_label} uptime_seconds must be non-negative: {uptime}"
    );
}
