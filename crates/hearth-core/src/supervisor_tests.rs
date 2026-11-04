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
        guard_port: None,
        raw_args: false,
        restart_on_clean_exit: true,
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

#[test]
fn a_clean_exit_is_restarted_at_once() {
    // php-cgi exits with code 0 after PHP_FCGI_MAX_REQUESTS: routine, not a crash, so no back-off.
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let port = free_port();
    // Long enough for a slow CI machine to see it ready before it exits.
    let mut s = spec(&dir, "php", port, Some(400));
    s.args.push("0".into());
    sup.ensure(s).unwrap();
    let started = Instant::now();
    eventually("two quick restarts", || sup.status()[0].restarts >= 2);
    // With the back-off, two restarts take at least 1 s + 2 s; the margin absorbs a busy machine.
    assert!(started.elapsed() < Duration::from_millis(2500), "took {:?}", started.elapsed());
    sup.stop_all();
}

/// macOS has no job object: after a Hearth crash, its children keep running. The next supervisor
/// for the same home stops the ones it recorded (still running the same program).
#[cfg(unix)]
#[test]
fn children_left_by_a_crash_are_stopped_on_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().join("run");
    std::fs::create_dir_all(&run).unwrap();
    let port = free_port();
    {
        let sup = Supervisor::new_in(Some(run.clone())).unwrap();
        sup.ensure(spec(&dir, "web", port, None)).unwrap();
        sup.forget_without_stopping();
    }
    std::thread::sleep(Duration::from_millis(300));
    assert!(answers(port), "the child outlived its supervisor, as after a crash");
    let _next = Supervisor::new_in(Some(run)).unwrap();
    eventually("the leftover child is stopped", || !answers(port));
}

#[test]
fn a_crashed_process_whose_port_was_taken_is_parked_not_restarted() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let port = free_port();
    let mut s = spec(&dir, "service-db", port, Some(400));
    s.guard_port = Some(port);
    sup.ensure(s).unwrap();
    // Once it has crashed, another program takes its port.
    eventually("the crash", || !answers(port) && !sup.status()[0].running);
    let _other = TcpListener::bind(("127.0.0.1", port)).unwrap();
    std::thread::sleep(Duration::from_millis(2500));
    let status = &sup.status()[0];
    assert_eq!(status.restarts, 0, "no restart onto a taken port");
    assert_eq!(status.last_error.as_deref(), Some(format!("Port {port} is used by another program").as_str()));
    sup.stop_all();
}

/// A shell and the program it started (as `npm run dev` starts node): stopping the shell's process
/// must end the program too, or it keeps its port after Stop.
fn shell_spec(dir: &tempfile::TempDir, key: &str, port: u16) -> ProcessSpec {
    let (program, args): (PathBuf, Vec<std::ffi::OsString>) = if cfg!(windows) {
        ("cmd.exe".into(), vec!["/d".into(), "/s".into(), "/c".into(), format!("{} {port}", helper().display()).into()])
    } else {
        ("/bin/sh".into(), vec!["-c".into(), format!("'{}' {port}; true", helper().display()).into()])
    };
    ProcessSpec {
        key: key.into(),
        program,
        args,
        env: vec![],
        cwd: None,
        log: dir.path().join(format!("{key}.log")),
        ready_port: Some(port),
        ready_socket: None,
        guard_port: None,
        raw_args: false,
        restart_on_clean_exit: true,
    }
}

#[test]
fn stopping_a_shell_stops_what_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let port = free_port();
    sup.ensure(shell_spec(&dir, "dev", port)).unwrap();
    assert!(answers(port));
    sup.stop("dev");
    eventually("the program the shell started stops", || !answers(port));
}

#[test]
fn stop_all_stops_what_shells_started() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let port = free_port();
    sup.ensure(shell_spec(&dir, "dev", port)).unwrap();
    sup.stop_all();
    eventually("the program the shell started stops", || !answers(port));
}

/// A shell running `script` (no port): for processes that finish or crash on their own.
fn script_spec(dir: &tempfile::TempDir, key: &str, script: &str) -> ProcessSpec {
    let (program, args): (PathBuf, Vec<std::ffi::OsString>) = if cfg!(windows) {
        ("cmd.exe".into(), vec!["/d".into(), "/s".into(), "/c".into(), format!("\"{script}\"").into()])
    } else {
        ("/bin/sh".into(), vec!["-c".into(), script.into()])
    };
    ProcessSpec {
        key: key.into(),
        program,
        args,
        env: vec![],
        cwd: None,
        log: dir.path().join(format!("{key}.log")),
        ready_port: None,
        ready_socket: None,
        guard_port: None,
        raw_args: cfg!(windows),
        restart_on_clean_exit: false,
    }
}

/// A user's command that finishes (`npm run build`, `migrate:fresh --seed`) runs once: run again
/// and again, it would burn the CPU and seed the database over and over.
#[test]
fn a_command_that_finishes_is_not_run_again() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    sup.ensure(script_spec(&dir, "build", "echo built")).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let s = &sup.status()[0];
    assert_eq!((s.running, s.waiting, s.restarts, s.last_error.clone()), (false, false, 0, None));
    let log = std::fs::read_to_string(dir.path().join("build.log")).unwrap();
    assert_eq!(log.matches("built").count(), 1, "{log}");
}

/// A crashed command waits for its next try: the page must still offer Stop then.
#[test]
fn a_crashed_command_is_reported_waiting_for_its_restart() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    sup.ensure(script_spec(&dir, "dev", "echo broken && exit 3")).unwrap();
    eventually("the crash is noticed", || sup.status()[0].waiting);
    let s = &sup.status()[0];
    assert!(!s.running);
    assert!(s.last_error.as_deref().is_some_and(|e| e.contains("broken")), "{s:?}");
    sup.stop("dev");
    assert!(sup.status().is_empty());
}

/// The reason in `last_error` comes from the end of a long log, also with non-UTF-8 bytes in it.
#[test]
fn the_log_tail_reads_only_the_end_and_survives_bad_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("x.log");
    let mut bytes = "noise\n".repeat(400_000).into_bytes();
    bytes.extend_from_slice(b"caf\xe9 broken\nreason here\n");
    std::fs::write(&log, bytes).unwrap();
    let tail = super::supervisor::log_tail(&log);
    assert!(tail.ends_with("reason here"), "{tail}");
    assert!(tail.contains("broken"), "{tail}");
}

/// macOS: a crashed shell's surviving program is stopped before the restart, or it keeps its port.
#[cfg(unix)]
#[test]
fn a_crashed_shell_takes_its_programs_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new().unwrap();
    let port = free_port();
    let script = format!("'{}' {port} & sleep 1; exit 3", helper().display());
    sup.ensure(script_spec(&dir, "dev", &script)).unwrap();
    eventually("the program listens", || answers(port));
    eventually("the crash is noticed", || sup.status()[0].waiting);
    eventually("the program is stopped with the shell", || !answers(port));
    sup.stop_all();
}
