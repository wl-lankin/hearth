use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError};
use crate::jobs::JobState;
use crate::php::service_tests::{fixture, wait};

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

async fn install(core: &CoreState, spec: &str) -> Value {
    let started = call(core, "node.install", json!({ "spec": spec })).await.unwrap();
    let job = wait(core, started["jobId"].as_u64().unwrap()).await;
    assert_eq!(job.state, JobState::Done, "{:?}", job.error);
    started
}

#[tokio::test(flavor = "multi_thread")]
async fn install_resolves_a_spec_against_the_catalog() {
    let f = fixture(false);
    let started = install(&f.core, "lts/iron").await;
    assert_eq!(started["version"], "20.19.5");
    install(&f.core, "v22").await;
    let list = call(&f.core, "node.list", Value::Null).await.unwrap();
    assert_eq!(
        list,
        json!({
            "installed": [
                { "version": "22.21.1", "major": 22, "lts": "Jod" },
                { "version": "20.19.5", "major": 20, "lts": "Iron" },
            ],
            "default": "20.19.5",
        })
    );
    let err = call(&f.core, "node.install", json!({ "spec": "system" })).await.unwrap_err();
    assert_eq!(err.message, "Name a Node version, like 22 or lts/*");
}

#[tokio::test(flavor = "multi_thread")]
async fn available_marks_what_is_installed() {
    let f = fixture(false);
    install(&f.core, "22").await;
    let rows = call(&f.core, "node.available", Value::Null).await.unwrap();
    assert_eq!(
        rows,
        json!([
            { "major": 22, "version": "22.21.1", "lts": "Jod", "installed": true },
            { "major": 21, "version": "21.7.3", "lts": null, "installed": false },
            { "major": 20, "version": "20.19.5", "lts": "Iron", "installed": false },
        ])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn default_and_uninstall_take_a_spec_of_an_installed_version() {
    let f = fixture(false);
    install(&f.core, "22").await;
    install(&f.core, "20").await;
    assert_eq!(call(&f.core, "node.default", json!({ "spec": "20" })).await.unwrap(), json!({ "version": "20.19.5" }));
    assert_eq!(call(&f.core, "node.list", Value::Null).await.unwrap()["default"], "20.19.5");
    let err = call(&f.core, "node.default", json!({ "spec": "18" })).await.unwrap_err();
    assert_eq!(err.message, "Node 18 is not installed");
    assert_eq!(
        call(&f.core, "node.uninstall", json!({ "spec": "20" })).await.unwrap(),
        json!({ "version": "20.19.5" })
    );
    assert_eq!(call(&f.core, "node.list", Value::Null).await.unwrap()["default"], "22.21.1");
}

#[tokio::test(flavor = "multi_thread")]
async fn which_names_the_version_and_why() {
    let f = fixture(false);
    install(&f.core, "22").await;
    install(&f.core, "20").await;
    let project = f.core.paths.root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let found = call(&f.core, "node.which", json!({ "dir": project })).await.unwrap();
    assert_eq!(found, json!({ "version": "22.21.1", "file": null, "error": null }));
    std::fs::write(project.join(".nvmrc"), "20").unwrap();
    let found = call(&f.core, "node.which", json!({ "dir": project })).await.unwrap();
    assert_eq!(found["version"], "20.19.5");
    assert_eq!(found["file"], json!(project.join(".nvmrc")));
    std::fs::write(project.join(".nvmrc"), "18").unwrap();
    let found = call(&f.core, "node.which", json!({ "dir": project })).await.unwrap();
    assert_eq!(found["version"], Value::Null);
    assert!(found["error"].as_str().unwrap().starts_with("This project wants Node 18"), "{found}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exact_version_outside_the_catalog_installs_from_nodejs_org() {
    let f = fixture(false);
    let started = install(&f.core, "20.11.1").await;
    assert_eq!((&started["version"], &started["source"]), (&json!("20.11.1"), &json!("nodejs.org")));
    // An LTS line the catalog no longer carries, by its codename.
    let started = install(&f.core, "lts/hydrogen").await;
    assert_eq!((&started["version"], &started["source"]), (&json!("18.20.4"), &json!("nodejs.org")));
    let list = call(&f.core, "node.list", Value::Null).await.unwrap();
    assert_eq!(
        list["installed"],
        json!([
            { "version": "20.11.1", "major": 20, "lts": "Iron" },
            { "version": "18.20.4", "major": 18, "lts": "Hydrogen" },
        ])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_catalog_wins_when_both_have_the_spec() {
    let f = fixture(false);
    let started = install(&f.core, "22").await;
    assert_eq!((&started["version"], &started["source"]), (&json!("22.21.1"), &json!("catalog")));
    let hits = f.hits.lock().unwrap();
    assert!(!hits.contains_key("/dist/index.json"), "{hits:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_nodejs_org_build_installs_nothing() {
    let f = fixture(false);
    // The download does not match SHASUMS256.txt: the job fails.
    let started = call(&f.core, "node.install", json!({ "spec": "20.10.0" })).await.unwrap();
    let job = wait(&f.core, started["jobId"].as_u64().unwrap()).await;
    assert_eq!(job.state, JobState::Failed);
    // The file is missing from SHASUMS256.txt: refused before any download.
    let err = call(&f.core, "node.install", json!({ "spec": "18.19" })).await.unwrap_err();
    let file = if cfg!(windows) { "node-v18.19.0-win-x64.zip" } else { "node-v18.19.0-darwin" };
    assert!(err.message.starts_with(&format!("nodejs.org does not list a checksum for {file}")), "{}", err.message);
    let node = crate::node::store::node_dir(&f.core.paths);
    for version in ["20.10.0", "18.19.0"] {
        assert!(!node.join(format!("v{version}")).exists(), "{version}");
    }
    assert_eq!(call(&f.core, "node.list", Value::Null).await.unwrap()["installed"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn no_build_anywhere_names_both_sources() {
    let f = fixture(false);
    for spec in ["9.9.9", "14"] {
        let err = call(&f.core, "node.install", json!({ "spec": spec })).await.unwrap_err();
        assert_eq!(err.message, format!("Neither Stoke's catalog nor nodejs.org has Node {spec} for this computer"));
    }
}
