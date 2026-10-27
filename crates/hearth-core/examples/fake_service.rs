//! Test stand-in for a service program. It finds its port in the usual flags (`--port=N`,
//! `--port N`, `-p N`, `--api-port N`, `--http-addr host:N`, `--address host:N`) and listens on it.
//! Called with `--initialize-insecure`, `-D` (initdb) or a `--datadir` and no port (install-db), it
//! leaves `initialized` in the data folder and exits. Called with `shutdown` or `stop` (admin
//! tools), it leaves `shutdown.called` next to itself and exits. Every call appends its arguments
//! to `calls.log` next to itself.

use std::io::Write;
use std::net::TcpListener;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let here = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(here.join("calls.log")) {
        let _ = writeln!(log, "{}", args.join(" "));
    }
    let value = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
            .or_else(|| args.iter().find_map(|a| a.strip_prefix(&format!("{flag}=")).map(str::to_string)))
    };
    if args.iter().any(|a| a == "shutdown" || a == "stop") {
        std::fs::write(here.join("shutdown.called"), b"1").unwrap();
        return;
    }
    let port = value("--port")
        .or_else(|| value("-p"))
        .or_else(|| value("--api-port"))
        .or_else(|| value("--http-addr").and_then(|a| a.rsplit(':').next().map(str::to_string)))
        .or_else(|| value("--address").and_then(|a| a.rsplit(':').next().map(str::to_string)))
        .and_then(|p| p.parse::<u16>().ok());
    let data = value("--datadir").or_else(|| value("-D"));
    let initializing = args.iter().any(|a| a == "--initialize-insecure")
        || (args.first().is_some_and(|a| a == "-D") && value("-U").is_some())
        || (data.is_some()
            && args.iter().any(|a| a.starts_with("--port="))
            && !args.iter().any(|a| a.starts_with("--bind-address")));
    if initializing {
        let data = data.expect("a data folder to set up");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(std::path::Path::new(&data).join("initialized"), b"1").unwrap();
        return;
    }
    // Like a real database: an admin tool's shutdown makes the server exit.
    let signal = here.join("shutdown.called");
    let _ = std::fs::remove_file(&signal);
    std::thread::spawn(move || loop {
        if signal.exists() {
            std::process::exit(0);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    });
    let listener = TcpListener::bind(("127.0.0.1", port.expect("a port"))).expect("bind");
    for stream in listener.incoming() {
        drop(stream);
    }
}
