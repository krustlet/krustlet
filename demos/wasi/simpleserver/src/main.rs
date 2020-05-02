use std::thread::sleep;
use std::time::Duration;

fn main() {
    loop {
        println!("Hello, World!");
        sleep(Duration::from_secs(5));
    }
}
