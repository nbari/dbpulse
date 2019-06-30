use chrono::{DateTime, Utc};
use dbpulse::queries;
//use dbpulse::slack;
use std::{
    env, process, thread,
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
        let wait_time = Duration::from_secs(5);
        let start = Instant::now();
        let pool = pool.clone();
        let q = queries::new(pool);

        let now = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(n) => n.as_secs(),
            Err(_) => 0,
        };

        // test RW
        let (mut elapsed, mut restart): (usize, bool) = (0, false);
        match q.test_rw(now) {
            Err(queries::Error::MySQL(e)) => match e {
                mysql::Error::IoError(e) => {
                    eprintln!("IoError: {}", e);
                    restart = true;
                    //send_msg(pool);
                }
                _ => {
                    eprintln!("Error: {}", e);
                    restart = true;
                }
            },
            Err(queries::Error::NotMatching(e)) => {
                eprintln!("NotMatching: {}", e);
                restart = true;
            }
            Err(e @ queries::Error::NoRecords) => {
                eprintln!("{}", e);
                restart = false;
            }
            Ok(t) => elapsed = t,
        };

        if restart {}
        let runtime = start.elapsed();
        println!("elapsed: {}, runtime: {:?}", elapsed, runtime);
        if let Some(remaining) = wait_time.checked_sub(runtime) {
            thread::sleep(remaining);
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
