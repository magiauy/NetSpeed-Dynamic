//! Read-only probe for the same network refresh used by the widget and console.
use std::time::{Duration, Instant};
use sysinfo::Networks;

fn main() {
    let mut networks = Networks::new();
    for sample in 1..=20 {
        let start = Instant::now();
        networks.refresh_list();
        println!("sample={sample} network_refresh_ms={:.2}", start.elapsed().as_secs_f64() * 1000.0);
        std::thread::sleep(Duration::from_millis(300));
    }
}
