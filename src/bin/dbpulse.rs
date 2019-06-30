use dbpulse::queries;
//use dbpulse::slack;
use serde::{Deserialize, Serialize};
use serde_json;
use std::{
    env, process, thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize, Debug, Default)]
struct Pulse {
    name: String,
    time: u128,
    io_error: bool,
    sql_error: bool,
    data_error: bool,
    db_runtime_s: usize,
    runtime_ms: u128,
}

fn main() {
    let dsn = env::var("DSN").unwrap_or_else(|e| {
        println!("could not find DSN: {}", e);
        process::exit(1);
    });

    let mut opts = mysql::OptsBuilder::from_opts(dsn);
    opts.stmt_cache_size(0);
    opts.read_timeout(Some(Duration::new(3, 0)));
    opts.write_timeout(Some(Duration::new(3, 0)));
    let pool = mysql::Pool::new_manual(1, 5, opts).expect("Could not connect to MySQL");

    loop {
        let mut pulse = Pulse::default();
        let pool = pool.clone();
        let q = queries::new(pool);
        let start = Instant::now();
        let wait_time = Duration::from_secs(30);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        pulse.time = now.as_nanos();

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
            Err(queries::Error::NotMatching(e)) => {
                eprintln!("NotMatching: {}", e);
                pulse.data_error = true;
                restart = true;
            }
            Err(e @ queries::Error::NoRecords) => {
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
