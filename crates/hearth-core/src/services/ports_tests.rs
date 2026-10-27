use std::net::TcpListener;

use super::ports::{check, port_free, suggest_from};
use super::store;
use crate::HearthPaths;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, paths)
}

/// A port nothing listens on (bound and released).
fn free() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[test]
fn a_port_another_program_listens_on_is_taken() {
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    assert!(!port_free(port));
    // (Whether it is free again after `drop` is not checked: a test running alongside may take it.)
    drop(taken);
}

#[test]
fn the_suggestion_skips_taken_ports_and_other_instances() {
    let (_d, paths) = home();
    // Held from the start: a port released and bound again can be taken by a test running alongside.
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let start = taken.local_addr().unwrap().port();
    // The next port belongs to another Hearth instance (not running, still reserved).
    store::create(&paths, "redis", "8", start + 1, None).unwrap();
    let suggested = suggest_from(&paths, start, 1);
    assert!(suggested >= start + 2, "start {start} suggested {suggested}");
    drop(taken);
}

#[test]
fn silo_needs_two_free_ports() {
    let (_d, paths) = home();
    let start = free();
    let console = TcpListener::bind(("127.0.0.1", start + 1));
    if let Ok(console) = console {
        let suggested = suggest_from(&paths, start, 2);
        assert!(suggested != start, "the console port next to {start} is taken");
        drop(console);
    }
}

#[test]
fn a_taken_port_is_explained() {
    let (_d, paths) = home();
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let err = check(&paths, &[port], None).unwrap_err().to_string();
    assert!(err.starts_with(&format!("Port {port} is used by another program (")), "{err}");
    let other = store::create(&paths, "mysql", "8.4", free(), None).unwrap();
    let err = check(&paths, &[other.port], None).unwrap_err().to_string();
    assert_eq!(err, format!("Port {} is used by MySQL 8.4", other.port));
    assert!(check(&paths, &[other.port], Some(&other.id)).is_ok(), "an instance does not block itself");
}
