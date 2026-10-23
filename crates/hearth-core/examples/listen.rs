//! Test helper for the supervisor: `listen <port> [exit_after_ms [exit_code]]` accepts connections on
//! 127.0.0.1:<port> and, with a second argument, exits after that many milliseconds (code 3, or
//! the third argument).

use std::net::TcpListener;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().and_then(|p| p.parse().ok()).expect("usage: listen <port> [crash_after_ms]");
    if let Some(ms) = args.next().and_then(|m| m.parse::<u64>().ok()) {
        let code = args.next().and_then(|c| c.parse::<i32>().ok()).unwrap_or(3);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            std::process::exit(code);
        });
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    println!("listening on {port}");
    for stream in listener.incoming() {
        drop(stream);
    }
}
