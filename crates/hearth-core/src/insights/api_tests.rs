use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;

use crate::insights::prepend::script_path;
use crate::ipc::{router, CoreState, RpcError};
use crate::php::ini::ini_dir;
use crate::php::manifest::Os;
use crate::php::service::BUILD_FILE;
use crate::php::store::{self, install_dir, VERSION_FILE};
use crate::HearthPaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

fn home() -> (tempfile::TempDir, CoreState) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, CoreState::new(paths))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// An installed PHP 8.4 (no programs: only its php.ini is written here).
fn fake_php(paths: &HearthPaths) {
    let dir = install_dir(paths, "8.4");
    std::fs::create_dir_all(&dir).unwrap();
    let build = json!({
        "minor": "8.4", "version": "8.4.27", "channel": "stable", "os": Os::current(), "arch": "x64",
        "url": "u", "sha256": "0".repeat(64), "format": "zip", "root": "", "extensions": {}
    });
    std::fs::write(dir.join(BUILD_FILE), build.to_string()).unwrap();
    std::fs::write(dir.join(VERSION_FILE), "8.4.27").unwrap();
    store::register(paths, "8.4", &[]).unwrap();
}

fn php_ini(paths: &HearthPaths) -> String {
    std::fs::read_to_string(ini_dir(paths, "8.4").join("php.ini")).unwrap()
}

#[tokio::test]
async fn list_and_clear_through_the_core() {
    let (_d, core) = home();
    let port = free_port();
    let status = call(&core, "dumps.port", json!({ "port": port })).await.unwrap();
    assert_eq!((status["listening"].clone(), status["enabled"].clone()), (json!(true), json!(true)));
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(b"{\"type\":\"dump\",\"site\":\"shop\",\"text\":\"1\"}\n").await.unwrap();
    s.shutdown().await.unwrap();
    let mut list = Value::Null;
    for _ in 0..100 {
        list = call(&core, "dumps.list", json!({})).await.unwrap();
        if list.as_array().is_some_and(|a| !a.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!((list[0]["site"].clone(), list[0]["text"].clone()), (json!("shop"), json!("1")));
    let newest = list[0]["id"].clone();
    assert_eq!(call(&core, "dumps.list", json!({ "after": newest })).await.unwrap(), json!([]), "only newer ones");
    call(&core, "dumps.clear", json!({})).await.unwrap();
    assert_eq!(call(&core, "dumps.list", json!({})).await.unwrap(), json!([]));
    core.collector.stop();
}

#[tokio::test]
async fn enable_off_removes_the_prepend_line_from_every_installed_php() {
    let (_d, core) = home();
    fake_php(&core.paths);
    crate::php::service::apply_config(&core.paths, "8.4").unwrap();
    assert!(php_ini(&core.paths).contains("auto_prepend_file"), "on by default");
    assert!(script_path(&core.paths).is_file(), "php.ini never names a missing script");
    let status = call(&core, "dumps.enable", json!({ "on": false })).await.unwrap();
    assert_eq!(status["enabled"], json!(false));
    assert!(!php_ini(&core.paths).contains("auto_prepend_file"));
    call(&core, "dumps.enable", json!({ "on": true })).await.unwrap();
    assert!(php_ini(&core.paths).contains("auto_prepend_file"));
    assert!(call(&core, "dumps.enable", json!({})).await.is_err());
}

#[tokio::test]
async fn port_change_rewrites_the_prepend_and_a_taken_port_keeps_the_old_one() {
    let (_d, core) = home();
    let first = free_port();
    call(&core, "dumps.port", json!({ "port": first })).await.unwrap();
    let script = std::fs::read_to_string(script_path(&core.paths)).unwrap();
    assert!(script.contains(&format!("tcp://127.0.0.1:{first}")), "the script sends to the new port");

    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = blocker.local_addr().unwrap().port();
    let err = call(&core, "dumps.port", json!({ "port": taken })).await.unwrap_err();
    assert!(err.message.contains("is taken"), "{}", err.message);
    let status = call(&core, "dumps.status", json!({})).await.unwrap();
    assert_eq!((status["port"].clone(), status["listening"].clone()), (json!(first), json!(true)));
    let script = std::fs::read_to_string(script_path(&core.paths)).unwrap();
    assert!(script.contains(&format!("tcp://127.0.0.1:{first}")), "the script keeps the port that works");
    assert!(call(&core, "dumps.port", json!({ "port": 70000 })).await.is_err());
    core.collector.stop();
}

/// A linked site `shop` with `storage/logs/laravel.log`; returns the log file.
fn shop_log(d: &tempfile::TempDir, paths: &HearthPaths) -> std::path::PathBuf {
    let dir = d.path().join("shop");
    let logs = dir.join("storage").join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::create_dir_all(dir.join("public")).unwrap();
    crate::sites::registry::link(paths, "shop", &dir).unwrap();
    let file = logs.join("laravel.log");
    std::fs::write(&file, "[2026-11-03 10:00:00] local.INFO: one\n[2026-11-03 10:00:01] local.ERROR: two\n").unwrap();
    file
}

#[tokio::test]
async fn sites_read_and_clear_logs_through_the_core() {
    let (d, core) = home();
    let file = shop_log(&d, &core.paths);
    let sites = call(&core, "logs.sites", json!({})).await.unwrap();
    assert_eq!((sites[0]["site"].clone(), sites[0]["files"][0]["name"].clone()), (json!("shop"), json!("laravel.log")));
    let read = call(&core, "logs.read", json!({ "site": "shop", "file": "laravel.log" })).await.unwrap();
    assert_eq!(read["entries"][0]["message"], json!("two"));
    assert_eq!(read["size"], json!(std::fs::metadata(&file).unwrap().len()));
    assert!(call(&core, "logs.read", json!({ "site": "shop", "file": "../x.log" })).await.is_err());
    call(&core, "logs.clear", json!({ "site": "shop", "file": "laravel.log" })).await.unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().len(), 0);
}

#[tokio::test]
async fn an_unchanged_log_is_not_parsed_again() {
    let (d, core) = home();
    let file = shop_log(&d, &core.paths);
    let size = std::fs::metadata(&file).unwrap().len();
    let same = call(&core, "logs.read", json!({ "site": "shop", "file": "laravel.log", "size": size })).await.unwrap();
    assert_eq!(same, json!({ "size": size, "unchanged": true }));
    std::fs::write(&file, "[2026-11-03 10:00:02] local.INFO: three\n").unwrap();
    let changed =
        call(&core, "logs.read", json!({ "site": "shop", "file": "laravel.log", "size": size })).await.unwrap();
    assert_eq!(changed["entries"][0]["message"], json!("three"));
}
