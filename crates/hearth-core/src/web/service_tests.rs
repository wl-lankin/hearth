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

/// The PHP pool ports are fixed per version (8.4 -> 19840..), so these tests run one at a time.
static PORTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Home {
    _ports: std::sync::MutexGuard<'static, ()>,
    _dir: tempfile::TempDir,
    core: CoreState,
    projects: PathBuf,
    port: u16,
}

fn home() -> Home {
    let ports = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    fake_php(&paths);
    let port = free_port();
    registry::set_http_port(&paths, port).unwrap();
    let projects = dir.path().join("Projects");
    laravel(&projects.join("shop"));
    Home { core: CoreState::new(paths), projects, port, _dir: dir, _ports: ports }
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
    match Os::current() {
        Os::Windows => assert!(answers(19840)),
        Os::Macos => assert!(h.core.paths.run.join("php-8.4.sock").exists()),
    }
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
    let pool = &crate::web::pools::pool_upstreams(&h.core.paths, Os::current(), "8.4")[0];
    assert!(caddyfile(&h).contains(pool.as_str()));
    h.core.supervisor.stop_all();
}

#[test]
fn sites_that_cannot_be_served_do_not_start_the_web_server() {
    let h = home();
    let empty = h.projects.join("..").join("Empty");
    std::fs::create_dir_all(empty.join("notes")).unwrap();
    registry::park(&h.core.paths, &empty).unwrap();
    let started = std::time::Instant::now();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(!status.running);
    assert_eq!(status.error, None, "nothing to serve is not an error");
    assert!(started.elapsed() < Duration::from_secs(2), "no 5 s wait for a Caddy without sites");
    assert!(h.core.supervisor.status().is_empty());
}

#[test]
fn a_web_server_that_dies_later_is_reported() {
    let h = home();
    registry::park(&h.core.paths, &h.projects).unwrap();
    let first = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(first.running);
    let pid = h.core.supervisor.status().into_iter().find(|p| p.key == "caddy").and_then(|p| p.pid).unwrap();
    kill(pid);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while h.core.supervisor.status().iter().any(|p| p.key == "caddy" && p.running) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    if !status.running {
        assert!(status.error.as_deref().unwrap_or_default().contains("caddy"), "{status:?}");
    }
    h.core.supervisor.stop_all();
}

fn kill(pid: u32) {
    #[cfg(windows)]
    std::process::Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output().unwrap();
    #[cfg(unix)]
    std::process::Command::new("kill").args(["-9", &pid.to_string()]).output().unwrap();
}

#[test]
fn nothing_is_synced_while_hearth_quits() {
    let mut h = home();
    h.core.web_sync = true;
    h.core.quitting.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(!super::service::schedule_sync(&h.core), "a sync while quitting could start Caddy again");
}

fn caddy_pid(h: &Home) -> Option<u32> {
    h.core.supervisor.status().into_iter().find(|p| p.key == "caddy").and_then(|p| p.pid)
}

#[test]
fn securing_a_site_writes_its_certificate_and_an_https_block() {
    let h = home();
    let https = free_port();
    registry::set_https_port(&h.core.paths, https).unwrap();
    registry::link(&h.core.paths, "shop", &h.projects.join("shop")).unwrap();
    registry::secure(&h.core.paths, "shop").unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert_eq!(status.error, None, "{status:?}");
    assert_eq!(status.https_port, https);
    assert!(!status.ca_trusted, "a test CA is never in the OS store");
    let (crt, key) = crate::tls::leaf_paths(&h.core.paths, "shop");
    assert!(crt.is_file() && key.is_file());
    let file = caddyfile(&h);
    assert!(file.contains(&format!("https://shop.test:{https} {{\n\ttls \"")), "{file}");
    h.core.supervisor.stop_all();
}

#[test]
fn a_deleted_certificate_is_issued_again_and_caddy_restarts_with_the_same_upstream() {
    let h = home();
    registry::link(&h.core.paths, "shop", &h.projects.join("shop")).unwrap();
    registry::proxy(&h.core.paths, "vite", "http://localhost:5173").unwrap();
    registry::secure(&h.core.paths, "vite").unwrap();
    sync_with(&h.core, &example("fake_server")).unwrap();
    let first = caddy_pid(&h).unwrap();
    let before = caddyfile(&h);
    let (crt, _) = crate::tls::leaf_paths(&h.core.paths, "vite");
    std::fs::remove_file(&crt).unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(status.running, "{status:?}");
    assert!(crt.is_file());
    assert_ne!(caddy_pid(&h).unwrap(), first, "Caddy reads certificates only when it starts");
    assert_eq!(caddyfile(&h), before);
    assert!(before.contains("reverse_proxy http://localhost:5173 {"));
    h.core.supervisor.stop_all();
}

#[test]
fn unsecuring_removes_the_certificate() {
    let h = home();
    registry::link(&h.core.paths, "shop", &h.projects.join("shop")).unwrap();
    registry::secure(&h.core.paths, "shop").unwrap();
    sync_with(&h.core, &example("fake_server")).unwrap();
    registry::unsecure(&h.core.paths, "shop").unwrap();
    sync_with(&h.core, &example("fake_server")).unwrap();
    let (crt, key) = crate::tls::leaf_paths(&h.core.paths, "shop");
    assert!(!crt.exists() && !key.exists());
    assert!(!caddyfile(&h).contains("https://"));
    h.core.supervisor.stop_all();
}

#[test]
fn sites_serve_without_name_resolution_and_the_status_says_why() {
    let mut h = home();
    h.core.dns_pipe = Some(format!("{}-missing-{}", crate::dns::protocol::PIPE, std::process::id()));
    registry::link(&h.core.paths, "shop", &h.projects.join("shop")).unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert!(status.running, "{status:?}");
    assert!(!status.dns.ready);
    let problem = status.dns.problem.unwrap_or_default();
    assert!(problem.starts_with("The .test helper is not ") || problem.contains("/etc/resolver/test"), "{problem}");
    h.core.supervisor.stop_all();
}

#[test]
fn names_are_left_alone_without_a_target() {
    let h = home();
    registry::link(&h.core.paths, "shop", &h.projects.join("shop")).unwrap();
    let status = sync_with(&h.core, &example("fake_server")).unwrap();
    assert_eq!(status.dns, crate::dns::DnsStatus { ready: false, problem: None });
    h.core.supervisor.stop_all();
}

#[test]
fn a_stopped_web_server_stays_stopped_until_started() {
    let mut h = home();
    h.core.web_sync = true;
    h.core.web_paused.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(!super::service::schedule_sync(&h.core), "the 30 s loop must not start Caddy again after `stop`");
}
