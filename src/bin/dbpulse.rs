use anyhow::Result;
use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use chrono::prelude::*;
use chrono::{Duration, Utc};
use dbpulse::{
    options,
    queries::{mysql, postgres},
};
use dsn::{self, DSN};
use lazy_static::lazy_static;
use prometheus::{Encoder, Histogram, HistogramOpts, IntGauge, Registry};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::mpsc, task, time};

lazy_static! {
    static ref REGISTRY: Registry = Registry::new();
    static ref PULSE: IntGauge =
        IntGauge::new("dbpuse_pulse", "1 ok, 0 error").expect("metric can be created");
    static ref RUNTIME: Histogram = Histogram::with_opts(HistogramOpts::new(
        "dbpulse_runtime",
        "pulse latency in seconds"
    ))
    .expect("metric can be created");
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Pulse {
    runtime_ms: i64,
    time: String,
    version: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    REGISTRY
        .register(Box::new(PULSE.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(RUNTIME.clone()))
        .expect("collector can be registered");

    let matches = options::new().get_matches();

    let dsn = matches
        .get_one("dsn")
        .map(|s: &String| s)
        .unwrap_or_else(|| {
            eprintln!("DSN is required");
            std::process::exit(1);
        });
    let dsn = dsn::parse(dsn)?;
    println!("DSN: {:?}", dsn);

    let interval = matches.get_one::<u16>("interval").copied().unwrap_or(30);

    let port = matches.get_one::<u16>("port").copied().unwrap_or(9300);

    let app = Router::new().route("/metrics", get(metrics_handler));

    let listener = TcpListener::bind(format!("::0:{port}")).await?;

    let now = Utc::now();
    println!(
        "{} - Listening on *:{}, interval: {}",
        now.to_rfc3339_opts(SecondsFormat::Secs, true),
        port,
        interval
    );

    // shutdown signal
    let (tx, mut rx) = mpsc::unbounded_channel();

    // check db pulse
    task::spawn(async move { run_loop(dsn, interval, tx).await });

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(async move {
            rx.recv().await;
        })
        .await?;

    Ok(())
}

/// # Errors
/// return Err if can't encode
pub async fn metrics_handler() -> impl IntoResponse {
    let mut buffer = Vec::new();
    let encoder = prometheus::TextEncoder::new();
    if let Err(e) = encoder.encode(&REGISTRY.gather(), &mut buffer) {
        eprintln!("could not encode custom metrics: {}", e);
    };
    (StatusCode::OK, buffer)
}

pub async fn run_loop(dsn: DSN, every: u16, tx: mpsc::UnboundedSender<()>) {
    loop {
        let mut pulse = Pulse::default();
        let now = Utc::now();
        let wait_time = Duration::seconds(every.into());

        // add start time
        pulse.time = now.to_rfc3339();

        let timer = RUNTIME.start_timer();

        match dsn.driver.as_str() {
            "postgres" | "postgresql" => match postgres::test_rw(&dsn, now).await {
                Ok(rs) => {
                    pulse.version = rs;
                    PULSE.set(1)
                }
                Err(e) => {
                    PULSE.set(0);
                    eprintln!("{}", e);
                }
            },
            "mysql" => match mysql::test_rw(&dsn, now).await {
                Ok(rs) => {
                    pulse.version = rs;
                    PULSE.set(1)
                }
                Err(e) => {
                    PULSE.set(0);
                    eprintln!("{}", e);
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
