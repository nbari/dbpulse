use chrono::{Duration, Utc};
use dbpulse::{options, queries};
use serde::{Deserialize, Serialize};
use std::thread;

#[derive(Serialize, Deserialize, Debug, Default)]
struct Pulse {
    time: i64,
    runtime_ms: i64,
}

#[tokio::main]
async fn main() {
    let args = options::new().unwrap();

    println!("{:#?}", args.opts);

    loop {
        let mut pulse = Pulse::default();
        let now = Utc::now();
        let wait_time = Duration::seconds(args.interval);

        // add start time
        pulse.time = now.timestamp_nanos();

        match queries::test_rw(args.opts.clone(), now).await {
            Ok(rs) => println!("{:#?}", rs),
            Err(e) => {
                eprintln!("{}", e);
            }
        }

        let runtime = Utc::now().time() - now.time();
        pulse.runtime_ms = runtime.num_milliseconds();

        if let Ok(serialized) = serde_json::to_string(&pulse) {
            println!("{}", serialized);
        }

        if let Some(remaining) = wait_time.checked_sub(&runtime) {
            let seconds_to_sleep = remaining.num_seconds() as u64;
            thread::sleep(std::time::Duration::from_secs(seconds_to_sleep));
        }
    }
}
