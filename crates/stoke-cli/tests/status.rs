use assert_cmd::Command;
use predicates::prelude::*;
use stoke_core::ipc::{router, serve, CoreState};
use stoke_core::StokePaths;

fn home() -> (tempfile::TempDir, StokePaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(dir.path().join("stoke"));
    paths.ensure().unwrap();
    (dir, paths)
}

fn stoke(paths: &StokePaths) -> Command {
    let mut cmd = Command::cargo_bin("stoke").unwrap();
    cmd.env("STOKE_HOME", &paths.root);
    cmd
}

#[test]
fn status_when_not_running_exits_1_with_message() {
    let (_d, paths) = home();
    stoke(&paths).arg("status").assert().code(1).stderr("Stoke core is not running. Start the Stoke app.\n");
}

#[test]
fn status_json_when_not_running() {
    let (_d, paths) = home();
    stoke(&paths).args(["status", "--json"]).assert().code(1).stdout("{\"running\":false}\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn status_when_running() {
    let (_d, paths) = home();
    let state = CoreState::new(paths.clone());
    let server = serve(&paths, router(state)).await.unwrap();
    let cmd_paths = paths.clone();
    let output = tokio::task::spawn_blocking(move || stoke(&cmd_paths).arg("status").output().unwrap()).await.unwrap();
    server.shutdown().await;
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let version = regex_escape(env!("CARGO_PKG_VERSION"));
    let re = predicate::str::is_match(format!(r"^Stoke core is running \(v{version}, pid \d+, up \d+s\)\n$")).unwrap();
    assert!(re.eval(&stdout), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn status_json_when_running() {
    let (_d, paths) = home();
    let state = CoreState::new(paths.clone());
    let server = serve(&paths, router(state)).await.unwrap();
    let cmd_paths = paths.clone();
    let output = tokio::task::spawn_blocking(move || stoke(&cmd_paths).args(["status", "--json"]).output().unwrap())
        .await
        .unwrap();
    server.shutdown().await;
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["home"], paths.root.to_string_lossy().as_ref());
}

#[test]
fn version_flag() {
    Command::cargo_bin("stoke")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("stoke {}\n", env!("CARGO_PKG_VERSION")));
}

/// The version with its dots escaped for a regex.
fn regex_escape(version: &str) -> String {
    version.replace('.', r"\.")
}
