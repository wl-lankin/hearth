//! End-to-end tests of PHP installs against a local server serving a test-signed catalog.

use std::io::Write;
use std::time::{Duration, Instant};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::service::*;
use super::store::{self, install_dir};
use crate::ipc::{Catalog, CoreState};
use crate::jobs::{JobId, JobState, JobStatus};
use crate::php::manifest::{Arch, Os};
use crate::HearthPaths;

const TEST_PUBLIC_KEY: &str = include_str!("../../tests/fixtures/test-catalog.pub");
const TEST_SECRET_KEY: &str = include_str!("../../tests/fixtures/test-catalog.key");

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Signs like `tauri signer sign` does: minisign, then base64 of the signature text.
fn sign(data: &[u8]) -> Vec<u8> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let key_text = String::from_utf8(b64.decode(TEST_SECRET_KEY.trim()).unwrap()).unwrap();
    let sk = minisign::SecretKeyBox::from_string(&key_text).unwrap().into_secret_key(Some("testpw".into())).unwrap();
    let sig = minisign::sign(None, &sk, data, None, None).unwrap();
    b64.encode(sig.to_string()).into_bytes()
}

fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(&mut buf);
    for (name, body) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(body).unwrap();
    }
    w.finish().unwrap();
    buf.into_inner()
}

fn tar_gz_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut b = tar::Builder::new(gz);
    for (name, body) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_size(body.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        b.append_data(&mut h, name, *body).unwrap();
    }
    b.into_inner().unwrap().finish().unwrap()
}

pub(crate) struct Fixture {
    _dir: tempfile::TempDir,
    _server: std::sync::Arc<tiny_http::Server>,
    pub(crate) core: CoreState,
    /// Requests per path, to prove what was (not) downloaded.
    pub(crate) hits: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, usize>>>,
}

/// A served catalog with PHP 8.4.27 and 8.3.36 for this OS. `damage_php` serves bytes that do not
/// match the hash.
pub(crate) fn fixture(damage_php: bool) -> Fixture {
    fixture_with(damage_php, true)
}

/// Like `fixture`; without `serve_composer` the Composer download answers 404.
pub(crate) fn fixture_with(damage_php: bool, serve_composer: bool) -> Fixture {
    let (php, format, root, extensions_src) = match Os::current() {
        Os::Windows => (
            zip_with(&[("php.exe", b"exe"), ("ext/php_curl.dll", b"dll")]),
            "zip",
            "",
            vec![("redis", "dll", "php_redis.dll"), ("xdebug", "zend_dll", "php_xdebug.dll")],
        ),
        Os::Macos => (
            tar_gz_with(&[("php-8.4.27/bin/php", b"bin"), ("php-8.4.27/lib/xdebug.so", b"so")]),
            "tar.gz",
            "php-8.4.27",
            vec![],
        ),
    };
    let redis = zip_with(&[("php_redis.dll", b"redis")]);
    let xdebug = zip_with(&[("php_xdebug.dll", b"xdebug")]);
    let composer = b"<?php // composer".to_vec();
    let cacert = b"-----BEGIN CERTIFICATE-----".to_vec();
    let (caddy, caddy_format, caddy_binary) = match Os::current() {
        Os::Windows => (zip_with(&[("caddy.exe", b"caddy"), ("LICENSE", b"Apache")]), "zip", "caddy.exe"),
        Os::Macos => (tar_gz_with(&[("caddy", b"caddy"), ("LICENSE", b"Apache")]), "tar.gz", "caddy"),
    };

    // The server's address is only known once it runs, so the catalog is served from a slot filled below.
    let catalog_slot: std::sync::Arc<std::sync::Mutex<(Vec<u8>, Vec<u8>)>> = Default::default();
    let server = std::sync::Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let served_php = if damage_php { b"not the archive".to_vec() } else { php.clone() };
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("/php".into(), served_php),
        ("/redis.zip".into(), redis.clone()),
        ("/xdebug.zip".into(), xdebug.clone()),
        ("/cacert.pem".into(), cacert.clone()),
        ("/caddy".into(), caddy.clone()),
    ];
    if serve_composer {
        files.push(("/composer.phar".into(), composer.clone()));
    }
    let hits: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, usize>>> = Default::default();
    let (s, slot, counter) = (server.clone(), catalog_slot.clone(), hits.clone());
    std::thread::spawn(move || {
        for request in s.incoming_requests() {
            let url = request.url().to_string();
            *counter.lock().unwrap().entry(url.clone()).or_default() += 1;
            let body = match url.as_str() {
                "/manifest.json" => Some(slot.lock().unwrap().0.clone()),
                "/manifest.json.sig" => Some(slot.lock().unwrap().1.clone()),
                _ => files.iter().find(|(p, _)| *p == url).map(|(_, b)| b.clone()),
            };
            let _ = match body {
                Some(b) => request.respond(tiny_http::Response::from_data(b).with_chunked_threshold(usize::MAX)),
                None => request.respond(tiny_http::Response::empty(404)),
            };
        }
    });

    let mut extensions = serde_json::Map::new();
    for (name, kind, file) in extensions_src {
        let bytes = if name == "redis" { &redis } else { &xdebug };
        extensions.insert(
            name.into(),
            json!({ "kind": kind, "version": "1.0.0", "url": format!("{base}/{name}.zip"), "sha256": hex(bytes), "file": file }),
        );
    }
    if Os::current() == Os::Macos {
        extensions.insert("redis".into(), json!({ "kind": "builtin" }));
        extensions.insert("xdebug".into(), json!({ "kind": "zend_so", "file": "lib/xdebug.so" }));
    }
    let manifest = json!({
        "schema": 1,
        "generated": "2026-10-23T12:00:00Z",
        "php": [{
            "minor": "8.4", "version": "8.4.27", "channel": "stable",
            "os": Os::current(), "arch": Arch::current(),
            "url": format!("{base}/php"), "sha256": hex(&php), "size": php.len(),
            "format": format, "root": root, "extensions": extensions,
        }, {
            "minor": "8.3", "version": "8.3.36", "channel": "stable",
            "os": Os::current(), "arch": Arch::current(),
            "url": format!("{base}/php"), "sha256": hex(&php), "size": php.len(),
            "format": format, "root": root, "extensions": extensions,
        }],
        "composer": { "url": format!("{base}/composer.phar"), "sha256": hex(&composer), "version": "2.10.3" },
        "cacert": { "url": format!("{base}/cacert.pem"), "sha256": hex(&cacert) },
        "tools": { "caddy": [{
            "os": Os::current(), "arch": Arch::current(), "version": "2.11.7",
            "url": format!("{base}/caddy"), "sha256": hex(&caddy), "format": caddy_format, "binary": caddy_binary,
        }] },
    });
    let json = serde_json::to_vec(&manifest).unwrap();
    *catalog_slot.lock().unwrap() = (json.clone(), sign(&json));

    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    let mut core = CoreState::new(paths);
    core.catalog = Catalog { url: format!("{base}/manifest.json"), public_key: TEST_PUBLIC_KEY.trim().into() };
    Fixture { _dir: dir, _server: server, core, hits }
}

pub(crate) async fn wait(core: &CoreState, id: JobId) -> JobStatus {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let job = core.jobs.get(id).unwrap();
        if job.state != JobState::Running || Instant::now() > deadline {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn paths(f: &Fixture) -> &HearthPaths {
    &f.core.paths
}

fn shim(name: &str) -> String {
    if Os::current() == Os::Windows {
        format!("{name}.bat")
    } else {
        name.into()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn install_puts_php_ini_shims_and_composer_in_place() {
    let f = fixture(false);
    let job = wait(&f.core, install(&f.core, "8.4")).await;
    assert_eq!(job.state, JobState::Done, "{:?}", job.error);
    let p = paths(&f);
    assert_eq!(store::installed(p)[0].version, "8.4.27");
    assert!(install_dir(p, "8.4").join(BUILD_FILE).is_file());
    if Os::current() == Os::Windows {
        assert_eq!(std::fs::read(install_dir(p, "8.4").join("ext").join("php_redis.dll")).unwrap(), b"redis");
    }
    let ini = std::fs::read_to_string(p.config.join("php").join("8.4").join("php.ini")).unwrap();
    assert!(ini.contains("memory_limit = 512M"));
    assert!(!ini.contains("xdebug.mode"), "{ini}");
    assert!(p.bin.join(shim("php")).is_file());
    assert!(p.bin.join(shim("php84")).is_file());
    assert!(p.bin.join("composer.phar").is_file());
    assert!(p.data.join("php").join("cacert.pem").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(p.data.join("php").join("downloads")).unwrap().collect();
    assert!(leftovers.is_empty(), "downloads are cleaned up");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_install_of_the_same_version_joins_the_running_job() {
    let f = fixture(false);
    let a = install(&f.core, "8.4");
    let b = install(&f.core, "8.4");
    assert_eq!(a, b);
    assert_eq!(wait(&f.core, a).await.state, JobState::Done);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_download_fails_the_job_and_installs_nothing() {
    let f = fixture(true);
    let job = wait(&f.core, install(&f.core, "8.4")).await;
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.error.as_deref(), Some("The download of PHP 8.4.27 is damaged; try again"));
    assert!(!install_dir(paths(&f), "8.4").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_version_missing_from_the_catalog_fails_clearly() {
    let f = fixture(false);
    let job = wait(&f.core, install(&f.core, "9.9")).await;
    assert_eq!(job.error.as_deref(), Some("PHP 9.9 is not available for this computer"));
}

#[tokio::test(flavor = "multi_thread")]
async fn xdebug_on_rewrites_php_ini_and_user_settings_survive() {
    let f = fixture(false);
    wait(&f.core, install(&f.core, "8.4")).await;
    let p = paths(&f);
    set_ini_user(&f.core, "8.4", "memory_limit = 1G\n").unwrap();
    set_extension(&f.core, "8.4", "xdebug", true).unwrap();
    let ini = std::fs::read_to_string(p.config.join("php").join("8.4").join("php.ini")).unwrap();
    assert!(ini.contains("xdebug.mode = debug,develop"), "{ini}");
    assert!(ini.ends_with("memory_limit = 1G\n"), "{ini}");
    let (generated, user) = ini_parts(p, "8.4").unwrap();
    assert!(generated.contains("xdebug.start_with_request = trigger"));
    assert_eq!(user, "memory_limit = 1G\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn uninstall_removes_the_version_but_keeps_user_ini() {
    let f = fixture(false);
    wait(&f.core, install(&f.core, "8.4")).await;
    let p = paths(&f);
    set_ini_user(&f.core, "8.4", "memory_limit = 1G\n").unwrap();
    assert_eq!(uninstall(&f.core, "8.4").unwrap(), None);
    assert!(!install_dir(p, "8.4").exists());
    assert!(p.config.join("php").join("8.4").join("user.ini").is_file());
    assert!(!p.bin.join(shim("php")).exists());
    assert!(!p.bin.join(shim("php84")).exists());
    assert_eq!(uninstall(&f.core, "8.4").unwrap_err().to_string(), "PHP 8.4 is not installed");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_laravel_installer_needs_a_php_first() {
    let f = fixture(false);
    let job = wait(&f.core, install_laravel_installer(&f.core)).await;
    assert_eq!(job.error.as_deref(), Some("Install a PHP version first"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_last_step_leaves_no_half_registered_version() {
    let f = fixture_with(false, false);
    let job = wait(&f.core, install(&f.core, "8.4")).await;
    assert_eq!(job.state, JobState::Failed);
    assert!(job.error.as_deref().unwrap_or_default().starts_with("Could not download"), "{:?}", job.error);
    let p = paths(&f);
    assert!(store::installed(p).is_empty(), "the version must not count as installed");
    assert!(!install_dir(p, "8.4").exists());
    let leftovers: Vec<_> = std::fs::read_dir(p.data.join("php").join("downloads")).unwrap().collect();
    assert!(leftovers.is_empty(), "downloads are cleaned up after a failure too");
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_version_while_it_installs_is_refused() {
    let f = fixture(false);
    let id = install(&f.core, "8.4");
    let err = uninstall(&f.core, "8.4").unwrap_err();
    assert_eq!(err.to_string(), "PHP 8.4 is being installed; try again when it is done");
    assert_eq!(wait(&f.core, id).await.state, JobState::Done);
}

#[tokio::test(flavor = "multi_thread")]
async fn two_versions_installed_at_once_are_both_fully_registered() {
    let f = fixture(false);
    let (a, b) = (install(&f.core, "8.4"), install(&f.core, "8.3"));
    assert_eq!(wait(&f.core, a).await.error, None);
    assert_eq!(wait(&f.core, b).await.error, None);
    let p = paths(&f);
    let state = store::load(p);
    assert!(state.extensions.contains_key("8.4") && state.extensions.contains_key("8.3"), "{state:?}");
    assert!(p.bin.join(shim("php84")).is_file() && p.bin.join(shim("php83")).is_file());
    assert!(p.config.join("php").join("8.3").join("php.ini").is_file());
}
