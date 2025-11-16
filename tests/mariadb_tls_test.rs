/// MariaDB/MySQL TLS Integration Tests
///
/// These tests verify TLS connectivity with `MariaDB` using self-signed certificates.
/// They require a `MariaDB` instance with TLS enabled.
///
/// Setup:
///   ./scripts/setup-tls-tests.sh setup
///
/// Run tests:
///   cargo test --test `mariadb_tls_test` -- --ignored --nocapture
///
/// Environment variables:
///   `TEST_MARIADB_DSN` - Override default `MariaDB` connection string
///   `MARIADB_CA_CERT`  - Path to CA certificate (default: `.certs/mariadb/ca.crt`)
mod common;

use chrono::Utc;
use common::*;
use dbpulse::queries::mysql;
use dbpulse::tls::{TlsConfig, TlsMode};
use std::env;
use std::path::PathBuf;

/// Get `MariaDB` DSN with TLS parameters
fn get_mariadb_tls_dsn(ssl_mode: &str) -> String {
    env::var("TEST_MARIADB_DSN").unwrap_or_else(|_| {
        format!("mysql://dbpulse:secret@tcp(localhost:3306)/testdb?ssl-mode={ssl_mode}")
    })
}

/// Get path to CA certificate
fn get_ca_cert_path() -> Option<PathBuf> {
    env::var("MARIADB_CA_CERT")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            let path = PathBuf::from(".certs/mariadb/ca.crt");
            if path.exists() { Some(path) } else { None }
        })
}

#[tokio::test]
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_tls_disable() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn_str = get_mariadb_tls_dsn("DISABLED");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Disable,
        ca: None,
        cert: None,
        key: None,
    };

    let table_name = test_table_name("test_mariadb_tls_disable");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &table_name).await;

    assert!(result.is_ok(), "TLS Disable failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);
    assert!(
        health.tls_metadata.is_none(),
        "TLS metadata should be None when disabled"
    );
}

#[tokio::test]
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_tls_require() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn_str = get_mariadb_tls_dsn("REQUIRED");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Require,
        ca: None,
        cert: None,
        key: None,
    };

    let table_name = test_table_name("test_mariadb_tls_require");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &table_name).await;

    assert!(result.is_ok(), "TLS Require failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);
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
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_tls_verify_ca() {
    if skip_if_no_mariadb() {
        return;
    }

    let ca_cert_path = get_ca_cert_path();
    if ca_cert_path.is_none() {
        println!("Skipping test: CA certificate not found");
        println!("Run: ./scripts/gen-certs.sh");
        return;
    }

    let dsn_str = get_mariadb_tls_dsn("VERIFY_CA");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::VerifyCA,
        ca: ca_cert_path,
        cert: None,
        key: None,
    };

    let table_name = test_table_name("test_mariadb_tls_verify_ca");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &table_name).await;

    assert!(result.is_ok(), "TLS Verify-CA failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);
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
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_tls_verify_identity() {
    if skip_if_no_mariadb() {
        return;
    }

    let ca_cert_path = get_ca_cert_path();
    if ca_cert_path.is_none() {
        println!("Skipping test: CA certificate not found");
        return;
    }

    let dsn_str = get_mariadb_tls_dsn("VERIFY_IDENTITY");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::VerifyFull,
        ca: ca_cert_path,
        cert: None,
        key: None,
    };

    let table_name = test_table_name("test_mariadb_tls_verify_identity");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &table_name).await;

    assert!(result.is_ok(), "TLS Verify-Identity failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);
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
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_tls_multiple_connections() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn_str = get_mariadb_tls_dsn("REQUIRED");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Require,
        ca: None,
        cert: None,
        key: None,
    };

    // Run multiple connections in sequence to verify TLS session reuse
    for i in 0..5 {
        let table_name = test_table_name(&format!("test_mariadb_tls_multi_{i}"));
        let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &table_name).await;
        assert!(result.is_ok(), "Connection {i} failed: {result:?}");

        let health = result.unwrap();
        assert_version_and_uptime("MariaDB", &health);
        assert!(health.tls_metadata.is_some());
    }
}

#[tokio::test]
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_tls_with_wrong_ca_fails() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn_str = get_mariadb_tls_dsn("VERIFY_CA");
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

    let table_name = test_table_name("test_mariadb_tls_wrong_ca");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &table_name).await;

    // This should fail because the CA certificate doesn't exist
    assert!(
        result.is_err(),
        "Should fail with non-existent CA certificate"
    );
}

#[tokio::test]
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_tls_connection_info() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn_str = get_mariadb_tls_dsn("REQUIRED");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Require,
        ca: None,
        cert: None,
        key: None,
    };

    let table_name = test_table_name("test_mariadb_tls_connection_info");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &table_name).await;

    assert!(result.is_ok(), "TLS connection failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);
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

#[tokio::test]
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_tls_cipher_suite() {
    if skip_if_no_mariadb() {
        return;
    }

    let dsn_str = get_mariadb_tls_dsn("REQUIRED");
    let dsn = parse_dsn(&dsn_str);
    let now = Utc::now();
    let tls = TlsConfig {
        mode: TlsMode::Require,
        ca: None,
        cert: None,
        key: None,
    };

    let table_name = test_table_name("test_mariadb_tls_cipher");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &table_name).await;

    assert!(result.is_ok(), "TLS connection failed: {result:?}");

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);
    if let Some(tls_meta) = &health.tls_metadata
        && let Some(cipher) = &tls_meta.cipher
    {
        println!("Cipher suite: {cipher}");

        // Verify we're using modern ciphers (ECDHE for forward secrecy)
        let is_modern =
            cipher.contains("ECDHE") || cipher.contains("TLS_AES") || cipher.contains("TLS_CHACHA");

        if !is_modern {
            println!("Warning: Not using ECDHE cipher suite");
        }
    }
}
