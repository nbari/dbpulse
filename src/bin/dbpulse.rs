use chrono::prelude::*;
use chrono::{Duration, Utc};
use dbpulse::{options, queries};
use lazy_static::lazy_static;
use prometheus::{Encoder, Histogram, HistogramOpts, IntGauge, Registry};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use tokio::{task, time};
use warp::{Filter, Rejection, Reply};

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
async fn main() {
    REGISTRY
        .register(Box::new(PULSE.clone()))
        .expect("collector can be registered");

    REGISTRY
        .register(Box::new(RUNTIME.clone()))
        .expect("collector can be registered");

    let (v46, port, interval, opts) = options::new();

    let now = Utc::now();
    println!(
        "{} - Listening on *:{}",
        now.to_rfc3339_opts(SecondsFormat::Secs, true),
        port
    );

    let addr = if v46 {
        // tcp46 or fallback to tcp4
        match IpAddr::from_str("::0") {
            Ok(a) => a,
            Err(_) => IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        }
    } else {
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))
    };

    let metrics = warp::path("metrics").and(warp::get().and_then(metrics_handler));

    // check db pulse
    task::spawn(async move { run_loop(opts, interval).await });

    warp::serve(metrics).run((addr, port)).await;
}

/// # Errors
/// return Err if can't encode
pub async fn metrics_handler() -> Result<impl Reply, Rejection> {
    let mut buffer = Vec::new();
    let encoder = prometheus::TextEncoder::new();
    if let Err(e) = encoder.encode(&REGISTRY.gather(), &mut buffer) {
        eprintln!("could not encode custom metrics: {}", e);
    };
    Ok(buffer)
}

pub async fn run_loop(opts: mysql_async::OptsBuilder, every: i64) {
    loop {
        let mut pulse = Pulse::default();
        let now = Utc::now();
        let wait_time = Duration::seconds(every);

        // add start time
        pulse.time = now.to_rfc3339();

        let timer = RUNTIME.start_timer();
        match queries::test_rw(opts.clone(), now).await {
            Ok(rs) => {
                pulse.version = rs;
                PULSE.set(1)
            }
            Err(e) => {
                PULSE.set(0);
                eprintln!("{}", e);
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
