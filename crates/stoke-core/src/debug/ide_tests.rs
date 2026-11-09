use crate::debug::ide::listener;

fn this_program() -> String {
    std::env::current_exe().unwrap().file_stem().unwrap().to_string_lossy().into_owned()
}

#[test]
fn finds_this_test_listening() {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let found = listener(port).expect("our own listener");
    assert_eq!(found.pid, std::process::id());
    assert_eq!(found.program.as_deref(), Some(this_program().as_str()));
}

#[test]
fn finds_an_ipv6_listener() {
    let Ok(socket) = std::net::TcpListener::bind("[::1]:0") else {
        return eprintln!("skipped: no IPv6 loopback");
    };
    let port = socket.local_addr().unwrap().port();
    assert_eq!(listener(port).map(|l| l.pid), Some(std::process::id()));
}

#[test]
fn nothing_listening_is_none() {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(listener(port).is_none());
}
