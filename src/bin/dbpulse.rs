use chrono::{DateTime, Utc};
use dbpulse::queries;
use dbpulse::slack;
use std::{
    env, error, process, thread,
    time::{Duration, Instant, SystemTime},
};

const PKG_VERSION: &'static str = env!("CARGO_PKG_VERSION");
const PKG_NAME: &'static str = env!("CARGO_PKG_NAME");

fn main() {
    let utc: DateTime<Utc> = Utc::now();
    println!("[{} - {}, {}]", PKG_NAME, PKG_VERSION, utc);

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
        let wait_time = Duration::from_secs(30);
        let start = Instant::now();
        let mut funcs: Vec<fn()> = Vec::new();
        // funcs.push(another function);
        funcs.push(test_rw);
        let mut threads = Vec::new();
        for f in funcs {
            //let pool = pool.clone();
            threads.push(thread::spawn(move || {
                f();
            }));
        }
        for t in threads {
            let _ = t.join();
        }

        let runtime = start.elapsed();
        if let Some(remaining) = wait_time.checked_sub(runtime) {
            println!(
                "sleeping for: {}s, pool: {:?}, now: {}",
                remaining.as_secs(),
                pool,
                Utc::now()
            );
            thread::sleep(remaining);
        }
    }
}

fn test_rw() {}
//fn wsrep_status(pool: mysql::Pool) {
//let mut stmt = pool.prepare("SHOW GLOBAL STATUS WHERE Variable_name IN ('wsrep_ready', 'wsrep_cluster_size', 'wsrep_cluster_status', 'wsrep_connected', 'wsrep_local_state', 'wsrep_local_index');").unwrap();
//for row in stmt.execute(()).unwrap() {
//let (k, v) = mysql::from_row::<(String, String)>(row.unwrap());
//println!("{} {}", k, v);
//}
//}

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
