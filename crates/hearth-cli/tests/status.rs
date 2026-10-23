use std::time::Instant;

use assert_cmd::Command;
use hearth_core::ipc::{router, serve, CoreState};
use hearth_core::HearthPaths;
use predicates::prelude::*;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    (dir, paths)
}

fn hearth(paths: &HearthPaths) -> Command {
    let mut cmd = Command::cargo_bin("hearth").unwrap();
    cmd.env("HEARTH_HOME", &paths.root);
    cmd
}

#[test]
fn status_when_not_running_exits_1_with_message() {
    let (_d, paths) = home();
    hearth(&paths)
        .arg("status")
        .assert()
        .code(1)
        .stderr("Hearth core is not running. Start the Hearth app.\n");
}

#[test]
fn status_json_when_not_running() {
    let (_d, paths) = home();
    hearth(&paths).args(["status", "--json"]).assert().code(1).stdout("{\"running\":false}\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn status_when_running() {
    let (_d, paths) = home();
    let state = CoreState { paths: paths.clone(), started: Instant::now() };
    let server = serve(&paths, router(state)).await.unwrap();
    let cmd_paths = paths.clone();
    let output = tokio::task::spawn_blocking(move || hearth(&cmd_paths).arg("status").output().unwrap())
        .await
        .unwrap();
    server.shutdown().await;
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let re = predicate::str::is_match(r"^Hearth core is running \(v0\.1\.0, pid \d+, up \d+s\)\n$").unwrap();
    assert!(re.eval(&stdout), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_when_running() {
    let (_d, paths) = home();
    let state = CoreState { paths: paths.clone(), started: Instant::now() };
    let server = serve(&paths, router(state)).await.unwrap();
    let cmd_paths = paths.clone();
    let output = tokio::task::spawn_blocking(move || hearth(&cmd_paths).args(["status", "--json"]).output().unwrap())
        .await
        .unwrap();
    server.shutdown().await;
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], "0.1.0");
    assert_eq!(value["home"], paths.root.to_string_lossy().as_ref());
}

#[test]
fn version_flag() {
    Command::cargo_bin("hearth").unwrap().arg("--version").assert().success().stdout("hearth 0.1.0\n");
}
