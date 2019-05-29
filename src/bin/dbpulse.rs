use dbpulse::slack;
use std::{
    env,
    process,
    thread,
    time::{Duration, Instant},
};
use chrono::prelude::*;


fn main() {
    let dsn= env::var("DSN").unwrap_or_else(|e| {
        println!("could not find DSN: {}", e);
        process::exit(1);
    });

    let pool = mysql::Pool::new_manual(1,10, dsn).expect("Could not connect to MySQL");

    let utc: DateTime<Utc> = Utc::now();

    println!("Starting: {}", utc);

    loop {
        let wait_time = Duration::from_secs(30);
        let start = Instant::now();
        let mut funcs: Vec<fn(mysql::Pool)> = Vec::new();
//        funcs.push(wsrep_status);
        funcs.push(not_sleeping);
        let mut threads = Vec::new();
        for f in funcs {
            let pool = pool.clone();
            threads.push(thread::spawn(move || {
                f(pool);
            }));
        }
        for t in threads{
            let _ = t.join();
        }

        let runtime = start.elapsed();
        if let Some(remaining) = wait_time.checked_sub(runtime) {
            thread::sleep(remaining);
        }
    }
}

//fn wsrep_status(pool: mysql::Pool) {
    //let mut stmt = pool.prepare("SHOW GLOBAL STATUS WHERE Variable_name IN ('wsrep_ready', 'wsrep_cluster_size', 'wsrep_cluster_status', 'wsrep_connected', 'wsrep_local_state', 'wsrep_local_index');").unwrap();
    //for row in stmt.execute(()).unwrap() {
        //let (k, v) = mysql::from_row::<(String, String)>(row.unwrap());
        //println!("{} {}", k, v);
    //}
//}

fn not_sleeping(pool: mysql::Pool) {
    let mut stmt = pool.prepare("SELECT user, time, state, info FROM information_schema.processlist WHERE command != 'Sleep' AND time >= ? ORDER BY time DESC, id LIMIT 1;").unwrap();
    for row in stmt.execute((60,)).unwrap() {
        let (user, time, state, info) = mysql::from_row::<(String, i64, String, String)>(row.unwrap());
        println!("{} {} {} {}", user, time, state, info);
        slack::send_msg(format!("user: {}, time: {}, state: {}, info: {}", user, time, state, info));
    }
}
