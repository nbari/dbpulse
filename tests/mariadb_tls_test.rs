#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
use dbpulse::tls::cache::CertCache;
use dbpulse::tls::{TlsConfig, TlsMode, TlsProbeProtocol, probe_certificate_expiry};
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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

    let table_name = test_table_name("test_mariadb_tls_disable");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

    let table_name = test_table_name("test_mariadb_tls_require");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

    let table_name = test_table_name("test_mariadb_tls_verify_ca");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

    let table_name = test_table_name("test_mariadb_tls_verify_identity");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

    // Run multiple connections in sequence to verify TLS session reuse
    for i in 0..5 {
        let table_name = test_table_name(&format!("test_mariadb_tls_multi_{i}"));
        let result =
            mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;
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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

    let table_name = test_table_name("test_mariadb_tls_wrong_ca");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

    let table_name = test_table_name("test_mariadb_tls_connection_info");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

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
    let cert_cache = CertCache::new(std::time::Duration::from_mins(5));

    let table_name = test_table_name("test_mariadb_tls_cipher");
    let result = mysql::test_rw_with_table(&dsn, now, 100, &tls, &cert_cache, &table_name).await;

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

/// DSN for the server whose certificate is signed by the same CA but names
/// only `db.invalid`, so the chain is valid and only the hostname check fails.
///
/// Derived from the primary DSN so a custom `TEST_MARIADB_DSN` keeps working;
/// set `TEST_MARIADB_MISMATCH_DSN` to point somewhere else entirely.
fn get_mariadb_mismatch_dsn(ssl_mode: &str) -> String {
    env::var("TEST_MARIADB_MISMATCH_DSN")
        .unwrap_or_else(|_| get_mariadb_tls_dsn(ssl_mode).replace(":3306", ":3307"))
}

/// `VERIFY_CA` must accept a CA-signed certificate that does not name the host,
/// *and* must still report certificate expiry.
///
/// Regression: expiry is read by a second, independent TLS handshake. When that
/// probe was given a real verifier it used a strict one, while the actual sqlx
/// connection sets `accept_invalid_hostnames` for every mode below
/// `VERIFY_IDENTITY`. The connection then succeeded while the probe failed --
/// precisely for the deployments that choose `VERIFY_CA`, which use an internal
/// CA whose certificates routinely do not match the connection address.
///
/// On MySQL/MariaDB the damage is easy to miss, because expiry is backfilled
/// from the `Ssl_server_not_after` status variable and the metric stays
/// populated while `dbpulse_tls_cert_probe_errors_total` climbs. The probe is
/// therefore asserted directly, not just its effect on the health check.
#[tokio::test]
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_verify_ca_accepts_hostname_mismatch_and_reports_expiry() {
    if skip_if_no_mariadb() {
        return;
    }

    let Some(ca_cert_path) = get_ca_cert_path() else {
        println!("Skipping test: CA certificate not found");
        return;
    };

    let dsn = parse_dsn(&get_mariadb_mismatch_dsn("VERIFY_CA"));
    let tls = TlsConfig {
        mode: TlsMode::VerifyCA,
        ca: Some(ca_cert_path),
        cert: None,
        key: None,
    };

    let probed = probe_certificate_expiry(&dsn, 3306, TlsProbeProtocol::Mysql, &tls)
        .await
        .expect("VERIFY_CA certificate probe must accept a valid chain with a mismatched name");
    assert!(
        probed.and_then(|meta| meta.cert_expiry_days).is_some(),
        "the probe must report certificate expiry under VERIFY_CA"
    );

    let table_name = test_table_name("test_mariadb_tls_verify_ca_mismatch");
    let result =
        mysql::test_rw_with_table(&dsn, Utc::now(), 100, &tls, &test_cert_cache(), &table_name)
            .await;

    assert!(
        result.is_ok(),
        "VERIFY_CA must accept a valid chain regardless of hostname: {result:?}"
    );

    let health = result.unwrap();
    assert_version_and_uptime("MariaDB", &health);

    let tls_meta = health
        .tls_metadata
        .expect("TLS metadata should be present under VERIFY_CA");

    let expiry = tls_meta
        .cert_expiry_days
        .expect("cert_expiry_days must be populated under VERIFY_CA");
    println!("Certificate expires in {expiry} days");
    assert!(
        expiry > 0,
        "test fixture certificate should not be expired, got {expiry} days"
    );
}

/// The counterpart of the test above: `VERIFY_IDENTITY` must still reject the
/// same certificate. Without this, relaxing the probe could be "fixed" by
/// relaxing hostname checking everywhere, which would be a security regression.
#[tokio::test]
#[ignore = "requires MariaDB with TLS enabled"]
async fn test_verify_full_rejects_hostname_mismatch() {
    if skip_if_no_mariadb() {
        return;
    }

    let Some(ca_cert_path) = get_ca_cert_path() else {
        println!("Skipping test: CA certificate not found");
        return;
    };

    let dsn = parse_dsn(&get_mariadb_mismatch_dsn("VERIFY_IDENTITY"));
    let tls = TlsConfig {
        mode: TlsMode::VerifyFull,
        ca: Some(ca_cert_path),
        cert: None,
        key: None,
    };

    assert!(
        probe_certificate_expiry(&dsn, 3306, TlsProbeProtocol::Mysql, &tls)
            .await
            .is_err(),
        "VERIFY_IDENTITY probe must reject a certificate that does not name the host"
    );

    let table_name = test_table_name("test_mariadb_tls_verify_full_mismatch");
    let result =
        mysql::test_rw_with_table(&dsn, Utc::now(), 100, &tls, &test_cert_cache(), &table_name)
            .await;

    assert!(
        result.is_err(),
        "VERIFY_IDENTITY must reject a certificate that does not name the host"
    );
}
