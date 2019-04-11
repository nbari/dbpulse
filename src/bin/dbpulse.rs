extern crate dbpulse;
extern crate tokio;

use dbpulse::slack;
use std::time::{Duration, Instant};
use tokio::prelude::*;
use tokio::timer::Interval;

fn main() {
    let task = Interval::new(Instant::now(), Duration::new(3, 0))
        .for_each(|instant| {
            println!("fire; instant={:?}", instant);
            slack::send_msg();
            Ok(())
        })
    .map_err(|e| panic!("interval errored; err={:?}", e));

    tokio::run(task);
}
