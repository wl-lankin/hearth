//! Test stand-in for Caddy and PHP's FastCGI servers:
//! - `validate --config <file> ...`: exits 1 when the file contains "INVALID", else 0;
//! - `run --config <file> ...`: listens on the `http_port` of that Caddyfile;
//! - `... -b 127.0.0.1:<port> ...` (php-cgi): listens on that port;
//! - `... -y <conf> ...` (php-fpm): listens on the `listen = 127.0.0.1:<port>` of that config.

use std::net::TcpListener;

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn number_after(text: &str, key: &str) -> Option<u16> {
    text.lines().find_map(|l| l.trim().strip_prefix(key)).and_then(|rest| {
        rest.trim().trim_start_matches('=').trim().rsplit(':').next().and_then(|p| p.trim().parse().ok())
    })
}

/// php-fpm with `listen = "<socket path>"` (macOS): serve on that Unix socket instead of a port.
#[cfg(unix)]
fn listen_on_socket(conf: &str) -> bool {
    let Some(path) = conf
        .lines()
        .find_map(|l| l.trim().strip_prefix("listen"))
        .map(|rest| rest.trim().trim_start_matches('=').trim().trim_matches('"').to_string())
        .filter(|p| p.starts_with('/'))
    else {
        return false;
    };
    let listener = std::os::unix::net::UnixListener::bind(&path).expect("bind socket");
    for stream in listener.incoming() {
        drop(stream);
    }
    true
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(unix)]
    if let Some(conf) = arg_after(&args, "-y") {
        if listen_on_socket(&std::fs::read_to_string(conf).unwrap_or_default()) {
            return;
        }
    }
    let config = arg_after(&args, "--config").map(|f| std::fs::read_to_string(f).unwrap_or_default());
    let port = match args.first().map(String::as_str) {
        Some("validate") => {
            if config.unwrap_or_default().contains("INVALID") {
                eprintln!("Error: the config is invalid");
                std::process::exit(1);
            }
            std::process::exit(0);
        }
        Some("run") => number_after(&config.unwrap_or_default(), "http_port"),
        _ => match (arg_after(&args, "-b"), arg_after(&args, "-y")) {
            (Some(addr), _) => addr.rsplit(':').next().and_then(|p| p.parse().ok()),
            (None, Some(conf)) => number_after(&std::fs::read_to_string(conf).unwrap_or_default(), "listen"),
            _ => None,
        },
    };
    let port = port.expect("no port to listen on");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    for stream in listener.incoming() {
        drop(stream);
    }
}
