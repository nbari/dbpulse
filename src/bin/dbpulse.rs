use std::{
    thread,
    time::{Duration, Instant},
    process::Command,
};

fn main() {
    loop {
        let scheduler = thread::spawn(|| {
            let wait_time = Duration::from_secs(5);
            let start = Instant::now();
            println!("Scheduler starting at {:?}", start);

            let mut children = vec![];

            for i in 0..5 {
                children.push(thread::spawn(move || {
                    println!("this is thread number {}", i);
                    mock_delay();
                }));
            }

            for child in children {
                // Wait for the thread to finish. Returns a result.
                let _ = child.join();
            }

            let runtime = start.elapsed();

            if let Some(remaining) = wait_time.checked_sub(runtime) {
                println!(
                    "schedule slice has time left over; sleeping for {:?}",
                    remaining
                );
                thread::sleep(remaining);
            }
        });

        scheduler.join().expect("Scheduler panicked");
    }
}

fn mock_delay() {
    Command::new("sleep").arg("3").output().expect("failed to execute process");
}
