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
    // Requests keep the script loaded; with them off too, dumps off removes it.
    let config = crate::insights::collector::CollectorConfig {
        requests: false,
        ..crate::insights::collector::config(&core.paths)
    };
    crate::insights::collector::set_config(&core.paths, &config).unwrap();
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

#[tokio::test]
async fn list_get_and_clear_requests() {
    let (_d, core) = home();
    let port = free_port();
    let status = call(&core, "dumps.port", json!({ "port": port })).await.unwrap();
    assert_eq!(status["requests"], json!(true));
    let line = json!({"type": "request", "site": "shop.test", "sapi": "fpm-fcgi", "method": "GET", "uri": "/cart",
        "status": 200, "ms": 12.0, "memory": 1, "dropped": 0,
        "queries": [{"sql": "select 1", "bindings": [], "ms": 150.0, "start": 1.0, "connection": "sqlite", "file": "/a.php", "line": 3}]});
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(format!("{line}\n").as_bytes()).await.unwrap();
    s.shutdown().await.unwrap();
    let mut list = Value::Null;
    for _ in 0..100 {
        list = call(&core, "requests.list", json!({})).await.unwrap();
        if list.as_array().is_some_and(|a| !a.is_empty()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!((list[0]["uri"].clone(), list[0]["findings"].clone()), (json!("/cart"), json!(["slow"])));
    assert!(list[0].get("queries").is_none(), "the list leaves the queries out");
    let id = list[0]["id"].clone();
    assert_eq!(call(&core, "requests.list", json!({ "after": id })).await.unwrap(), json!([]));
    let full = call(&core, "requests.get", json!({ "id": id })).await.unwrap();
    assert_eq!(full["queries"][0]["sql"], json!("select 1"));
    assert!(call(&core, "requests.get", json!({ "id": 999 })).await.is_err());
    call(&core, "requests.clear", json!({})).await.unwrap();
    assert_eq!(call(&core, "requests.list", json!({})).await.unwrap(), json!([]));
    core.collector.stop();
}

#[tokio::test]
async fn requests_off_and_dumps_off_removes_the_prepend_line() {
    let (_d, core) = home();
    fake_php(&core.paths);
    crate::php::service::apply_config(&core.paths, "8.4").unwrap();
    call(&core, "dumps.enable", json!({ "on": false })).await.unwrap();
    assert!(php_ini(&core.paths).contains("auto_prepend_file"), "requests still need the script");
    let status = call(&core, "requests.enable", json!({ "on": false })).await.unwrap();
    assert_eq!((status["requests"].clone(), status["enabled"].clone()), (json!(false), json!(false)));
    assert!(!php_ini(&core.paths).contains("auto_prepend_file"));
    call(&core, "requests.enable", json!({ "on": true })).await.unwrap();
    assert!(php_ini(&core.paths).contains("auto_prepend_file"));
    let script = std::fs::read_to_string(script_path(&core.paths)).unwrap();
    assert!(script.contains("if (false && !function_exists('hearth_dump_text'))"), "dumps stay off in the script");
}
