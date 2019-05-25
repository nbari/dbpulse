extern crate futures;
extern crate tokio;

use futures::future::lazy;
use std::time::{self, Duration, Instant};

use tokio::prelude::*;
use tokio::timer::{Delay, Interval};

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

                let futures: Vec<_> = (0..1)
                    .map(|i| {
                        lazy(move || {
                            println!("Running Task-{} in  Thread {:?}", i, std::thread::current().id());
                            // mock delay
                            Ok(())
                        })
                        .and_then(move |_| {
                            println!("Task-{} is done in  Thread {:?}", i, std::thread::current().id());
                            Ok(())
                        })
                    })
                .collect();

                let unlocker = locker.clone();
                tokio::spawn(join_all(futures).and_then(move |_| {
                    unlocker.store(false, Ordering::SeqCst);
                    println!("unlocked\n\n");

                    Ok(())
                }));
            }

            Ok(())
        });

    tokio::run(task.then(|_| Ok(())));
}
