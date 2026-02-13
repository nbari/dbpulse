use crate::{
    metrics::{
        DATABASE_HOST_INFO, DATABASE_UPTIME_SECONDS, DATABASE_VERSION_INFO, DB_ERRORS, DB_READONLY,
        ITERATIONS_TOTAL, LAST_RUNTIME_MS, LAST_SUCCESS, PANICS_RECOVERED, PULSE, RUNTIME,
        TLS_CERT_EXPIRY_DAYS, TLS_CONNECTION_ERRORS, TLS_INFO, encode_metrics,
    },
    queries::{mysql, postgres},
    tls::{TlsConfig, cache::CertCache},
};
use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
use chrono::{Duration, Utc, prelude::*};
use dsn::DSN;
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use std::{env::var, net::IpAddr, sync::Arc};
use tokio::{net::TcpListener, sync::mpsc, task, time};

#[derive(Serialize, Deserialize, Debug, Default)]
struct Pulse {
    runtime_ms: i64,
    time: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_cipher: Option<String>,
}

/// Start the monitoring service
///
/// # Errors
///
/// Returns an error if the service fails to start or bind to the port
pub async fn start(
    dsn: DSN,
    interval: u16,
    listen: Option<IpAddr>,
    port: u16,
    range: u32,
    tls: TlsConfig,
) -> anyhow::Result<()> {
    // Metrics are already registered with REGISTRY via lazy_static macros
    let app = Router::new().route("/metrics", get(metrics_handler));

    // Bind to socket with smart fallback
    let (listener, bind_addr) = match listen {
        Some(addr) => {
            // Explicit address specified - bind to it
            let socket_addr = format!("{addr}:{port}");
            let listener = TcpListener::bind(&socket_addr).await?;
            (listener, socket_addr)
        }
        None => {
            // Auto mode: try IPv6 first, fallback to IPv4
            if let Ok(l) = TcpListener::bind(format!("::0:{port}")).await {
                (l, format!("[::]:{port}"))
            } else {
                // Fallback to IPv4 if IPv6 fails
                let socket_addr = format!("0.0.0.0:{port}");
                (TcpListener::bind(&socket_addr).await?, socket_addr)
            }
        }
    };

    println!(
        "{} - Listening on {}, interval: {}",
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        bind_addr,
        interval
    );

    // Initialize TLS certificate cache with configurable TTL (default: 1 hour)
    let cert_cache_ttl_secs = var("DBPULSE_TLS_CERT_CACHE_TTL")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(3600); // Default: 1 hour
    let cert_cache = Arc::new(CertCache::new(std::time::Duration::from_secs(
        cert_cache_ttl_secs,
    )));

    println!(
        "{} - TLS certificate cache TTL: {}s",
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        cert_cache_ttl_secs
    );

    // shutdown signal
    let (tx, mut rx) = mpsc::unbounded_channel();

    // check db pulse - keep JoinHandle to detect task failures
    let monitor_handle =
        task::spawn(async move { run_loop(dsn, interval, range, tls, cert_cache, tx).await });

    // Race between normal operation and monitoring task failure
    let server =
        axum::serve(listener, app.into_make_service()).with_graceful_shutdown(async move {
            rx.recv().await;
        });

    tokio::select! {
        result = server => {
            result?;
        }
        result = monitor_handle => {
            match result {
                Ok(()) => {
                    eprintln!("Monitoring loop exited unexpectedly");
                    anyhow::bail!("Monitoring loop stopped");
                }
                Err(e) => {
                    eprintln!("Monitoring loop panicked: {e}");
                    anyhow::bail!("Monitoring loop panicked: {e}");
                }
            }
        }
    }

    Ok(())
}

async fn metrics_handler() -> impl IntoResponse {
    match encode_metrics() {
        Ok(buffer) => (StatusCode::OK, buffer),
        Err(e) => {
            eprintln!("{e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Vec::new())
        }
    }
}

/// Check if an error is TLS-related without multiple allocations
#[inline]
fn is_tls_error(error: &anyhow::Error) -> bool {
    let error_str = format!("{error:#}");
    // Check both lowercase and uppercase variants to avoid to_lowercase() allocation
    error_str.contains("ssl")
        || error_str.contains("SSL")
        || error_str.contains("tls")
        || error_str.contains("TLS")
        || error_str.contains("certificate")
        || error_str.contains("Certificate")
}

#[inline]
fn is_database_read_only(db: &str, version: &str) -> bool {
    match db {
        // PostgreSQL replicas report recovery mode in version string.
        "postgres" | "postgresql" => {
            version.contains("recovery mode") || version.contains("read-only")
        }
        _ => version.contains("read-only"),
    }
}

#[inline]
fn update_database_version_metric(
    database: &str,
    version: &str,
    last_version: &mut Option<String>,
) {
    if let Some(previous_version) = last_version.as_deref()
        && previous_version != version
    {
        let _ = DATABASE_VERSION_INFO.remove_label_values(&[database, previous_version]);
    }

    DATABASE_VERSION_INFO
        .with_label_values(&[database, version])
        .set(1);

    *last_version = Some(version.to_string());
}

#[inline]
fn update_database_host_metric(database: &str, host: Option<&str>, last_host: &mut Option<String>) {
    if let Some(previous_host) = last_host.as_deref()
        && Some(previous_host) != host
    {
        let _ = DATABASE_HOST_INFO.remove_label_values(&[database, previous_host]);
    }

    if let Some(current_host) = host {
        DATABASE_HOST_INFO
            .with_label_values(&[database, current_host])
            .set(1);
        *last_host = Some(current_host.to_string());
    } else {
        *last_host = None;
    }
}

#[inline]
fn remaining_sleep_duration(wait_time: Duration, runtime: Duration) -> Option<time::Duration> {
    wait_time
        .checked_sub(&runtime)
        .and_then(|remaining| remaining.to_std().ok())
        .filter(|duration| !duration.is_zero())
}

#[allow(clippy::too_many_lines)]
async fn run_loop(
    dsn: DSN,
    every: u16,
    range: u32,
    tls: TlsConfig,
    cert_cache: Arc<CertCache>,
    tx: mpsc::UnboundedSender<()>,
) {
    let mut last_version_label: Option<String> = None;
    let mut last_host_label: Option<String> = None;

    loop {
        // Catch panics in individual iterations to keep loop alive
        let iteration_result = std::panic::AssertUnwindSafe(async {
            let mut pulse = Pulse::default();
            let now = Utc::now();
            let wait_time = Duration::seconds(every.into());

            // add start time
            pulse.time = now.to_rfc3339();

            let timer = RUNTIME.start_timer();

            let db_driver = dsn.driver.as_str();
            match db_driver {
                "postgres" | "postgresql" => {
                    match postgres::test_rw(&dsn, now, range, &tls, &cert_cache).await {
                        Ok(result) => {
                            result.version.clone_into(&mut pulse.version);
                            pulse.uptime_seconds = result.uptime_seconds;

                            // Record database version and uptime
                            update_database_version_metric(
                                "postgres",
                                result.version.as_str(),
                                &mut last_version_label,
                            );
                            update_database_host_metric(
                                "postgres",
                                result.db_host.as_deref(),
                                &mut last_host_label,
                            );
                            if let Some(uptime) = result.uptime_seconds {
                                DATABASE_UPTIME_SECONDS
                                    .with_label_values(&["postgres"])
                                    .set(uptime);
                            }

                            // Check for read-only mode
                            let is_read_only = is_database_read_only("postgres", &result.version);
                            if is_read_only {
                                DB_READONLY.with_label_values(&["postgres"]).set(1);
                                // Pulse must represent full read/write health.
                                PULSE.set(0);
                                ITERATIONS_TOTAL
                                    .with_label_values(&["postgres", "error"])
                                    .inc();
                                DB_ERRORS.with_label_values(&["postgres", "query"]).inc();
                            } else {
                                DB_READONLY.with_label_values(&["postgres"]).set(0);
                                PULSE.set(1);

                                // Record successful iteration
                                ITERATIONS_TOTAL
                                    .with_label_values(&["postgres", "success"])
                                    .inc();

                                // Record last success timestamp
                                LAST_SUCCESS
                                    .with_label_values(&["postgres"])
                                    .set(now.timestamp());
                            }

                            // Record TLS metrics if available
                            if let Some(ref metadata) = result.tls_metadata {
                                metadata.version.clone_into(&mut pulse.tls_version);
                                metadata.cipher.clone_into(&mut pulse.tls_cipher);

                                // Update TLS info gauge
                                if let (Some(version), Some(cipher)) =
                                    (&metadata.version, &metadata.cipher)
                                {
                                    TLS_INFO
                                        .with_label_values(&[
                                            "postgres",
                                            version.as_str(),
                                            cipher.as_str(),
                                        ])
                                        .set(1);
                                }

                                // Record certificate expiry if available
                                if let Some(days) = metadata.cert_expiry_days {
                                    TLS_CERT_EXPIRY_DAYS
                                        .with_label_values(&["postgres"])
                                        .set(days);
                                }
                            }
                        }
                        Err(e) => {
                            PULSE.set(0);
                            eprintln!("{e}");
                            update_database_host_metric("postgres", None, &mut last_host_label);

                            // Record failed iteration
                            ITERATIONS_TOTAL
                                .with_label_values(&["postgres", "error"])
                                .inc();

                            // Classify error type
                            let error_str = format!("{e:#}");
                            let error_type = if error_str.contains("authentication")
                                || error_str.contains("password")
                            {
                                "authentication"
                            } else if error_str.contains("timeout") {
                                "timeout"
                            } else if error_str.contains("connection")
                                || error_str.contains("refused")
                            {
                                "connection"
                            } else if error_str.contains("transaction") {
                                "transaction"
                            } else {
                                "query"
                            };

                            DB_ERRORS.with_label_values(&["postgres", error_type]).inc();

                            // Record TLS error if it's SSL-related
                            if tls.mode.is_enabled() && is_tls_error(&e) {
                                TLS_CONNECTION_ERRORS
                                    .with_label_values(&["postgres", "handshake"])
                                    .inc();
                            }
                        }
                    }
                }
                "mysql" => match mysql::test_rw(&dsn, now, range, &tls, &cert_cache).await {
                    Ok(result) => {
                        result.version.clone_into(&mut pulse.version);
                        pulse.uptime_seconds = result.uptime_seconds;

                        // Record database version and uptime
                        update_database_version_metric(
                            "mysql",
                            result.version.as_str(),
                            &mut last_version_label,
                        );
                        update_database_host_metric(
                            "mysql",
                            result.db_host.as_deref(),
                            &mut last_host_label,
                        );
                        if let Some(uptime) = result.uptime_seconds {
                            DATABASE_UPTIME_SECONDS
                                .with_label_values(&["mysql"])
                                .set(uptime);
                        }

                        // Check for read-only mode
                        let is_read_only = is_database_read_only("mysql", &result.version);
                        if is_read_only {
                            DB_READONLY.with_label_values(&["mysql"]).set(1);
                            // Pulse must represent full read/write health.
                            PULSE.set(0);
                            ITERATIONS_TOTAL
                                .with_label_values(&["mysql", "error"])
                                .inc();
                            DB_ERRORS.with_label_values(&["mysql", "query"]).inc();
                        } else {
                            DB_READONLY.with_label_values(&["mysql"]).set(0);
                            PULSE.set(1);

                            // Record successful iteration
                            ITERATIONS_TOTAL
                                .with_label_values(&["mysql", "success"])
                                .inc();

                            // Record last success timestamp
                            LAST_SUCCESS
                                .with_label_values(&["mysql"])
                                .set(now.timestamp());
                        }

                        // Record TLS metrics if available
                        if let Some(ref metadata) = result.tls_metadata {
                            metadata.version.clone_into(&mut pulse.tls_version);
                            metadata.cipher.clone_into(&mut pulse.tls_cipher);

                            // Update TLS info gauge
                            if let (Some(version), Some(cipher)) =
                                (&metadata.version, &metadata.cipher)
                            {
                                TLS_INFO
                                    .with_label_values(&[
                                        "mysql",
                                        version.as_str(),
                                        cipher.as_str(),
                                    ])
                                    .set(1);
                            }

                            // Record certificate expiry if available
                            if let Some(days) = metadata.cert_expiry_days {
                                TLS_CERT_EXPIRY_DAYS.with_label_values(&["mysql"]).set(days);
                            }
                        }
                    }
                    Err(e) => {
                        PULSE.set(0);
                        eprintln!("{e}");
                        update_database_host_metric("mysql", None, &mut last_host_label);

                        // Record failed iteration
                        ITERATIONS_TOTAL
                            .with_label_values(&["mysql", "error"])
                            .inc();

                        // Classify error type
                        let error_str = format!("{e:#}");
                        let error_type = if error_str.contains("authentication")
                            || error_str.contains("password")
                            || error_str.contains("Access denied")
                        {
                            "authentication"
                        } else if error_str.contains("timeout") {
                            "timeout"
                        } else if error_str.contains("connection") || error_str.contains("refused")
                        {
                            "connection"
                        } else if error_str.contains("transaction") {
                            "transaction"
                        } else {
                            "query"
                        };

                        DB_ERRORS.with_label_values(&["mysql", error_type]).inc();

                        // Record TLS error if it's SSL-related
                        if tls.mode.is_enabled() && is_tls_error(&e) {
                            TLS_CONNECTION_ERRORS
                                .with_label_values(&["mysql", "handshake"])
                                .inc();
                        }
                    }
                },
                _ => {
                    eprintln!("unsupported driver");
                    let _ = tx.send(());
                    return;
                }
            }

            timer.observe_duration();

            let end = Utc::now();
            let runtime = end.signed_duration_since(now);
            pulse.runtime_ms = runtime.num_milliseconds();

            // Record runtime metric
            let metric_db = match db_driver {
                "postgres" | "postgresql" => "postgres",
                "mysql" => "mysql",
                other => other,
            };
            LAST_RUNTIME_MS
                .with_label_values(&[metric_db])
                .set(pulse.runtime_ms);

            if let Ok(serialized) = serde_json::to_string(&pulse) {
                println!("{serialized}");
            }

            // Sleep for remaining interval time to maintain fixed interval
            if let Some(remaining) = remaining_sleep_duration(wait_time, runtime) {
                time::sleep(remaining).await;
            }
        })
        .catch_unwind()
        .await;

        // Handle panics in iteration gracefully
        if let Err(panic_info) = iteration_result {
            eprintln!("Panic in monitoring loop iteration: {panic_info:?}");
            PULSE.set(0); // Mark as unhealthy
            PANICS_RECOVERED.inc(); // Track panic recovery
            // Sleep for the interval before retrying
            time::sleep(time::Duration::from_secs(every.into())).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_is_tls_error_lowercase_ssl() {
        let error = anyhow!("Connection failed: ssl handshake error");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_is_tls_error_uppercase_ssl() {
        let error = anyhow!("Connection failed: SSL handshake error");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_is_tls_error_lowercase_tls() {
        let error = anyhow!("tls connection refused");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_is_tls_error_uppercase_tls() {
        let error = anyhow!("TLS connection refused");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_is_tls_error_lowercase_certificate() {
        let error = anyhow!("Invalid certificate chain");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_is_tls_error_uppercase_certificate() {
        let error = anyhow!("Certificate verification failed");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_is_not_tls_error() {
        let error = anyhow!("Connection timeout");
        assert!(!is_tls_error(&error));

        let error = anyhow!("Authentication failed");
        assert!(!is_tls_error(&error));

        let error = anyhow!("Database not found");
        assert!(!is_tls_error(&error));
    }

    #[test]
    fn test_is_database_read_only_postgres_recovery() {
        assert!(is_database_read_only(
            "postgres",
            "PostgreSQL 16.0 - Database is in recovery mode",
        ));
    }

    #[test]
    fn test_is_database_read_only_postgres_read_only_text() {
        assert!(is_database_read_only(
            "postgres",
            "PostgreSQL 16.0 - Transaction read-only mode enabled",
        ));
    }

    #[test]
    fn test_is_database_read_only_mysql() {
        assert!(is_database_read_only(
            "mysql",
            "MariaDB 11.4.5 - Database is in read-only mode",
        ));
        assert!(!is_database_read_only("mysql", "MariaDB 11.4.5"));
    }

    #[test]
    fn test_is_database_read_only_writable() {
        assert!(!is_database_read_only("postgres", "PostgreSQL 16.0"));
    }

    fn version_metric_exists(database: &str, version: &str) -> bool {
        crate::metrics::REGISTRY.gather().into_iter().any(|family| {
            family.name() == "dbpulse_database_version_info"
                && family.get_metric().iter().any(|metric| {
                    let labels = metric.get_label();
                    labels
                        .iter()
                        .any(|lp| lp.name() == "database" && lp.value() == database)
                        && labels
                            .iter()
                            .any(|lp| lp.name() == "version" && lp.value() == version)
                })
        })
    }

    fn version_metric_count_for_database(database: &str) -> usize {
        crate::metrics::REGISTRY
            .gather()
            .into_iter()
            .find(|family| family.name() == "dbpulse_database_version_info")
            .map_or(0, |family| {
                family
                    .get_metric()
                    .iter()
                    .filter(|metric| {
                        metric
                            .get_label()
                            .iter()
                            .any(|lp| lp.name() == "database" && lp.value() == database)
                    })
                    .count()
            })
    }

    fn host_metric_exists(database: &str, host: &str) -> bool {
        crate::metrics::REGISTRY.gather().into_iter().any(|family| {
            family.name() == "dbpulse_database_host_info"
                && family.get_metric().iter().any(|metric| {
                    let labels = metric.get_label();
                    labels
                        .iter()
                        .any(|lp| lp.name() == "database" && lp.value() == database)
                        && labels
                            .iter()
                            .any(|lp| lp.name() == "host" && lp.value() == host)
                })
        })
    }

    fn host_metric_count_for_database(database: &str) -> usize {
        crate::metrics::REGISTRY
            .gather()
            .into_iter()
            .find(|family| family.name() == "dbpulse_database_host_info")
            .map_or(0, |family| {
                family
                    .get_metric()
                    .iter()
                    .filter(|metric| {
                        metric
                            .get_label()
                            .iter()
                            .any(|lp| lp.name() == "database" && lp.value() == database)
                    })
                    .count()
            })
    }

    #[test]
    fn test_update_database_version_metric_replaces_old_version_label() {
        let database = "test-version-transition";
        let v1 = "MariaDB 11.4.5 - Database is in read-only mode";
        let v2 = "MariaDB 11.4.5";
        let mut last_version = None;

        update_database_version_metric(database, v1, &mut last_version);
        assert!(version_metric_exists(database, v1));
        assert_eq!(version_metric_count_for_database(database), 1);

        update_database_version_metric(database, v2, &mut last_version);
        assert!(version_metric_exists(database, v2));
        assert!(!version_metric_exists(database, v1));
        assert_eq!(version_metric_count_for_database(database), 1);
    }

    #[test]
    fn test_update_database_version_metric_same_version_keeps_single_series() {
        let database = "test-version-same";
        let version = "PostgreSQL 16.3";
        let mut last_version = None;

        update_database_version_metric(database, version, &mut last_version);
        update_database_version_metric(database, version, &mut last_version);

        assert!(version_metric_exists(database, version));
        assert_eq!(version_metric_count_for_database(database), 1);
    }

    #[test]
    fn test_update_database_host_metric_replaces_old_host_label() {
        let database = "test-host-transition";
        let h1 = "db-a";
        let h2 = "db-b";
        let mut last_host = None;

        update_database_host_metric(database, Some(h1), &mut last_host);
        assert!(host_metric_exists(database, h1));
        assert_eq!(host_metric_count_for_database(database), 1);

        update_database_host_metric(database, Some(h2), &mut last_host);
        assert!(host_metric_exists(database, h2));
        assert!(!host_metric_exists(database, h1));
        assert_eq!(host_metric_count_for_database(database), 1);
    }

    #[test]
    fn test_update_database_host_metric_same_host_keeps_single_series() {
        let database = "test-host-same";
        let host = "db-primary";
        let mut last_host = None;

        update_database_host_metric(database, Some(host), &mut last_host);
        update_database_host_metric(database, Some(host), &mut last_host);

        assert!(host_metric_exists(database, host));
        assert_eq!(host_metric_count_for_database(database), 1);
    }

    #[test]
    fn test_update_database_host_metric_none_clears_previous_label() {
        let database = "test-host-clear";
        let host = "db-primary";
        let mut last_host = None;

        update_database_host_metric(database, Some(host), &mut last_host);
        assert!(host_metric_exists(database, host));

        update_database_host_metric(database, None, &mut last_host);
        assert!(!host_metric_exists(database, host));
        assert_eq!(host_metric_count_for_database(database), 0);
    }

    #[test]
    fn test_remaining_sleep_duration_preserves_subsecond_interval() {
        let wait_time = Duration::seconds(1);
        let runtime = Duration::milliseconds(250);

        let remaining = remaining_sleep_duration(wait_time, runtime).unwrap();
        assert_eq!(remaining, std::time::Duration::from_millis(750));
    }

    #[test]
    fn test_remaining_sleep_duration_one_millisecond_remainder() {
        // Regression test for `-i 1`: runtime just under 1s must still sleep.
        let wait_time = Duration::seconds(1);
        let runtime = Duration::milliseconds(999);

        let remaining = remaining_sleep_duration(wait_time, runtime).unwrap();
        assert_eq!(remaining, std::time::Duration::from_millis(1));
    }

    #[test]
    fn test_remaining_sleep_duration_subsecond_remainder_for_longer_interval() {
        let wait_time = Duration::seconds(2);
        let runtime = Duration::milliseconds(1500);

        let remaining = remaining_sleep_duration(wait_time, runtime).unwrap();
        assert_eq!(remaining, std::time::Duration::from_millis(500));
    }

    #[test]
    fn test_remaining_sleep_duration_none_when_runtime_exceeds_interval() {
        let wait_time = Duration::seconds(1);
        let runtime = Duration::milliseconds(1200);

        let remaining = remaining_sleep_duration(wait_time, runtime);
        assert!(remaining.is_none());
    }

    #[test]
    fn test_remaining_sleep_duration_none_when_runtime_matches_interval() {
        let wait_time = Duration::seconds(1);
        let runtime = Duration::seconds(1);

        let remaining = remaining_sleep_duration(wait_time, runtime);
        assert!(remaining.is_none());
    }

    #[test]
    fn test_pulse_default() {
        let pulse = Pulse::default();
        assert_eq!(pulse.runtime_ms, 0);
        assert_eq!(pulse.time, "");
        assert_eq!(pulse.version, "");
        assert!(pulse.tls_version.is_none());
        assert!(pulse.tls_cipher.is_none());
    }

    #[test]
    fn test_pulse_serialization() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: 123,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "PostgreSQL 15.0".to_string(),
            tls_version: Some("TLSv1.3".to_string()),
            tls_cipher: Some("AES256-GCM-SHA384".to_string()),
        };

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(json.contains("\"runtime_ms\":123"));
        assert!(json.contains("\"version\":\"PostgreSQL 15.0\""));
        assert!(json.contains("\"tls_version\":\"TLSv1.3\""));
        assert!(json.contains("\"tls_cipher\":\"AES256-GCM-SHA384\""));
    }

    #[test]
    fn test_pulse_serialization_without_tls() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: 50,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "MySQL 8.0".to_string(),
            tls_version: None,
            tls_cipher: None,
        };

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(json.contains("\"runtime_ms\":50"));
        assert!(json.contains("\"version\":\"MySQL 8.0\""));
        // These fields should be omitted when None (skip_serializing_if)
        assert!(!json.contains("tls_version"));
        assert!(!json.contains("tls_cipher"));
    }

    #[test]
    fn test_pulse_deserialization_full() {
        let json = r#"{
            "runtime_ms": 123,
            "time": "2024-01-01T00:00:00Z",
            "version": "PostgreSQL 15.0",
            "tls_version": "TLSv1.3",
            "tls_cipher": "AES256-GCM-SHA384"
        }"#;

        let pulse: Pulse = serde_json::from_str(json).unwrap();
        assert_eq!(pulse.runtime_ms, 123);
        assert_eq!(pulse.time, "2024-01-01T00:00:00Z");
        assert_eq!(pulse.version, "PostgreSQL 15.0");
        assert_eq!(pulse.tls_version, Some("TLSv1.3".to_string()));
        assert_eq!(pulse.tls_cipher, Some("AES256-GCM-SHA384".to_string()));
    }

    #[test]
    fn test_pulse_deserialization_without_tls() {
        let json = r#"{
            "runtime_ms": 50,
            "time": "2024-01-01T00:00:00Z",
            "version": "MySQL 8.0"
        }"#;

        let pulse: Pulse = serde_json::from_str(json).unwrap();
        assert_eq!(pulse.runtime_ms, 50);
        assert_eq!(pulse.time, "2024-01-01T00:00:00Z");
        assert_eq!(pulse.version, "MySQL 8.0");
        assert!(pulse.tls_version.is_none());
        assert!(pulse.tls_cipher.is_none());
    }

    #[test]
    fn test_pulse_serialization_only_tls_version() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: 100,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "PostgreSQL 14.0".to_string(),
            tls_version: Some("TLSv1.2".to_string()),
            tls_cipher: None,
        };

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(json.contains("\"tls_version\":\"TLSv1.2\""));
        assert!(!json.contains("tls_cipher"));
    }

    #[test]
    fn test_pulse_serialization_only_tls_cipher() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: 100,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "PostgreSQL 14.0".to_string(),
            tls_version: None,
            tls_cipher: Some("AES128-SHA".to_string()),
        };

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(json.contains("\"tls_cipher\":\"AES128-SHA\""));
        assert!(!json.contains("tls_version"));
    }

    #[test]
    fn test_pulse_deserialization_partial_tls() {
        let json = r#"{
            "runtime_ms": 75,
            "time": "2024-01-01T00:00:00Z",
            "version": "MySQL 8.0",
            "tls_version": "TLSv1.2"
        }"#;

        let pulse: Pulse = serde_json::from_str(json).unwrap();
        assert_eq!(pulse.runtime_ms, 75);
        assert_eq!(pulse.tls_version, Some("TLSv1.2".to_string()));
        assert!(pulse.tls_cipher.is_none());
    }

    #[test]
    fn test_pulse_zero_runtime() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: 0,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "PostgreSQL 15.0".to_string(),
            tls_version: None,
            tls_cipher: None,
        };

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(json.contains("\"runtime_ms\":0"));
    }

    #[test]
    fn test_pulse_negative_runtime() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: -1,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "PostgreSQL 15.0".to_string(),
            tls_version: None,
            tls_cipher: None,
        };

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(json.contains("\"runtime_ms\":-1"));
    }

    #[test]
    fn test_pulse_empty_strings() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: 50,
            time: String::new(),
            version: String::new(),
            tls_version: Some(String::new()),
            tls_cipher: Some(String::new()),
        };

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(json.contains("\"time\":\"\""));
        assert!(json.contains("\"version\":\"\""));
        assert!(json.contains("\"tls_version\":\"\""));
        assert!(json.contains("\"tls_cipher\":\"\""));
    }

    #[tokio::test]
    async fn test_metrics_handler_success() {
        // Initialize metrics by accessing them
        let _ = &*PULSE;
        let _ = &*RUNTIME;

        let response = metrics_handler().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        // Verify metrics content
        assert!(body_str.contains("dbpulse_pulse"));
        assert!(body_str.contains("dbpulse_runtime"));
    }

    #[test]
    fn test_is_tls_error_mixed_case() {
        // "SSL error" contains both "SSL" and "ssl"
        let error = anyhow!("Connection failed: SSL error in ssl handshake");
        assert!(is_tls_error(&error));

        // "TLS" uppercase is detected
        let error = anyhow!("Connection failed: TLS connection refused");
        assert!(is_tls_error(&error));

        // "Certificate" with capital C is detected
        let error = anyhow!("Invalid Certificate chain");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_is_tls_error_multiple_keywords() {
        let error = anyhow!("SSL/TLS certificate verification failed");
        assert!(is_tls_error(&error));

        let error = anyhow!("TLS handshake failed: invalid certificate");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_is_tls_error_embedded_keywords() {
        let error = anyhow!("Error in sslconnect: handshake failed");
        assert!(is_tls_error(&error));

        let error = anyhow!("certificate_verify_failed in TLS setup");
        assert!(is_tls_error(&error));
    }

    #[test]
    fn test_pulse_large_runtime() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: i64::MAX,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "PostgreSQL 15.0".to_string(),
            tls_version: None,
            tls_cipher: None,
        };

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(json.contains(&format!("\"runtime_ms\":{}", i64::MAX)));
    }

    #[test]
    fn test_pulse_special_characters_in_version() {
        let pulse = Pulse {
            uptime_seconds: None,
            runtime_ms: 50,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "PostgreSQL 15.0 \"special\" <tags> & symbols".to_string(),
            tls_version: None,
            tls_cipher: None,
        };

        let json = serde_json::to_string(&pulse).unwrap();
        // Verify JSON escaping works
        let parsed: Pulse = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed.version,
            "PostgreSQL 15.0 \"special\" <tags> & symbols"
        );
    }

    #[test]
    fn test_pulse_unicode_in_fields() {
        let pulse = Pulse {
            runtime_ms: 50,
            time: "2024-01-01T00:00:00Z".to_string(),
            version: "PostgreSQL 15.0 🚀 数据库".to_string(),
            uptime_seconds: None,
            tls_version: Some("TLSv1.3 ✓".to_string()),
            tls_cipher: Some("AES256 🔒".to_string()),
        };

        let json = serde_json::to_string(&pulse).unwrap();
        let parsed: Pulse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "PostgreSQL 15.0 🚀 数据库");
        assert_eq!(parsed.tls_version, Some("TLSv1.3 ✓".to_string()));
        assert_eq!(parsed.tls_cipher, Some("AES256 🔒".to_string()));
    }

    #[test]
    fn test_pulse_debug_trait() {
        let pulse = Pulse::default();
        let debug_str = format!("{pulse:?}");
        assert!(debug_str.contains("Pulse"));
        assert!(debug_str.contains("runtime_ms"));
    }
}
