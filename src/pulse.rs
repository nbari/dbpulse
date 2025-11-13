use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
use chrono::prelude::*;
use chrono::{Duration, Utc};
use dsn::DSN;
use lazy_static::lazy_static;
use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, IntGaugeVec,
    Registry, opts, register_histogram_vec_with_registry, register_int_counter_vec_with_registry,
    register_int_gauge_vec_with_registry,
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tokio::{net::TcpListener, sync::mpsc, task, time};

use crate::queries::{mysql, postgres};
use crate::tls::TlsConfig;

lazy_static! {
    static ref REGISTRY: Registry = Registry::new();
    static ref PULSE: IntGauge =
        IntGauge::new("dbpuse_pulse", "1 ok, 0 error").expect("metric can be created");
    static ref RUNTIME: Histogram = Histogram::with_opts(HistogramOpts::new(
        "dbpulse_runtime",
        "pulse latency in seconds"
    ))
    .expect("metric can be created");
    // TLS-specific metrics
    static ref TLS_HANDSHAKE_DURATION: HistogramVec = register_histogram_vec_with_registry!(
        HistogramOpts::new(
            "dbpulse_tls_handshake_duration_seconds",
            "TLS handshake duration in seconds"
        ),
        &["database"],
        REGISTRY
    )
    .expect("metric can be created");
    static ref TLS_CONNECTION_ERRORS: IntCounterVec = register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_tls_connection_errors_total",
            "Total TLS connection errors by type"
        ),
        &["database", "error_type"],
        REGISTRY
    )
    .expect("metric can be created");
    static ref TLS_INFO: IntGaugeVec = register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_tls_info",
            "TLS connection info (version, cipher) - value is always 1"
        ),
        &["database", "version", "cipher"],
        REGISTRY
    )
    .expect("metric can be created");
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Pulse {
    runtime_ms: i64,
    time: String,
    version: String,
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
    REGISTRY
        .register(Box::new(PULSE.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(RUNTIME.clone()))
        .expect("collector can be registered");

    let app = Router::new().route("/metrics", get(metrics_handler));

    // Bind to socket with smart fallback
    let (listener, bind_addr) = match listen {
        Some(addr) => {
            // Explicit address specified - bind to it
            let socket_addr = format!("{}:{}", addr, port);
            let listener = TcpListener::bind(&socket_addr).await?;
            (listener, socket_addr)
        }
        None => {
            // Auto mode: try IPv6 first, fallback to IPv4
            match TcpListener::bind(format!("::0:{port}")).await {
                Ok(l) => (l, format!("[::]:{port}")),
                Err(_) => {
                    // Fallback to IPv4 if IPv6 fails
                    let socket_addr = format!("0.0.0.0:{port}");
                    (TcpListener::bind(&socket_addr).await?, socket_addr)
                }
            }
        }
    };

    println!(
        "{} - Listening on {}, interval: {}",
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        bind_addr,
        interval
    );

    // shutdown signal
    let (tx, mut rx) = mpsc::unbounded_channel();

    // check db pulse
    task::spawn(async move { run_loop(dsn, interval, range, tls, tx).await });

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move {
            rx.recv().await;
        })
        .await?;

    Ok(())
}

async fn metrics_handler() -> impl IntoResponse {
    let mut buffer = Vec::new();

    let encoder = prometheus::TextEncoder::new();

    if let Err(e) = encoder.encode(&REGISTRY.gather(), &mut buffer) {
        eprintln!("could not encode custom metrics: {}", e);
    };

    (StatusCode::OK, buffer)
}

async fn run_loop(dsn: DSN, every: u16, range: u32, tls: TlsConfig, tx: mpsc::UnboundedSender<()>) {
    loop {
        let mut pulse = Pulse::default();
        let now = Utc::now();
        let wait_time = Duration::seconds(every.into());

        // add start time
        pulse.time = now.to_rfc3339();

        let timer = RUNTIME.start_timer();

        let db_driver = dsn.driver.as_str();
        match db_driver {
            "postgres" | "postgresql" => match postgres::test_rw(&dsn, now, range, &tls).await {
                Ok(result) => {
                    pulse.version = result.version;
                    PULSE.set(1);

                    // Record TLS metrics if available
                    if let Some(ref metadata) = result.tls_metadata {
                        pulse.tls_version = metadata.version.clone();
                        pulse.tls_cipher = metadata.cipher.clone();

                        // Update TLS info gauge
                        if let (Some(version), Some(cipher)) = (&metadata.version, &metadata.cipher)
                        {
                            TLS_INFO
                                .with_label_values(&["postgres", version.as_str(), cipher.as_str()])
                                .set(1);
                        }
                    }
                }
                Err(e) => {
                    PULSE.set(0);
                    eprintln!("{}", e);

                    // Record TLS error if it's SSL-related
                    if tls.mode.is_enabled() {
                        let error_str = e.to_string().to_lowercase();
                        if error_str.contains("ssl")
                            || error_str.contains("tls")
                            || error_str.contains("certificate")
                        {
                            TLS_CONNECTION_ERRORS
                                .with_label_values(&["postgres", "handshake"])
                                .inc();
                        }
                    }
                }
            },
            "mysql" => match mysql::test_rw(&dsn, now, range, &tls).await {
                Ok(result) => {
                    pulse.version = result.version;
                    PULSE.set(1);

                    // Record TLS metrics if available
                    if let Some(ref metadata) = result.tls_metadata {
                        pulse.tls_version = metadata.version.clone();
                        pulse.tls_cipher = metadata.cipher.clone();

                        // Update TLS info gauge
                        if let (Some(version), Some(cipher)) = (&metadata.version, &metadata.cipher)
                        {
                            TLS_INFO
                                .with_label_values(&["mysql", version.as_str(), cipher.as_str()])
                                .set(1);
                        }
                    }
                }
                Err(e) => {
                    PULSE.set(0);
                    eprintln!("{}", e);

                    // Record TLS error if it's SSL-related
                    if tls.mode.is_enabled() {
                        let error_str = e.to_string().to_lowercase();
                        if error_str.contains("ssl")
                            || error_str.contains("tls")
                            || error_str.contains("certificate")
                        {
                            TLS_CONNECTION_ERRORS
                                .with_label_values(&["mysql", "handshake"])
                                .inc();
                        }
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

        let runtime = Utc::now().time() - now.time();
        pulse.runtime_ms = runtime.num_milliseconds();

        if let Ok(serialized) = serde_json::to_string(&pulse) {
            println!("{}", serialized);
        }

        if let Some(remaining) = wait_time.checked_sub(&runtime) {
            let seconds_to_wait = remaining.num_seconds() as u64;
            time::sleep(time::Duration::from_secs(seconds_to_wait)).await;
        }
    }
}
