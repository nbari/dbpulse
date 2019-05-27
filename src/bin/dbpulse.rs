extern crate dbpulse;

//use dbpulse::slack;
use std::{
    thread,
    time::{Duration, Instant},
    process::Command,
};


fn main() {
    loop {
        let wait_time = Duration::from_secs(30);
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
        //        slack::send_msg();
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
    Command::new("sleep").arg("3").output().expect("failed to execute process");
}
