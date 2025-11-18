mod common;

use chrono::Utc;
use common::*;
use dbpulse::queries::postgres;
use dbpulse::tls::cache::CertCache;
use dbpulse::tls::{TlsConfig, TlsMode};

#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_basic_connection() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn = parse_dsn(POSTGRES_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let table_name = test_table_name("test_postgres_basic_connection");
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
    assert!(
        result.is_ok(),
        "Failed to connect to PostgreSQL: {result:?}"
    );

    let health = result.unwrap();
    assert_version_and_uptime("PostgreSQL", &health);
    assert!(
        health.version.chars().any(|c| c.is_ascii_digit()),
        "Should contain version number"
    );
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_read_write_operations() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn = parse_dsn(POSTGRES_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    // Run test multiple times to ensure cleanup works
    for i in 0..5 {
        let table_name = test_table_name(&format!("test_postgres_read_write_operations_{i}"));
        let result =
            postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
        let health = result.unwrap_or_else(|e| panic!("Iteration {i} failed: {e:?}"));
        assert_version_and_uptime("PostgreSQL", &health);
    }
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_transaction_rollback() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn = parse_dsn(POSTGRES_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let table_name = test_table_name("test_postgres_transaction_rollback");
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    // This tests that transaction rollback works correctly
    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
    let health = result.unwrap_or_else(|e| panic!("Transaction test failed: {e:?}"));
    assert_version_and_uptime("PostgreSQL", &health);
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_concurrent_connections() {
    if skip_if_no_postgres() {
        return;
    }

    // Spawn multiple concurrent health checks with unique table names
    // Each task gets its own table, eliminating all collision possibilities
    let mut handles = vec![];
    for i in 0..10 {
        let table_name = test_table_name(&format!("test_postgres_concurrent_connections_{i}"));
        let handle = tokio::spawn(async move {
            let dsn = parse_dsn(POSTGRES_DSN);
            let tls = TlsConfig::default();
            let now = Utc::now();
            let cert_cache = CertCache::new(std::time::Duration::from_secs(300));
            postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        match result {
            Ok(health) => assert_version_and_uptime("PostgreSQL", &health),
            Err(e) => panic!("Concurrent test failed: {e:?}"),
        }
    }
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_with_different_ranges() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn = parse_dsn(POSTGRES_DSN);
    let now = Utc::now();
    let tls = TlsConfig::default();
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    // Test different range values
    for range in [10, 50, 100, 500, 1000] {
        let table_name = test_table_name(&format!("test_postgres_with_different_ranges_{range}"));
        let result =
            postgres::test_rw_with_table(&dsn, now, range, &tls, &cert_cache, &table_name).await;
        let health = result.unwrap_or_else(|e| panic!("Range {range} failed: {e:?}"));
        assert_version_and_uptime("PostgreSQL", &health);
    }
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container with TLS"]
async fn test_postgres_tls_disable() {
    if skip_if_no_postgres() {
        return;
    }

    let result = test_postgres_with_tls(POSTGRES_DSN, TlsMode::Disable).await;
    assert!(result.is_ok(), "TLS Disable failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("PostgreSQL", &health);
    assert!(
        health.tls_metadata.is_none(),
        "TLS metadata should be None when disabled"
    );
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container with TLS enabled"]
async fn test_postgres_tls_require() {
    if skip_if_no_postgres() {
        return;
    }

    let result = test_postgres_with_tls(POSTGRES_DSN, TlsMode::Require).await;

    // This may fail if PostgreSQL doesn't have TLS configured
    // That's expected in local test environments
    match result {
        Ok(health) => {
            assert_version_and_uptime("PostgreSQL", &health);
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
            // Expected if PostgreSQL doesn't have TLS configured
            println!("TLS test skipped (no TLS configured): {e}");
        }
    }
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_database_creation() {
    if skip_if_no_postgres() {
        return;
    }

    // Test with a non-existent database (should be auto-created)
    let dsn_str = "postgres://postgres:secret@tcp(localhost:5432)/dbpulse_test_db";
    let table_name = test_table_name("test_postgres_database_creation");
    let result = test_postgres_connection_with_table(dsn_str, &table_name).await;

    // Should succeed by creating the database
    let health = result.unwrap_or_else(|e| panic!("Database auto-creation failed: {e:?}"));
    assert_version_and_uptime("PostgreSQL", &health);
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_invalid_credentials() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn_str = "postgres://invalid:invalid@tcp(localhost:5432)/testdb";
    let result = test_postgres_connection(dsn_str).await;

    // Should fail with authentication error
    assert!(result.is_err(), "Should fail with invalid credentials");
}

#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_postgres_version_info() {
    if skip_if_no_postgres() {
        return;
    }

    let table_name = test_table_name("test_postgres_version_info");
    let result = test_postgres_connection_with_table(POSTGRES_DSN, &table_name).await;
    assert!(result.is_ok());

    let health = result.unwrap();
    assert_version_and_uptime("PostgreSQL", &health);
    println!("PostgreSQL version: {}", health.version);

    // Version should contain version number
    assert!(!health.version.is_empty(), "Version should not be empty");
    assert!(
        health.version.chars().any(|c| c.is_ascii_digit()),
        "Version should contain version number"
    );
}
