extern crate tokio;
extern crate futures;

use futures::future::{self, Loop}; // 0.1.26
use std::time::{Duration, Instant};
use tokio::{prelude::*, timer::Delay};  // 0.1.18

fn main() {
    let repeat_count = Some(5);

    let forever = future::loop_fn(repeat_count, |repeat_count| {
        eprintln!("Loop starting at {:?}", Instant::now());

        // Resolves when all pages are done
        let batch_of_pages = future::join_all(all_pages());

        // Resolves when both all pages and a delay of 1 second is done
        let wait = Future::join(batch_of_pages, ez_delay_ms(1000));

        // Run all this again
        wait.map(move |_| {
            if let Some(0) = repeat_count {
                Loop::Break(())
            } else {
                Loop::Continue(repeat_count.map(|c| c - 1))
            }
        })
    });

    tokio::run(forever.map_err(drop));
}

fn all_pages() -> Vec<Box<dyn Future<Item = (), Error = ()> + Send + 'static>> {
    vec![Box::new(page("a", 1000)), Box::new(page("b", 2000)), Box::new(page("c", 3000))]
}

fn page(name: &'static str, time_ms: u64) -> impl Future<Item = (), Error = ()> + Send + 'static {
    future::ok(())
        .inspect(move |_| eprintln!("page {} starting", name))
        .and_then(move |_| ez_delay_ms(time_ms))
        .inspect(move |_| eprintln!("page {} done", name))
}

fn ez_delay_ms(ms: u64) -> impl Future<Item = (), Error = ()> + Send + 'static {
    Delay::new(Instant::now() + Duration::from_millis(ms)).map_err(drop)
}
