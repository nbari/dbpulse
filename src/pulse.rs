use crate::{
    metrics,
    metrics::{
        DATABASE_HOST_INFO, DATABASE_UPTIME_SECONDS, DATABASE_VERSION_INFO, DB_ERRORS, DB_READONLY,
        ITERATIONS_TOTAL, LAST_RUNTIME_MS, LAST_SUCCESS, PANICS_RECOVERED, PULSE, RUNTIME,
        TLS_CERT_EXPIRY_DAYS, TLS_CONNECTION_ERRORS, TLS_INFO, encode_metrics,
    },
    queries::{HealthCheckResult, mysql, postgres},
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
    /// Read-only state, previously conveyed by appending text to `version`.
    // `default` keeps older pulse lines (written before this field existed)
    // deserializable.
    #[serde(default, skip_serializing_if = "is_false")]
    read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only_reason: Option<String>,
}

// serde's skip_serializing_if requires fn(&T) -> bool.
#[allow(clippy::trivially_copy_pass_by_ref)]
#[inline]
const fn is_false(value: &bool) -> bool {
    !*value
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
    // An unsupported driver used to be rejected only on the first check, once
    // the metrics listener was already bound and serving. Fail before any of
    // that is set up.
    let Some(database) = metric_database(dsn.driver.as_str()) else {
        anyhow::bail!(
            "unsupported database driver `{}` (expected `postgres` or `mysql`)",
            dsn.driver
        );
    };

    // Register every metric before serving. Metrics are LazyLock, so an
    // untouched one is *absent* from /metrics rather than zero, and an alert
    // written as `dbpulse_pulse == 0` would silently never fire while the very
    // first check is still running (or hanging).
    metrics::init(Some(database));
    PULSE.set(0);

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
        "{} {} - {} - Listening on {}, interval: {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
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

/// Check if an error is TLS-related
#[inline]
fn is_tls_error(error: &anyhow::Error) -> bool {
    let error_str = format!("{error:#}");
    // Both cases are matched explicitly rather than lowercasing the string,
    // which would mean a second allocation on top of the rendering above.
    error_str.contains("ssl")
        || error_str.contains("SSL")
        || error_str.contains("tls")
        || error_str.contains("TLS")
        || error_str.contains("certificate")
        || error_str.contains("Certificate")
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

/// Smallest pause between iterations, however long the previous check took.
///
/// dbpulse is a pulse check, not a load test. If an iteration overruns the
/// configured interval the loop must still back off: without a floor it would
/// re-run continuously, hammering the database precisely when it is already
/// struggling, which is the opposite of what a health probe should do.
const MIN_SLEEP: time::Duration = time::Duration::from_secs(1);

/// Longest a single check may take before it is abandoned.
///
/// Tracks the interval, but never drops below the 5s server-side statement
/// timeout, so a legitimately slow statement gets the chance to fail on its own
/// terms first. For intervals under 5s that floor wins and a check can outlast
/// its own cycle; [`MIN_SLEEP`] then keeps the loop from running back-to-back.
#[inline]
fn check_timeout(every: u16) -> time::Duration {
    time::Duration::from_secs(u64::from(every).max(5))
}

/// How long to sleep before the next iteration.
///
/// Returns the remainder of the interval, or [`MIN_SLEEP`] when the check
/// consumed the whole interval (or longer).
#[inline]
fn remaining_sleep_duration(wait_time: Duration, runtime: Duration) -> time::Duration {
    wait_time
        .checked_sub(&runtime)
        .and_then(|remaining| remaining.to_std().ok())
        .filter(|duration| !duration.is_zero())
        .unwrap_or(MIN_SLEEP)
}

#[derive(Default)]
struct LoopLabels {
    version: Option<String>,
    host: Option<String>,
    tls: Option<(String, String)>,
}

#[inline]
fn metric_database(driver: &str) -> Option<&'static str> {
    match driver {
        "postgres" | "postgresql" => Some("postgres"),
        "mysql" => Some("mysql"),
        _ => None,
    }
}

async fn run_health_check(
    database: &str,
    dsn: &DSN,
    now: DateTime<Utc>,
    range: u32,
    tls: &TlsConfig,
    cert_cache: &CertCache,
) -> anyhow::Result<HealthCheckResult> {
    match database {
        "postgres" => postgres::test_rw(dsn, now, range, tls, cert_cache).await,
        "mysql" => mysql::test_rw(dsn, now, range, tls, cert_cache).await,
        _ => unreachable!("unsupported database label"),
    }
}

/// Update `dbpulse_tls_info`, retiring the previous version/cipher series.
///
/// The TLS version and cipher are label values, so a renegotiated cipher or an
/// upgraded server would otherwise leave the old combination stuck at 1
/// forever and `dbpulse_tls_info{database="..."}` would match several rows at
/// once. Same treatment the version and host labels already get.
#[inline]
fn update_tls_info_metric(
    database: &str,
    current: Option<(&str, &str)>,
    last_tls_label: &mut Option<(String, String)>,
) {
    if let Some((previous_version, previous_cipher)) = last_tls_label.as_ref()
        && current != Some((previous_version.as_str(), previous_cipher.as_str()))
    {
        let _ = TLS_INFO.remove_label_values(&[database, previous_version, previous_cipher]);
    }

    if let Some((version, cipher)) = current {
        TLS_INFO
            .with_label_values(&[database, version, cipher])
            .set(1);
        *last_tls_label = Some((version.to_string(), cipher.to_string()));
    } else {
        *last_tls_label = None;
    }
}

/// Retire the TLS series when the connection they described no longer exists.
///
/// Shared by the success path (probe returned nothing this time) and the error
/// path (there was no usable connection at all). Applies the same rule in both:
/// absent beats stale, because a frozen `dbpulse_tls_cert_expiry_days` reports
/// a comfortable expiry for a certificate nobody has managed to read since.
fn clear_tls_metrics(database: &str, labels: &mut LoopLabels) {
    update_tls_info_metric(database, None, &mut labels.tls);
    let _ = TLS_CERT_EXPIRY_DAYS.remove_label_values(&[database]);
}

fn apply_tls_metrics(
    database: &str,
    pulse: &mut Pulse,
    result: &HealthCheckResult,
    labels: &mut LoopLabels,
) {
    let metadata = result.tls_metadata.as_ref();

    pulse.tls_version = metadata.and_then(|m| m.version.clone());
    pulse.tls_cipher = metadata.and_then(|m| m.cipher.clone());

    let tls_label = metadata.and_then(|m| match (m.version.as_deref(), m.cipher.as_deref()) {
        (Some(version), Some(cipher)) => Some((version, cipher)),
        _ => None,
    });
    update_tls_info_metric(database, tls_label, &mut labels.tls);

    // Absent beats stale: if the probe stops returning an expiry (the server
    // stopped accepting probe connections, the certificate no longer parses),
    // keeping the last value would report a comfortable expiry for a
    // certificate nobody has been able to read since.
    match metadata.and_then(|m| m.cert_expiry_days) {
        Some(days) => {
            TLS_CERT_EXPIRY_DAYS
                .with_label_values(&[database])
                .set(days);
        }
        None => {
            let _ = TLS_CERT_EXPIRY_DAYS.remove_label_values(&[database]);
        }
    }
}
fn record_success(
    database: &str,
    now: DateTime<Utc>,
    pulse: &mut Pulse,
    result: &HealthCheckResult,
    labels: &mut LoopLabels,
) {
    result.version.clone_into(&mut pulse.version);
    pulse.uptime_seconds = result.uptime_seconds;
    pulse.read_only = result.read_only;
    pulse.read_only_reason.clone_from(&result.read_only_reason);

    update_database_version_metric(database, result.version.as_str(), &mut labels.version);
    update_database_host_metric(database, result.db_host.as_deref(), &mut labels.host);
    // Absent beats stale: if the server stops reporting uptime, retire the
    // series instead of leaving the last reading to age silently.
    match result.uptime_seconds {
        Some(uptime) => DATABASE_UPTIME_SECONDS
            .with_label_values(&[database])
            .set(uptime),
        None => {
            let _ = DATABASE_UPTIME_SECONDS.remove_label_values(&[database]);
        }
    }

    if result.read_only {
        DB_READONLY.with_label_values(&[database]).set(1);
        PULSE.set(0);
        ITERATIONS_TOTAL
            .with_label_values(&[database, "error"])
            .inc();
        DB_ERRORS.with_label_values(&[database, "query"]).inc();
    } else {
        DB_READONLY.with_label_values(&[database]).set(0);
        PULSE.set(1);
        ITERATIONS_TOTAL
            .with_label_values(&[database, "success"])
            .inc();
        LAST_SUCCESS
            .with_label_values(&[database])
            .set(now.timestamp());
    }

    apply_tls_metrics(database, pulse, result, labels);
}

fn classify_error_type(database: &str, error: &anyhow::Error) -> &'static str {
    let error_str = format!("{error:#}");
    if error_str.contains("authentication")
        || error_str.contains("password")
        || (database == "mysql" && error_str.contains("Access denied"))
    {
        "authentication"
    } else if error_str.contains("timeout") {
        "timeout"
    } else if error_str.contains("connection") || error_str.contains("refused") {
        "connection"
    } else if error_str.contains("transaction") {
        "transaction"
    } else {
        "query"
    }
}

fn record_error(
    database: &str,
    error: &anyhow::Error,
    error_type: Option<&'static str>,
    tls: &TlsConfig,
    labels: &mut LoopLabels,
) {
    PULSE.set(0);
    eprintln!("{error}");
    update_database_host_metric(database, None, &mut labels.host);
    // The check failed, so whatever TLS state was last observed describes a
    // connection that no longer exists. Leaving it exported meant a database
    // that had been unreachable for hours still advertised a healthy
    // certificate expiry and an active cipher suite.
    clear_tls_metrics(database, labels);
    ITERATIONS_TOTAL
        .with_label_values(&[database, "error"])
        .inc();
    // A known cause is passed in directly rather than round-tripped through
    // message matching.
    let error_type = error_type.unwrap_or_else(|| classify_error_type(database, error));
    DB_ERRORS.with_label_values(&[database, error_type]).inc();

    if tls.mode.is_enabled() && is_tls_error(error) {
        TLS_CONNECTION_ERRORS
            .with_label_values(&[database, "handshake"])
            .inc();
    }
}

async fn run_iteration(
    dsn: &DSN,
    every: u16,
    range: u32,
    tls: &TlsConfig,
    cert_cache: &CertCache,
    tx: &mpsc::UnboundedSender<()>,
    labels: &mut LoopLabels,
) {
    let mut pulse = Pulse::default();
    let now = Utc::now();
    let wait_time = Duration::seconds(every.into());
    pulse.time = now.to_rfc3339();
    let timer = RUNTIME.start_timer();

    let Some(database) = metric_database(dsn.driver.as_str()) else {
        eprintln!("unsupported driver");
        let _ = tx.send(());
        return;
    };

    // Bound the whole check. The 5s statement / 2s lock timeouts are set with
    // SET SESSION and only apply once a connection exists: they cannot help if
    // the TCP connect or TLS handshake never completes. Without a deadline here
    // a wedged server (hung host, stuck proxy, failing-over VIP) stalls the
    // monitoring loop forever and no metric is ever updated again.
    let deadline = check_timeout(every);
    // Publish the deadline so best-effort work inside the check (the
    // certificate probe) can size its own budget against what is actually
    // left, instead of assuming a fixed slice that may no longer exist.
    let check_deadline = tokio::time::Instant::now() + deadline;
    match time::timeout(
        deadline,
        crate::tls::probe::CHECK_DEADLINE.scope(
            check_deadline,
            run_health_check(database, dsn, now, range, tls, cert_cache),
        ),
    )
    .await
    {
        Ok(Ok(result)) => record_success(database, now, &mut pulse, &result, labels),
        Ok(Err(error)) => record_error(database, &error, None, tls, labels),
        Err(_elapsed) => record_error(
            database,
            &anyhow::anyhow!("health check timed out after {}s", deadline.as_secs()),
            Some("timeout"),
            tls,
            labels,
        ),
    }

    timer.observe_duration();
    let runtime = Utc::now().signed_duration_since(now);
    pulse.runtime_ms = runtime.num_milliseconds();
    LAST_RUNTIME_MS
        .with_label_values(&[database])
        .set(pulse.runtime_ms);

    if let Ok(serialized) = serde_json::to_string(&pulse) {
        println!("{serialized}");
    }
    time::sleep(remaining_sleep_duration(wait_time, runtime)).await;
}

async fn run_loop(
    dsn: DSN,
    every: u16,
    range: u32,
    tls: TlsConfig,
    cert_cache: Arc<CertCache>,
    tx: mpsc::UnboundedSender<()>,
) {
    let mut labels = LoopLabels::default();

    loop {
        let iteration_result = std::panic::AssertUnwindSafe(run_iteration(
            &dsn,
            every,
            range,
            &tls,
            cert_cache.as_ref(),
            &tx,
            &mut labels,
        ))
        .catch_unwind()
        .await;

        if let Err(panic_info) = iteration_result {
            eprintln!("Panic in monitoring loop iteration: {panic_info:?}");
            PULSE.set(0);
            PANICS_RECOVERED.inc();
            time::sleep(time::Duration::from_secs(every.into())).await;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::metrics::DB_READONLY;
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

    fn health_result(version: &str, read_only: bool) -> HealthCheckResult {
        HealthCheckResult {
            version: version.to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
            read_only,
            read_only_reason: read_only.then(|| "Database is in recovery mode".to_string()),
        }
    }

    #[test]
    fn test_read_only_reason_reaches_the_pulse_line() {
        // Regression: read-only used to be conveyed by appending the reason to
        // the version string, so it showed up in the JSON line operators read.
        // Moving read-only into a flag dropped it from stdout entirely -- the
        // reason was captured into HealthCheckResult and never read again. The
        // old test only asserted `pulse.version`, the field the data had just
        // moved out of, so nothing caught it.
        let mut labels = LoopLabels::default();
        let mut pulse = Pulse::default();

        record_success(
            "readonly_json_db",
            Utc::now(),
            &mut pulse,
            &health_result("PostgreSQL 16.0", true),
            &mut labels,
        );

        assert!(pulse.read_only);
        assert_eq!(
            pulse.read_only_reason.as_deref(),
            Some("Database is in recovery mode")
        );

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(
            json.contains(r#""read_only":true"#),
            "read-only state missing from the pulse line: {json}"
        );
        assert!(
            json.contains("Database is in recovery mode"),
            "read-only reason missing from the pulse line: {json}"
        );
    }

    #[test]
    fn test_writable_pulse_line_omits_read_only_fields() {
        // A healthy database should not gain noise in its pulse line.
        let mut labels = LoopLabels::default();
        let mut pulse = Pulse::default();

        record_success(
            "writable_json_db",
            Utc::now(),
            &mut pulse,
            &health_result("PostgreSQL 16.0", false),
            &mut labels,
        );

        let json = serde_json::to_string(&pulse).unwrap();
        assert!(!json.contains("read_only"), "unexpected noise: {json}");
    }

    #[test]
    fn test_pulse_line_without_read_only_still_deserializes() {
        // Pulse lines written by earlier versions have no `read_only` key.
        let json = r#"{"runtime_ms":1,"time":"2024-01-01T00:00:00Z","version":"PostgreSQL 16.0"}"#;
        let pulse: Pulse = serde_json::from_str(json).unwrap();
        assert!(!pulse.read_only);
        assert!(pulse.read_only_reason.is_none());
    }

    #[test]
    fn test_classify_error_type_only_returns_known_types() {
        // The startup pre-registration in metrics::init creates a counter child
        // for every entry in ERROR_TYPES. A classifier returning something not
        // in that list would produce a series that only appears after the first
        // such failure -- the exact absent-series problem this all guards.
        let samples = [
            anyhow!("authentication failed for user"),
            anyhow!("Access denied for user 'x'"),
            anyhow!("connection timeout while reading"),
            anyhow!("connection refused"),
            anyhow!("transaction rollback failed"),
            anyhow!("something else entirely"),
            anyhow!(""),
        ];
        for database in ["mysql", "postgres"] {
            for error in &samples {
                let classified = classify_error_type(database, error);
                assert!(
                    crate::metrics::ERROR_TYPES.contains(&classified),
                    "{classified:?} is not pre-registered in ERROR_TYPES"
                );
            }
        }
    }

    fn uptime_metric_exists(database: &str) -> bool {
        crate::metrics::REGISTRY.gather().into_iter().any(|family| {
            family.name() == "dbpulse_database_uptime_seconds"
                && family.get_metric().iter().any(|m| {
                    m.get_label()
                        .iter()
                        .any(|l| l.name() == "database" && l.value() == database)
                })
        })
    }

    /// Regression: a gauge that stops being written keeps its last sample
    /// forever. When the server stopped reporting uptime, the previous reading
    /// stayed on `/metrics` and aged silently, indistinguishable from a live
    /// one. Absent beats stale.
    #[test]
    fn uptime_series_is_retired_when_the_server_stops_reporting_it() {
        let database = "uptime_retire_db";
        let mut labels = LoopLabels::default();
        let mut pulse = Pulse::default();

        let mut with_uptime = health_result("PostgreSQL 16.0", false);
        with_uptime.uptime_seconds = Some(4_242);
        record_success(database, Utc::now(), &mut pulse, &with_uptime, &mut labels);
        assert!(
            uptime_metric_exists(database),
            "a reported uptime should be published"
        );

        let without = health_result("PostgreSQL 16.0", false);
        assert!(without.uptime_seconds.is_none());
        record_success(database, Utc::now(), &mut pulse, &without, &mut labels);
        assert!(
            !uptime_metric_exists(database),
            "the series must be retired, not left at its last value"
        );
    }

    #[test]
    fn test_read_only_flag_drives_the_readonly_metric() {
        // Asserts only on the per-database labelled series: PULSE is a process
        // -wide gauge that other tests in this binary mutate concurrently.
        let database = "readonly_flag_db";
        let mut labels = LoopLabels::default();
        let mut pulse = Pulse::default();

        record_success(
            database,
            Utc::now(),
            &mut pulse,
            &health_result("PostgreSQL 16.0", true),
            &mut labels,
        );
        assert_eq!(DB_READONLY.with_label_values(&[database]).get(), 1);

        record_success(
            database,
            Utc::now(),
            &mut pulse,
            &health_result("PostgreSQL 16.0", false),
            &mut labels,
        );
        assert_eq!(DB_READONLY.with_label_values(&[database]).get(), 0);
    }

    #[test]
    fn test_read_only_state_never_leaks_into_the_version_label() {
        // Regression: read-only used to be signalled by appending text to the
        // version string, which is used as a Prometheus label. That churned the
        // series on every flip and mixed volatile state into an identity label.
        let database = "readonly_label_db";
        let version = "MariaDB 11.4.5";
        let mut labels = LoopLabels::default();
        let mut pulse = Pulse::default();

        record_success(
            database,
            Utc::now(),
            &mut pulse,
            &health_result(version, true),
            &mut labels,
        );

        assert!(version_metric_exists(database, version));
        assert_eq!(version_metric_count_for_database(database), 1);
        assert_eq!(pulse.version, version);
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

        let remaining = remaining_sleep_duration(wait_time, runtime);
        assert_eq!(remaining, std::time::Duration::from_millis(750));
    }

    #[test]
    fn test_remaining_sleep_duration_one_millisecond_remainder() {
        // Regression test for `-i 1`: runtime just under 1s must still sleep.
        let wait_time = Duration::seconds(1);
        let runtime = Duration::milliseconds(999);

        let remaining = remaining_sleep_duration(wait_time, runtime);
        assert_eq!(remaining, std::time::Duration::from_millis(1));
    }

    #[test]
    fn test_remaining_sleep_duration_subsecond_remainder_for_longer_interval() {
        let wait_time = Duration::seconds(2);
        let runtime = Duration::milliseconds(1500);

        let remaining = remaining_sleep_duration(wait_time, runtime);
        assert_eq!(remaining, std::time::Duration::from_millis(500));
    }

    #[test]
    fn test_remaining_sleep_duration_floors_when_runtime_exceeds_interval() {
        // Regression: an overrunning check used to skip the sleep entirely,
        // turning the loop into a hot loop against an already-struggling
        // database. It must always back off by at least MIN_SLEEP.
        let wait_time = Duration::seconds(1);
        let runtime = Duration::milliseconds(1200);

        assert_eq!(remaining_sleep_duration(wait_time, runtime), MIN_SLEEP);
    }

    #[test]
    fn test_remaining_sleep_duration_floors_when_runtime_matches_interval() {
        let wait_time = Duration::seconds(1);
        let runtime = Duration::seconds(1);

        assert_eq!(remaining_sleep_duration(wait_time, runtime), MIN_SLEEP);
    }

    #[test]
    fn test_remaining_sleep_duration_never_returns_zero() {
        // Whatever the interval and however long the check took, the loop must
        // never spin without pausing.
        for every in [1_u16, 5, 30, 3600] {
            for runtime_ms in [0_i64, 1, 999, 1_000, 60_000, 7_200_000] {
                let sleep = remaining_sleep_duration(
                    Duration::seconds(every.into()),
                    Duration::milliseconds(runtime_ms),
                );
                assert!(
                    !sleep.is_zero(),
                    "every={every}s runtime={runtime_ms}ms produced a zero sleep"
                );
            }
        }
    }

    #[test]
    fn test_check_timeout_is_never_below_statement_timeout() {
        // The server-side statement timeout is 5s; a shorter client deadline
        // would cut off statements that would have failed on their own terms.
        assert_eq!(check_timeout(1), std::time::Duration::from_secs(5));
        assert_eq!(check_timeout(5), std::time::Duration::from_secs(5));
    }

    #[test]
    fn test_check_timeout_tracks_the_interval() {
        // A check must never outlast its own cycle.
        assert_eq!(check_timeout(30), std::time::Duration::from_secs(30));
        assert_eq!(check_timeout(600), std::time::Duration::from_secs(600));
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

    /// Count the series of `metric_name` carrying `database`.
    fn series_count(metric_name: &str, database: &str) -> usize {
        crate::metrics::REGISTRY
            .gather()
            .into_iter()
            .find(|family| family.name() == metric_name)
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

    fn tls_result(
        version: Option<&str>,
        cipher: Option<&str>,
        expiry_days: Option<i64>,
    ) -> HealthCheckResult {
        HealthCheckResult {
            version: "TestDB 1.0".to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: Some(crate::tls::TlsMetadata {
                version: version.map(ToString::to_string),
                cipher: cipher.map(ToString::to_string),
                cert_subject: None,
                cert_issuer: None,
                cert_expiry_days: expiry_days,
            }),
            read_only: false,
            read_only_reason: None,
        }
    }

    /// Regression: `dbpulse_tls_info` encodes the version and cipher as label
    /// values, so a renegotiated cipher must retire the previous series
    /// instead of leaving it pinned at 1 forever.
    #[test]
    fn tls_info_retires_the_previous_version_and_cipher() {
        let database = "test-tls-info-churn";
        let mut labels = LoopLabels::default();

        update_tls_info_metric(
            database,
            Some(("TLSv1.2", "ECDHE-RSA-AES128-GCM-SHA256")),
            &mut labels.tls,
        );
        assert_eq!(series_count("dbpulse_tls_info", database), 1);

        update_tls_info_metric(
            database,
            Some(("TLSv1.3", "TLS_AES_256_GCM_SHA384")),
            &mut labels.tls,
        );
        assert_eq!(
            series_count("dbpulse_tls_info", database),
            1,
            "the superseded version/cipher combination must not linger"
        );
        assert_eq!(
            labels.tls,
            Some(("TLSv1.3".to_string(), "TLS_AES_256_GCM_SHA384".to_string()))
        );
    }

    /// Regression: when TLS stops being reported the series must disappear
    /// rather than keep asserting a connection that is no longer encrypted.
    #[test]
    fn tls_info_series_is_dropped_when_tls_is_no_longer_reported() {
        let database = "test-tls-info-absent";
        let mut labels = LoopLabels::default();

        update_tls_info_metric(
            database,
            Some(("TLSv1.3", "TLS_AES_256_GCM_SHA384")),
            &mut labels.tls,
        );
        assert_eq!(series_count("dbpulse_tls_info", database), 1);

        update_tls_info_metric(database, None, &mut labels.tls);
        assert_eq!(series_count("dbpulse_tls_info", database), 0);
        assert!(labels.tls.is_none());
    }

    /// Regression: a *failed* check must retire the TLS series too.
    ///
    /// `record_error` cleared the host label but left `dbpulse_tls_info` and
    /// `dbpulse_tls_cert_expiry_days` untouched, so a database that had been
    /// unreachable for hours went on advertising an active cipher suite and a
    /// comfortable certificate expiry. The success path already applied
    /// "absent beats stale"; the error path did not.
    #[test]
    fn failed_check_retires_tls_series() {
        let database = "test-tls-cleared-on-error";
        let mut labels = LoopLabels::default();
        let mut pulse = Pulse::default();

        apply_tls_metrics(
            database,
            &mut pulse,
            &tls_result(Some("TLSv1.3"), Some("TLS_AES_256_GCM_SHA384"), Some(90)),
            &mut labels,
        );
        assert_eq!(series_count("dbpulse_tls_info", database), 1);
        assert_eq!(series_count("dbpulse_tls_cert_expiry_days", database), 1);

        let tls = TlsConfig {
            mode: crate::tls::TlsMode::Require,
            ..TlsConfig::default()
        };
        record_error(
            database,
            &anyhow::anyhow!("connection refused"),
            None,
            &tls,
            &mut labels,
        );

        assert_eq!(
            series_count("dbpulse_tls_info", database),
            0,
            "a cipher suite for a connection that failed must not stay exported"
        );
        assert_eq!(
            series_count("dbpulse_tls_cert_expiry_days", database),
            0,
            "a stale expiry outlives the connection it described and hides an expiring cert"
        );
    }

    /// Regression: a certificate expiry that can no longer be probed must go
    /// absent. Keeping the last value would report a comfortable expiry for a
    /// certificate nobody has managed to read since, which is precisely the
    /// alert an operator most needs to fire.
    #[test]
    fn cert_expiry_is_dropped_when_the_probe_stops_reporting() {
        let database = "test-cert-expiry-stale";
        let mut labels = LoopLabels::default();
        let mut pulse = Pulse::default();

        apply_tls_metrics(
            database,
            &mut pulse,
            &tls_result(Some("TLSv1.3"), Some("TLS_AES_256_GCM_SHA384"), Some(30)),
            &mut labels,
        );
        assert_eq!(series_count("dbpulse_tls_cert_expiry_days", database), 1);

        apply_tls_metrics(
            database,
            &mut pulse,
            &tls_result(Some("TLSv1.3"), Some("TLS_AES_256_GCM_SHA384"), None),
            &mut labels,
        );
        assert_eq!(
            series_count("dbpulse_tls_cert_expiry_days", database),
            0,
            "a probe that stopped returning an expiry must not leave a stale value"
        );
    }

    /// The pulse line must forget TLS details once they stop being reported,
    /// instead of repeating the last successful handshake's values.
    #[test]
    fn pulse_line_clears_tls_fields_when_tls_is_absent() {
        let database = "test-pulse-tls-fields";
        let mut labels = LoopLabels::default();
        let mut pulse = Pulse::default();

        apply_tls_metrics(
            database,
            &mut pulse,
            &tls_result(Some("TLSv1.3"), Some("TLS_AES_256_GCM_SHA384"), Some(10)),
            &mut labels,
        );
        assert_eq!(pulse.tls_version.as_deref(), Some("TLSv1.3"));

        let plaintext = HealthCheckResult {
            version: "TestDB 1.0".to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
            read_only: false,
            read_only_reason: None,
        };
        apply_tls_metrics(database, &mut pulse, &plaintext, &mut labels);

        assert!(pulse.tls_version.is_none());
        assert!(pulse.tls_cipher.is_none());
    }
}
