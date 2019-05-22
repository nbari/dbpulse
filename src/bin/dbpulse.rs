extern crate tokio;
extern crate futures;

use futures::future::lazy;
use std::sync::{Arc, Barrier};
use std::{thread, time};
use tokio::prelude::*;
use tokio::timer::Interval;

fn main() {
    let task = Interval::new(time::Instant::now(), time::Duration::new(1, 0))
        .for_each(|interval| {
            println!("Interval: {:?}", interval);
            let mut handles = Vec::with_capacity(5);
            let barrier = Arc::new(Barrier::new(5));
            for i in 0..5 {
                let c = barrier.clone();
                handles.push(thread::spawn(move|| {
                    println!("before wait");
                    c.wait();
                    println!("after wait");
                }));

                tokio::spawn(lazy(move || {
                    println!("Hello from task {}", i);
                    thread::sleep(time::Duration::from_secs(3));
                    Ok(())
                }));
            }

            // Wait for other threads to finish.
            for handle in handles {
                handle.join().unwrap();
            }
            Ok(())
        })
    .map_err(|e| panic!("interval errored; err={:?}", e));

    tokio::run(task);
}
