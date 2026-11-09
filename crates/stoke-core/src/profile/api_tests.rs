use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError};
use crate::profile::{data_dir, spx_key};
use crate::StokePaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

fn home() -> (tempfile::TempDir, CoreState) {
    let d = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(d.path().join("stoke"));
    paths.ensure().unwrap();
    (d, CoreState::new(paths))
}

#[tokio::test(flavor = "multi_thread")]
async fn status_says_whether_any_php_has_spx_and_gives_the_key() {
    let (_d, core) = home();
    let status = call(&core, "profile.status", Value::Null).await.unwrap();
    assert_eq!(status["available"], json!(false));
    crate::php::store::register(&core.paths, "8.4", &["spx".to_string()]).unwrap();
    let status = call(&core, "profile.status", Value::Null).await.unwrap();
    assert_eq!(status["available"], json!(true));
    assert_eq!(status["key"], json!(spx_key(&core.paths).unwrap()));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_and_clear_work_on_the_spx_folder() {
    let (_d, core) = home();
    let dir = data_dir(&core.paths);
    std::fs::create_dir_all(&dir).unwrap();
    let meta = json!({ "key": "spx-full-1", "exec_ts": 7, "cli": 0, "http_method": "GET",
        "http_request_uri": "/", "http_host": "shop.test", "wall_time_ms": 1,
        "peak_memory_usage": 1, "called_function_count": 1 });
    std::fs::write(dir.join("spx-full-1.json"), meta.to_string()).unwrap();
    let listed = call(&core, "profile.list", Value::Null).await.unwrap();
    assert_eq!(listed["reports"][0]["key"], json!("spx-full-1"));
    assert_eq!(listed["reports"][0]["wallMs"], json!(1));
    assert_eq!(call(&core, "profile.clear", Value::Null).await.unwrap(), json!({ "deleted": 1 }));
    assert_eq!(call(&core, "profile.list", Value::Null).await.unwrap()["reports"], json!([]));
}
