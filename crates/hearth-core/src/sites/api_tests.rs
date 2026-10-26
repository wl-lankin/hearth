use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError, INVALID_PARAMS};
use crate::HearthPaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

fn home() -> (tempfile::TempDir, CoreState, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    let projects = d.path().join("Projects");
    std::fs::create_dir_all(projects.join("shop").join("public")).unwrap();
    std::fs::write(projects.join("shop").join("public").join("index.php"), "<?php").unwrap();
    (d, CoreState::new(paths), projects)
}

#[tokio::test(flavor = "multi_thread")]
async fn park_link_proxy_and_list() {
    let (_d, core, projects) = home();
    call(&core, "sites.park", json!({ "dir": projects })).await.unwrap();
    call(&core, "sites.link", json!({ "name": "api.shop", "dir": projects.join("shop") })).await.unwrap();
    call(&core, "sites.proxy", json!({ "name": "vite", "upstream": "http://localhost:5173" })).await.unwrap();
    call(&core, "sites.isolate", json!({ "name": "shop", "minor": "8.3" })).await.unwrap();
    let list = call(&core, "sites.list", Value::Null).await.unwrap();
    let names: Vec<&str> = list["sites"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["api.shop", "shop", "vite"]);
    assert_eq!(list["sites"][1]["php"], "8.3");
    assert_eq!(list["httpPort"], 8080);
    assert_eq!(list["parked"][0], projects.to_string_lossy().as_ref());
    call(&core, "sites.isolate", json!({ "name": "shop", "minor": null })).await.unwrap();
    call(&core, "sites.unlink", json!({ "name": "api.shop" })).await.unwrap();
    call(&core, "sites.unproxy", json!({ "name": "vite" })).await.unwrap();
    call(&core, "sites.unpark", json!({ "dir": projects })).await.unwrap();
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["sites"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_port_and_the_web_status_are_reachable() {
    let (_d, core, _projects) = home();
    call(&core, "sites.port", json!({ "port": 8081 })).await.unwrap();
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["httpPort"], 8081);
    let status = call(&core, "web.status", Value::Null).await.unwrap();
    assert_eq!(status["httpPort"], 8081);
    assert_eq!(status["running"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_params_and_site_errors() {
    let (_d, core, _projects) = home();
    for (method, params) in [
        ("sites.park", json!({})),
        ("sites.link", json!({ "name": "x" })),
        ("sites.port", json!({ "port": "80" })),
        ("sites.port", json!({ "port": 70000 })),
        ("sites.isolate", json!({ "name": "x", "minor": 8.4 })),
    ] {
        assert_eq!(call(&core, method, params).await.unwrap_err().code, INVALID_PARAMS, "{method}");
    }
    let err = call(&core, "sites.unlink", json!({ "name": "nope" })).await.unwrap_err();
    assert_eq!(err.message, "nope is not a linked site");
}
