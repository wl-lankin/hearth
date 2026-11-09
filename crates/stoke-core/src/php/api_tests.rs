//! The php.* and jobs.* IPC methods, through the real router.

use serde_json::{json, Value};

use super::service_tests::{fixture, wait};
use crate::ipc::{router, RpcError, INVALID_PARAMS};

async fn call(core: &crate::ipc::CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

#[tokio::test(flavor = "multi_thread")]
async fn list_is_empty_before_any_install() {
    let f = fixture(false);
    assert_eq!(call(&f.core, "php.list", Value::Null).await.unwrap(), json!({ "installed": [], "default": null }));
}

#[tokio::test(flavor = "multi_thread")]
async fn install_through_ipc_then_list_available_and_switch() {
    let f = fixture(false);
    let started = call(&f.core, "php.install", json!({ "minor": "8.4" })).await.unwrap();
    let id = started["jobId"].as_u64().unwrap();
    assert_eq!(wait(&f.core, id).await.error, None);

    let job = call(&f.core, "jobs.get", json!({ "id": id })).await.unwrap();
    assert_eq!(job["state"], "done");

    let list = call(&f.core, "php.list", Value::Null).await.unwrap();
    assert_eq!(list["default"], "8.4");
    assert_eq!(list["installed"][0]["version"], "8.4.27");
    assert_eq!(list["installed"][0]["extensions"]["redis"], true);
    assert_eq!(list["installed"][0]["extensions"]["xdebug"], false);

    let available = call(&f.core, "php.available", Value::Null).await.unwrap();
    assert_eq!(available[0]["minor"], "8.4");
    assert_eq!(available[0]["installedVersion"], "8.4.27");
    assert_eq!(available[0]["updateAvailable"], false);

    call(&f.core, "php.extension", json!({ "minor": "8.4", "name": "xdebug", "on": true })).await.unwrap();
    let ini = call(&f.core, "php.ini.get", json!({ "minor": "8.4" })).await.unwrap();
    assert!(ini["generated"].as_str().unwrap().contains("xdebug.mode"));
    call(&f.core, "php.ini.set", json!({ "minor": "8.4", "user": "memory_limit = 1G\n" })).await.unwrap();
    assert_eq!(call(&f.core, "php.ini.get", json!({ "minor": "8.4" })).await.unwrap()["user"], "memory_limit = 1G\n");

    assert_eq!(call(&f.core, "php.uninstall", json!({ "minor": "8.4" })).await.unwrap(), json!({ "default": null }));
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_params_are_invalid_params() {
    let f = fixture(false);
    for (method, params) in [
        ("php.install", json!({})),
        ("php.use", json!({ "minor": 84 })),
        ("php.extension", json!({ "minor": "8.4", "name": "xdebug" })),
        ("jobs.get", json!({ "id": "x" })),
        ("jobs.get", json!({ "id": 999 })),
    ] {
        let err = call(&f.core, method, params).await.unwrap_err();
        assert_eq!(err.code, INVALID_PARAMS, "{method}: {}", err.message);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn php_errors_carry_the_user_message() {
    let f = fixture(false);
    let err = call(&f.core, "php.use", json!({ "minor": "9.9" })).await.unwrap_err();
    assert_eq!(err.message, "PHP 9.9 is not installed");
}
