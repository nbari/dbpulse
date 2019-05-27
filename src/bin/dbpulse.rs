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

        tasks();

        let runtime = start.elapsed();
        if let Some(remaining) = wait_time.checked_sub(runtime) {
            thread::sleep(remaining);
        }
    }
}

fn tasks() {
    let task = thread::spawn(|| {
        let mut children = vec![];

        children.push(thread::spawn(move || {
            not_sleeping();
        }));

        for child in children {
            // Wait for the thread to finish. Returns a result.
            let _ = child.join();
        }
    });
    task.join().expect("task panicked");
}

fn not_sleeping() {
    Command::new("sleep").arg("3").output().expect("failed to execute process");
}
