extern crate tokio;
extern crate futures;

use futures::future::lazy;
use std::{thread, time};
use tokio::prelude::*;
use tokio::timer::Interval;

use futures::future::join_all;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn main() {
    let locker = Arc::new(AtomicBool::new(false));

    let task = Interval::new(time::Instant::now(), time::Duration::new(1, 0))
        .map_err(|e| panic!("interval errored; err={:?}", e))
        .for_each(move |interval| {
            let is_locked = locker.load(Ordering::SeqCst);
            println!("Interval: {:?} --- {:?}", interval, is_locked);

            if !is_locked {
                locker.store(true, Ordering::SeqCst);
                println!("locked");

                let futures: Vec<_> = (0..5)
                    .map(|i| {
                        lazy(move || {
                            println!("Hello from task {}", i);
                            // mock delay
                            thread::sleep(time::Duration::from_secs(5));
                            Ok(())
                        })
                    })
                .collect();

                let unlocker = locker.clone();
                tokio::spawn(join_all(futures).and_then(move |_| {
                    unlocker.store(false, Ordering::SeqCst);
                    println!("unlocked");

                    Ok(())
                }));
            }

            Ok(())
        });

    tokio::run(task.then(|_| Ok(())));
}
