use std::{thread, time};

fn main() {
    loop {
        println!("Loop forever!");
        thread::sleep(time::Duration::new(2, 0));
    }
}
