extern crate tokio;
extern crate futures;

use futures::future::lazy;
use std::{thread, time};
use tokio::prelude::*;
use tokio::timer::Interval;

fn main() {
    let task = Interval::new(time::Instant::now(), time::Duration::new(1, 0))
        .for_each(|interval| {
            println!("Interval: {:?}", interval);
            for i in 0..5 {
                tokio::spawn(lazy(move || {
                    println!("Hello from task {}", i);
                    // mock delay
                    thread::sleep(time::Duration::from_secs(3));
                    Ok(())
                }));
            }
            Ok(())
        })
    .map_err(|e| panic!("interval errored; err={:?}", e));

    tokio::run(task);
}
