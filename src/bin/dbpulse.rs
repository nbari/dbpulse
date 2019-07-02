use clap::App;
use dbpulse::slack;
use dbpulse::{envs, queries};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{
    process, thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize, Debug, Default)]
struct Pulse {
    name: String,
    time: u128,
    io_error: bool,
    sql_error: bool,
    data_error: bool,
    db_runtime_s: isize,
    runtime_ms: u128,
}

#[derive(Debug, Default)]
struct Threshold {
    healthy: usize,
    unhealthy: usize,
}

fn main() {
    App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .get_matches();

    let every: u64 = envs::get_env("EVERY").parse().unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    let rw_timeout: u64 = envs::get_env("RW_TIMEOUT").parse().unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });
    let threshold_healthy: usize = envs::get_env("THRESHOLD_HEALTHY")
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("{}", e);
            process::exit(1);
        });
    let threshold_unhealthy: usize =
        envs::get_env("THRESHOLD_UNHEALTHY")
            .parse()
            .unwrap_or_else(|e| {
                eprintln!("{}", e);
                process::exit(1);
            });
    let mut threshold = Threshold::default();
    let mut skip_ok_alert: bool = true;

    // create mysql pool
    let mut opts = mysql::OptsBuilder::from_opts(envs::get_env("DSN"));
    opts.stmt_cache_size(0);
    opts.read_timeout(Some(Duration::new(rw_timeout, 0)));
    opts.write_timeout(Some(Duration::new(rw_timeout, 0)));
    let pool = mysql::Pool::new_manual(1, 2, opts).expect("Could not connect to MySQL");

    loop {
        let mut pulse = Pulse::default();
        let pool = pool.clone();
        let q = queries::new(pool);
        let start = Instant::now();
        let wait_time = Duration::from_secs(every);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        // add start time
        pulse.time = now.as_nanos();
        pulse.name = envs::get_env("ENVIRONMENT");

        // test RW
        let mut restart: bool = false;
        match q.test_rw(now.as_secs()) {
            Err(queries::Error::MySQL(e)) => match e {
                mysql::Error::IoError(e) => {
                    eprintln!("IoError: {}", e);
                    pulse.io_error = true;
                    restart = true;
                    threshold.unhealthy += 1;
                }
                _ => {
                    eprintln!("Error: {}", e);
                    pulse.sql_error = true;
                    restart = true;
                    threshold.unhealthy += 1;
                    drop(q.drop_table());
                }
            },
            Err(queries::Error::RowError(e)) => match e {
                _ => {
                    eprintln!("Error: {}", e);
                    pulse.sql_error = true;
                    restart = true;
                    threshold.unhealthy += 1;
                }
            },
            Err(queries::Error::NotMatching(e)) => {
                eprintln!("NotMatching: {}", e);
                pulse.data_error = true;
                threshold.unhealthy += 1;
                restart = true;
            }
            Err(e @ queries::Error::RowExpected) => {
                eprintln!("{}", e);
                pulse.data_error = true;
                restart = false;
            }
            Ok(t) => {
                pulse.db_runtime_s = t;
                if threshold.unhealthy > 0 {
                    threshold.unhealthy = 0;
                    threshold.healthy = 1;
                    skip_ok_alert = false;
                } else {
                    threshold.healthy += 1;
                }
            }
        };

        let runtime = start.elapsed();
        pulse.runtime_ms = runtime.as_millis();

        if let Ok(serialized) = serde_json::to_string(&pulse) {
            println!("{}", serialized);
        }

        // don't wait if get an error, try try again after 1 second
        if restart {
            thread::sleep(Duration::from_secs(1));
        } else {
            if let Some(remaining) = wait_time.checked_sub(runtime) {
                thread::sleep(remaining);
            }
        }

        // Alert onlye once
        if threshold.unhealthy == threshold_unhealthy {
            println!("threshold BAD: {}", threshold.unhealthy);
            if let Ok(rs) = q.get_user_time_state_info() {
                let (user, time, db, state, memory_usage) = rs;
                slack::send_msg(format!(
                    "user: {}, time: {}, db: {} state: {}, memory_usage: {}",
                    user, time, db, state, memory_usage
                ))
            }
        } else if threshold.healthy == threshold_healthy && !skip_ok_alert {
            println!("threshold OK: {}", threshold.healthy);
        }
    }
}
