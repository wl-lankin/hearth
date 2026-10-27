use std::net::TcpListener;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError, INVALID_PARAMS};
use crate::HearthPaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

fn fake_service() -> PathBuf {
    let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    deps.parent().unwrap().join("examples").join(format!("fake_service{}", std::env::consts::EXE_SUFFIX))
}

fn home() -> (tempfile::TempDir, CoreState) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    // An installed Redis 8 (the fake service), so nothing is downloaded.
    let program = paths
        .data
        .join("services")
        .join("bin")
        .join("redis")
        .join("8.10.3")
        .join(format!("redis-server{}", std::env::consts::EXE_SUFFIX));
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::copy(fake_service(), &program).unwrap();
    (d, CoreState::new(paths))
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[tokio::test(flavor = "multi_thread")]
async fn create_start_stop_env_and_delete_a_service() {
    let _one = super::PROCESS_TESTS.lock().await;
    let (_d, core) = home();
    let port = free_port();
    let created = call(&core, "services.create", json!({ "kind": "redis", "line": "8", "port": port })).await.unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(id, "redis");
    call(&core, "services.start", json!({ "id": id })).await.unwrap();
    let list = call(&core, "services.list", Value::Null).await.unwrap();
    assert_eq!((list[0]["running"].clone(), list[0]["port"].clone()), (json!(true), json!(port)));
    let env = call(&core, "services.env", json!({ "id": id })).await.unwrap();
    assert!(env.as_str().unwrap().contains(&format!("REDIS_PORT={port}")), "{env}");
    call(&core, "services.autostart", json!({ "id": id, "on": true })).await.unwrap();
    call(&core, "services.stop", json!({ "id": id })).await.unwrap();
    let list = call(&core, "services.list", Value::Null).await.unwrap();
    assert_eq!((list[0]["running"].clone(), list[0]["autostart"].clone()), (json!(false), json!(true)));
    call(&core, "services.delete", json!({ "id": id })).await.unwrap();
    assert_eq!(call(&core, "services.list", Value::Null).await.unwrap(), json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_taken_port_is_refused_at_create_with_a_suggestion() {
    let (_d, core) = home();
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let err = call(&core, "services.create", json!({ "kind": "redis", "line": "8", "port": port })).await.unwrap_err();
    assert!(err.message.starts_with(&format!("Port {port} is used by another program (")), "{}", err.message);
    let suggested = call(&core, "services.suggestPort", json!({ "kind": "redis" })).await.unwrap();
    assert!(suggested.as_u64().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_params_are_refused() {
    let (_d, core) = home();
    for (method, params) in [
        ("services.create", json!({ "line": "8" })),
        ("services.start", json!({})),
        ("services.port", json!({ "id": "redis", "port": "x" })),
        ("services.autostart", json!({ "id": "redis" })),
    ] {
        assert_eq!(call(&core, method, params).await.unwrap_err().code, INVALID_PARAMS, "{method}");
    }
    let err = call(&core, "services.start", json!({ "id": "nope" })).await.unwrap_err();
    assert_eq!(err.message, "No service named nope");
}
