use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::supervisor::{ProcessSpec, Supervisor};

/// The `listen` example, built by `cargo test` next to the test binaries.
fn helper() -> PathBuf {
    let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    let name = if cfg!(windows) { "listen.exe" } else { "listen" };
    deps.parent().unwrap().join("examples").join(name)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn spec(dir: &tempfile::TempDir, key: &str, port: u16, crash_after_ms: Option<u64>) -> ProcessSpec {
    let mut args = vec![port.to_string().into()];
    if let Some(ms) = crash_after_ms {
        args.push(ms.to_string().into());
    }
    ProcessSpec {
        key: key.into(),
        program: helper(),
        args,
        env: vec![],
        cwd: None,
        log: dir.path().join(format!("{key}.log")),
        ready_port: Some(port),
        ready_socket: None,
    }
}

fn answers(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok()
}

fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_process_starts_and_is_reported_running() {
    assert!(helper().is_file(), "build the example first: {}", helper().display());
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let port = free_port();
    sup.ensure(spec(&dir, "web", port, None)).unwrap();
    assert!(answers(port));
    let status = sup.status();
    assert_eq!(status.len(), 1);
    assert!(status[0].running && status[0].pid.is_some());
    sup.stop_all();
}

#[test]
fn a_taken_port_fails_with_a_clear_message() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let err = sup.ensure(spec(&dir, "web", port, None)).unwrap_err();
    assert_eq!(err.to_string(), format!("Port {port} is used by another program"));
}

#[test]
fn a_crashed_process_is_restarted() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let port = free_port();
    sup.ensure(spec(&dir, "php", port, Some(400))).unwrap();
    eventually("a restart after the crash", || sup.status()[0].restarts >= 1);
    eventually("the restarted process answers", || answers(port));
    assert!(sup.status()[0].last_error.as_deref().unwrap_or_default().contains("exited"));
    sup.stop_all();
}

#[test]
fn stop_all_leaves_nothing_running() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let (a, b) = (free_port(), free_port());
    sup.ensure(spec(&dir, "a", a, None)).unwrap();
    sup.ensure(spec(&dir, "b", b, None)).unwrap();
    sup.stop_all();
    eventually("both ports are free", || !answers(a) && !answers(b));
    assert!(sup.status().is_empty());
}

#[test]
fn ensuring_a_changed_spec_restarts_the_process() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let (a, b) = (free_port(), free_port());
    sup.ensure(spec(&dir, "web", a, None)).unwrap();
    let first = sup.status()[0].pid;
    sup.ensure(spec(&dir, "web", a, None)).unwrap();
    assert_eq!(sup.status()[0].pid, first, "same spec: keeps the running process");
    sup.ensure(spec(&dir, "web", b, None)).unwrap();
    assert_ne!(sup.status()[0].pid, first);
    eventually("the old port is free", || !answers(a));
    assert!(answers(b));
    sup.stop_all();
}

#[cfg(windows)]
#[test]
fn children_die_with_the_supervisor() {
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    {
        let sup = Supervisor::new().unwrap();
        sup.ensure(spec(&dir, "web", port, None)).unwrap();
        // Simulates Hearth disappearing: the job object handle closes without stop_all.
        sup.forget_without_stopping();
    }
    eventually("the child is gone with the job object", || !answers(port));
}
