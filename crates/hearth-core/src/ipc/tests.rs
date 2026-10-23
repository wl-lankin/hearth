use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::*;
use crate::{CoreError, HearthPaths};

fn home() -> (tempfile::TempDir, HearthPaths) {
    home_named("hearth")
}

fn home_named(name: &str) -> (tempfile::TempDir, HearthPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join(name));
    paths.ensure().unwrap();
    (dir, paths)
}

async fn start(paths: &HearthPaths) -> ServerHandle {
    let state = CoreState { paths: paths.clone(), started: Instant::now() };
    serve(paths, router(state)).await.unwrap()
}

#[tokio::test]
async fn ping_roundtrip() {
    let (_d, paths) = home();
    let server = start(&paths).await;
    assert_eq!(call(&paths, "core.ping", Value::Null).await.unwrap(), json!("pong"));
    server.shutdown().await;
}

#[tokio::test]
async fn status_reports_version_pid_home() {
    let (_d, paths) = home();
    let server = start(&paths).await;
    let status = call(&paths, "core.status", Value::Null).await.unwrap();
    assert_eq!(status["version"], json!(env!("CARGO_PKG_VERSION")));
    assert_eq!(status["pid"], json!(std::process::id()));
    assert!(status["uptimeSecs"].is_u64());
    assert_eq!(status["home"], json!(paths.root.to_string_lossy()));
    server.shutdown().await;
}

#[tokio::test]
async fn unknown_method_is_32601() {
    let (_d, paths) = home();
    let server = start(&paths).await;
    match call(&paths, "nope.nothing", Value::Null).await {
        Err(CoreError::Rpc { code, .. }) => assert_eq!(code, METHOD_NOT_FOUND),
        other => panic!("expected an RPC error, got {other:?}"),
    }
    server.shutdown().await;
}

#[tokio::test]
async fn malformed_line_gets_parse_error_and_connection_survives() {
    let (_d, paths) = home();
    let server = start(&paths).await;
    let stream = endpoint::connect(&paths).await.unwrap();
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();

    write.write_all(b"{oops\n").await.unwrap();
    let reply: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(reply["error"]["code"], json!(PARSE_ERROR));

    write.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"core.ping\",\"params\":null}\n").await.unwrap();
    let reply: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(reply["id"], json!(7));
    assert_eq!(reply["result"], json!("pong"));
    server.shutdown().await;
}

#[tokio::test]
async fn second_server_same_home_is_already_running() {
    let (_d, paths) = home();
    let server = start(&paths).await;
    let state = CoreState { paths: paths.clone(), started: Instant::now() };
    assert!(matches!(serve(&paths, router(state)).await, Err(CoreError::AlreadyRunning)));
    server.shutdown().await;
}

#[tokio::test]
async fn two_homes_do_not_collide() {
    let (_a, first) = home_named("one");
    let (_b, second) = home_named("two");
    let s1 = start(&first).await;
    let s2 = start(&second).await;
    let h1 = call(&first, "core.status", Value::Null).await.unwrap();
    let h2 = call(&second, "core.status", Value::Null).await.unwrap();
    assert_eq!(h1["home"], json!(first.root.to_string_lossy()));
    assert_eq!(h2["home"], json!(second.root.to_string_lossy()));
    s1.shutdown().await;
    s2.shutdown().await;
}

#[tokio::test]
async fn call_without_server_is_not_running() {
    let (_d, paths) = home();
    let started = Instant::now();
    assert!(matches!(call(&paths, "core.ping", Value::Null).await, Err(CoreError::NotRunning)));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn shutdown_stops_answering() {
    let (_d, paths) = home();
    let server = start(&paths).await;
    server.shutdown().await;
    assert!(matches!(call(&paths, "core.ping", Value::Null).await, Err(CoreError::NotRunning)));
}

#[tokio::test]
async fn unicode_home_with_spaces_works() {
    let (_d, paths) = home_named("Jörg Müller");
    let server = start(&paths).await;
    assert_eq!(call(&paths, "core.ping", Value::Null).await.unwrap(), json!("pong"));
    server.shutdown().await;
}

#[test]
fn endpoint_names_differ_per_home_and_are_safe() {
    let (_a, first) = home_named("one");
    let (_b, second) = home_named("two");
    let n1 = endpoint::endpoint_name(&first);
    assert_ne!(n1, endpoint::endpoint_name(&second));
    assert!(n1.starts_with("hearth-"));
    assert!(n1.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "{n1}");
}

#[cfg(unix)]
#[tokio::test]
async fn stale_socket_is_replaced() {
    let (_d, paths) = home();
    std::fs::write(paths.run.join("hearth.sock"), b"").unwrap();
    let server = start(&paths).await;
    assert_eq!(call(&paths, "core.ping", Value::Null).await.unwrap(), json!("pong"));
    server.shutdown().await;
}
