use dbpulse::{envs, queries};
//use dbpulse::slack;
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

fn main() {
    let mut opts = mysql::OptsBuilder::from_opts(envs::get_env("DSN"));
    opts.stmt_cache_size(0);
    opts.read_timeout(Some(Duration::new(3, 0)));
    opts.write_timeout(Some(Duration::new(3, 0)));
    let pool = mysql::Pool::new_manual(1, 2, opts).expect("Could not connect to MySQL");
    let every: u64 = match envs::get_env("DBPULSE_EVERY").parse() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };

    loop {
        let mut pulse = Pulse::default();
        let pool = pool.clone();
        let q = queries::new(pool);
        let start = Instant::now();
        let wait_time = Duration::from_secs(every);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

        // add start time
        pulse.time = now.as_nanos();
        pulse.name = envs::get_env("DBPULSE_ENVIRONMENT");

        // test RW
        let mut restart: bool = false;
        match q.test_rw(now.as_secs()) {
            Err(queries::Error::MySQL(e)) => match e {
                mysql::Error::IoError(e) => {
                    eprintln!("IoError: {}", e);
                    pulse.io_error = true;
                    restart = true;
                }
                _ => {
                    eprintln!("Error: {}", e);
                    pulse.sql_error = true;
                    restart = true;
                }
            },
            Err(queries::Error::RowError(e)) => match e {
                _ => {
                    eprintln!("Error: {}", e);
                    pulse.sql_error = true;
                    restart = true;
                }
            },
            Err(queries::Error::NotMatching(e)) => {
                eprintln!("NotMatching: {}", e);
                pulse.data_error = true;
                restart = true;
            }
            Err(e @ queries::Error::RowExpected) => {
                eprintln!("{}", e);
                pulse.data_error = true;
                restart = false;
            }
            Ok(t) => pulse.db_runtime_s = t,
        };

        let runtime = start.elapsed();
        pulse.runtime_ms = runtime.as_millis();

        if let Ok(serialized) = serde_json::to_string(&pulse) {
            println!("{}", serialized);
        }

        if restart {
            thread::sleep(Duration::from_secs(1));
        } else {
            if let Some(remaining) = wait_time.checked_sub(runtime) {
                thread::sleep(remaining);
            }
        }
    }
}

/*
fn send_msg(pool: mysql::Pool) {
    let mut stmt = match pool.prepare("SELECT user, time, state, info FROM information_schema.processlist WHERE command != 'Sleep' AND time >= ? ORDER BY time DESC, id LIMIT 1;") {
        Ok(stmt) => stmt,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    for row in stmt.execute((30,)).unwrap() {
        let (user, time, state, info) =
            mysql::from_row::<(String, i64, String, String)>(row.unwrap());
        println!("{} {} {} {}", user, time, state, info);
        slack::send_msg(format!(
            "user: {}, time: {}, state: {}, info: {}",
            user, time, state, info
        ));
    }
}
*/
