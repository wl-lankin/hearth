//! Test helper for the supervisor: `listen <port> [crash_after_ms]` accepts connections on
//! 127.0.0.1:<port> and, with a second argument, exits with code 3 after that many milliseconds.

use std::net::TcpListener;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|p| p.parse().ok()).expect("usage: listen <port> [crash_after_ms]");
    if let Some(ms) = args.next().and_then(|m| m.parse::<u64>().ok()) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            std::process::exit(3);
        });
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    println!("listening on {port}");
    for stream in listener.incoming() {
        drop(stream);
    }
}
