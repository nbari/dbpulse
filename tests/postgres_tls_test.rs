/// `PostgreSQL` TLS Integration Tests
///
/// These tests verify TLS connectivity with `PostgreSQL` using self-signed certificates.
/// They require a `PostgreSQL` instance with TLS enabled.
///
/// Setup:
///   ./scripts/setup-tls-tests.sh setup
///
/// Run tests:
///   cargo test --test `postgres_tls_test` -- --ignored --nocapture
///
/// Environment variables:
///   `TEST_POSTGRES_DSN` - Override default `PostgreSQL` connection string
///   `POSTGRES_CA_CERT`  - Path to CA certificate (default: `.certs/postgres/ca.crt`)
mod common;

use chrono::Utc;
use common::*;
use dbpulse::queries::postgres;
use dbpulse::tls::cache::CertCache;
use dbpulse::tls::{TlsConfig, TlsMode};
use std::env;
use std::path::PathBuf;

/// Get `PostgreSQL` DSN with TLS parameters
fn get_postgres_tls_dsn(ssl_mode: &str) -> String {
    env::var("TEST_POSTGRES_DSN").unwrap_or_else(|_| {
        format!("postgresql://postgres:secret@tcp(localhost:5432)/testdb?sslmode={ssl_mode}")
    })
}

/// Get path to CA certificate
fn get_ca_cert_path() -> Option<PathBuf> {
    env::var("POSTGRES_CA_CERT")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let path = PathBuf::from(".certs/postgres/ca.crt");
            if path.exists() { Some(path) } else { None }
        })
}

#[tokio::test]
#[ignore = "requires PostgreSQL with TLS enabled"]
async fn test_tls_disable() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn_str = get_postgres_tls_dsn("disable");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Disable,
        ca: None,
        cert: None,
        key: None,
    };
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    let table_name = test_table_name("test_postgres_tls_disable");
    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

    assert!(result.is_ok(), "TLS Disable failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("PostgreSQL", &health);
    assert!(
        health.tls_metadata.is_none(),
        "TLS metadata should be None when disabled"
    );
}

#[tokio::test]
#[ignore = "requires PostgreSQL with TLS enabled"]
async fn test_tls_require() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn_str = get_postgres_tls_dsn("require");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Require,
        ca: None,
        cert: None,
        key: None,
    };
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    let table_name = test_table_name("test_postgres_tls_require");
    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

    assert!(result.is_ok(), "TLS Require failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("PostgreSQL", &health);
    assert!(
        health.tls_metadata.is_some(),
        "TLS metadata should be present when TLS is required"
    );

    let tls_meta = health.tls_metadata.unwrap();
    println!("TLS Version: {:?}", tls_meta.version);
    println!("TLS Cipher: {:?}", tls_meta.cipher);

    assert!(
        tls_meta.version.is_some() || tls_meta.cipher.is_some(),
        "Should have TLS version or cipher info"
    );
}

#[tokio::test]
#[ignore = "requires PostgreSQL with TLS enabled"]
async fn test_tls_verify_ca() {
    if skip_if_no_postgres() {
        return;
    }

    let ca_cert_path = get_ca_cert_path();
    if ca_cert_path.is_none() {
        println!("Skipping test: CA certificate not found");
        println!("Run: ./scripts/gen-certs.sh");
        return;
    }

    let dsn_str = get_postgres_tls_dsn("verify-ca");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::VerifyCA,
        ca: ca_cert_path,
        cert: None,
        key: None,
    };
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    let table_name = test_table_name("test_postgres_tls_verify_ca");
    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

    assert!(result.is_ok(), "TLS Verify-CA failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("PostgreSQL", &health);
    assert!(
        health.tls_metadata.is_some(),
        "TLS metadata should be present"
    );

    let tls_meta = health.tls_metadata.unwrap();
    println!("TLS Version: {:?}", tls_meta.version);
    println!("TLS Cipher: {:?}", tls_meta.cipher);

    // Verify we're using a strong cipher
    if let Some(cipher) = &tls_meta.cipher {
        println!("Verifying cipher strength: {cipher}");
        assert!(
            !cipher.contains("NULL") && !cipher.contains("EXPORT"),
            "Should not use weak ciphers"
        );
    }
}

#[tokio::test]
#[ignore = "requires PostgreSQL with TLS enabled"]
async fn test_tls_verify_full() {
    if skip_if_no_postgres() {
        return;
    }

    let ca_cert_path = get_ca_cert_path();
    if ca_cert_path.is_none() {
        println!("Skipping test: CA certificate not found");
        return;
    }

    let dsn_str = get_postgres_tls_dsn("verify-full");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::VerifyFull,
        ca: ca_cert_path,
        cert: None,
        key: None,
    };
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    let table_name = test_table_name("test_postgres_tls_verify_full");
    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

    assert!(result.is_ok(), "TLS Verify-Full failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("PostgreSQL", &health);
    assert!(
        health.tls_metadata.is_some(),
        "TLS metadata should be present"
    );

    let tls_meta = health.tls_metadata.unwrap();
    println!("TLS Version: {:?}", tls_meta.version);
    println!("TLS Cipher: {:?}", tls_meta.cipher);

    // Verify TLS version is modern
    if let Some(version) = &tls_meta.version {
        println!("Verifying TLS version: {version}");
        assert!(
            version.contains("TLSv1.2") || version.contains("TLSv1.3"),
            "Should use TLS 1.2 or 1.3"
        );
    }
}

#[tokio::test]
#[ignore = "requires PostgreSQL with TLS enabled"]
async fn test_tls_multiple_connections() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn_str = get_postgres_tls_dsn("require");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Require,
        ca: None,
        cert: None,
        key: None,
    };
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    // Run multiple connections in sequence to verify TLS session reuse
    for i in 0..5 {
        let table_name = test_table_name(&format!("test_postgres_tls_multi_{i}"));
        let result =
            postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
        assert!(result.is_ok(), "Connection {i} failed: {result:?}");

        let health = result.unwrap();
        assert_version_and_uptime("PostgreSQL", &health);
        assert!(health.tls_metadata.is_some());
    }
}

#[tokio::test]
#[ignore = "requires PostgreSQL with TLS enabled"]
async fn test_tls_with_wrong_ca_fails() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn_str = get_postgres_tls_dsn("verify-ca");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();

    // Use a non-existent CA certificate
    let wrong_ca = PathBuf::from("/tmp/nonexistent-ca.crt");
    let tls = TlsConfig {
        mode: TlsMode::VerifyCA,
        ca: Some(wrong_ca),
        cert: None,
        key: None,
    };
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    let table_name = test_table_name("test_postgres_tls_wrong_ca");
    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

    // This should fail because the CA certificate doesn't exist
    assert!(
        result.is_err(),
        "Should fail with non-existent CA certificate"
    );
}

#[tokio::test]
#[ignore = "requires PostgreSQL with TLS enabled"]
async fn test_tls_connection_info() {
    if skip_if_no_postgres() {
        return;
    }

    let dsn_str = get_postgres_tls_dsn("require");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Require,
        ca: None,
        cert: None,
        key: None,
    };
    let cert_cache = CertCache::new(std::time::Duration::from_secs(300));

    let table_name = test_table_name("test_postgres_tls_connection_info");
    let result = postgres::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

    assert!(result.is_ok(), "TLS connection failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("PostgreSQL", &health);
    println!("Database Version: {}", health.version);

    if let Some(tls_meta) = &health.tls_metadata {
        println!("=== TLS Connection Info ===");
        if let Some(version) = &tls_meta.version {
            println!("  TLS Version: {version}");
        }
        if let Some(cipher) = &tls_meta.cipher {
            println!("  TLS Cipher: {cipher}");
        }

        // Verify at least one piece of TLS metadata is present
        assert!(
            tls_meta.version.is_some() || tls_meta.cipher.is_some(),
            "Should have TLS metadata"
        );
    } else {
        panic!("Expected TLS metadata to be present");
    }
}
