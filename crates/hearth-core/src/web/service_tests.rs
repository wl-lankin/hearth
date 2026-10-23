use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::service::sync_with;
use crate::ipc::CoreState;
use crate::php::manifest::Os;
use crate::php::service::BUILD_FILE;
use crate::php::store::{self, install_dir, VERSION_FILE};
use crate::sites::registry;
use crate::HearthPaths;

fn example(name: &str) -> PathBuf {
    let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    deps.parent().unwrap().join("examples").join(if cfg!(windows) { format!("{name}.exe") } else { name.into() })
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn answers(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok()
}

/// An installed PHP 8.4 whose FastCGI program is the fake server.
fn fake_php(paths: &HearthPaths) {
    let dir = install_dir(paths, "8.4");
    let program = match Os::current() {
        Os::Windows => dir.join("php-cgi.exe"),
        Os::Macos => dir.join("sbin").join("php-fpm"),
    };
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::copy(example("fake_server"), &program).unwrap();
    let build = json!({
        "minor": "8.4", "version": "8.4.27", "channel": "stable", "os": Os::current(), "arch": "x64",
        "url": "u", "sha256": "0".repeat(64), "format": "zip", "root": "", "extensions": {}
    });
    std::fs::write(dir.join(BUILD_FILE), build.to_string()).unwrap();
    std::fs::write(dir.join(VERSION_FILE), "8.4.27").unwrap();
    store::register(paths, "8.4", &[]).unwrap();
}

fn laravel(dir: &Path) {
    std::fs::create_dir_all(dir.join("public")).unwrap();
    std::fs::write(dir.join("public").join("index.php"), "<?php").unwrap();
}

struct Home {
    _dir: tempfile::TempDir,
    core: CoreState,
    projects: PathBuf,
    port: u16,
}

fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    fake_php(&paths);
    let port = free_port();
    registry::set_http_port(&paths, port).unwrap();
    let projects = dir.path().join("Projects");
    laravel(&projects.join("shop"));
    Home { core: CoreState::new(paths), projects, port, _dir: dir }
}

fn caddyfile(h: &Home) -> String {
    std::fs::read_to_string(h.core.paths.config.join("caddy").join("Caddyfile")).unwrap_or_default()
}

#[test]
fn nothing_is_started_without_sites() {
    let h = home();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(!status.running);
    assert!(h.core.supervisor.status().is_empty());
}

#[test]
fn parking_a_folder_starts_its_php_pool_and_the_web_server() {
    let h = home();
    registry::park(&h.core.paths, &h.projects).unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(status.running, "{status:?}");
    assert_eq!(status.error, None);
    assert!(answers(h.port));
    assert!(answers(19840));
    assert!(caddyfile(&h).contains(&format!("http://shop.test:{} {{", h.port)));
    h.core.supervisor.stop_all();
}

#[test]
fn unparking_stops_the_pool_and_the_web_server() {
    let h = home();
    registry::park(&h.core.paths, &h.projects).unwrap();
    sync_with(&h.core, &example("fake_server")).unwrap();
    registry::unpark(&h.core.paths, &h.projects).unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(!status.running);
    assert!(h.core.supervisor.status().is_empty(), "{:?}", h.core.supervisor.status());
}

#[test]
fn an_invalid_config_keeps_the_last_good_one() {
    let h = home();
    registry::park(&h.core.paths, &h.projects).unwrap();
    sync_with(&h.core, &example("fake_server")).unwrap();
    let good = caddyfile(&h);
    registry::proxy(&h.core.paths, "broken", "http://INVALID:1").unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(status.error.as_deref().unwrap_or_default().contains("invalid"), "{status:?}");
    assert_eq!(caddyfile(&h), good);
    assert!(status.running, "the old config keeps serving");
    h.core.supervisor.stop_all();
}

#[test]
fn a_taken_http_port_is_reported() {
    let h = home();
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    registry::set_http_port(&h.core.paths, port).unwrap();
    registry::park(&h.core.paths, &h.projects).unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(!status.running);
    assert_eq!(status.error.as_deref(), Some(format!("Port {port} is used by another program").as_str()));
    h.core.supervisor.stop_all();
}

#[test]
fn a_site_on_a_removed_version_is_served_by_the_default_with_a_note() {
    let h = home();
    registry::link(&h.core.paths, "shop", &h.projects.join("shop")).unwrap();
    registry::isolate(&h.core.paths, "shop", Some("8.3")).unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert_eq!(status.notes.get("shop").map(String::as_str), Some("Uses the default PHP 8.4 (8.3 was removed)"));
    assert!(caddyfile(&h).contains("127.0.0.1:19840"));
    h.core.supervisor.stop_all();
}
