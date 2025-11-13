#![allow(dead_code)]

use chrono::Utc;
use dbpulse::queries::{HealthCheckResult, mysql, postgres};
use dbpulse::tls::{TlsConfig, TlsMode};
use dsn::DSN;
use std::env;

pub const POSTGRES_DSN: &str = "postgres://postgres:secret@tcp(localhost:5432)/testdb";
pub const MARIADB_DSN: &str = "mysql://dbpulse:secret@tcp(localhost:3306)/testdb";

pub fn skip_if_no_postgres() -> bool {
    env::var("SKIP_POSTGRES_TESTS").is_ok()
}

pub fn skip_if_no_mariadb() -> bool {
    env::var("SKIP_MARIADB_TESTS").is_ok()
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
    postgres::test_rw_with_table(&dsn, now, 100, &tls, table_name).await
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
    mysql::test_rw_with_table(&dsn, now, 100, &tls, table_name).await
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
    postgres::test_rw_with_table(&dsn, now, 100, &tls, table_name).await
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
    mysql::test_rw_with_table(&dsn, now, 100, &tls, table_name).await
}

pub fn parse_dsn(dsn_str: &str) -> DSN {
    dsn::parse(dsn_str).expect("Failed to parse DSN")
}

/// Generate a unique table name for a test
/// Uses the test name and thread ID to ensure uniqueness across parallel tests
pub fn test_table_name(test_name: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let thread_id = std::thread::current().id();
    let mut hasher = DefaultHasher::new();
    test_name.hash(&mut hasher);
    format!("{:?}", thread_id).hash(&mut hasher);

    format!("dbpulse_rw_test_{:x}", hasher.finish())
}
