extern crate dbpulse;
extern crate rand;

use dbpulse::slack;
use std::{
    thread,
    time::{Duration, Instant},
    process::Command,
};

use rand::Rng;


fn main() {
    loop {
        let wait_time = Duration::from_secs(10);
        let start = Instant::now();
        println!("Scheduler starting at {:?}", start);

        scheduler();

        let runtime = start.elapsed();
        if let Some(remaining) = wait_time.checked_sub(runtime) {
            println!(
                "schedule slice has time left over; sleeping for {:?}",
                remaining
            );
            thread::sleep(remaining);
        }
        slack::send_msg();
    }
}

fn scheduler() {
    let scheduler = thread::spawn(|| {
        let mut children = vec![];

        for i in 0..30 {
            children.push(thread::spawn(move || {
                println!("this is thread number {}", i);
                mock_delay();
            }));
        }

        for child in children {
            // Wait for the thread to finish. Returns a result.
            let _ = child.join();
        }

    });

    scheduler.join().expect("Scheduler panicked");
}

fn mock_delay() {
    let num = rand::thread_rng().gen_range(1, 5);
    println!("sleeping for {}", num);
    Command::new("sleep").arg(format!("{}", num)).output().expect("failed to execute process");
}
