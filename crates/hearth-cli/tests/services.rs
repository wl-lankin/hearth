//! Services from the CLI, in Herd's command shapes.

use std::net::TcpListener;
use std::path::Path;

use assert_cmd::Command;
use hearth_core::ipc::{router, serve, CoreState};
use hearth_core::HearthPaths;
use serde_json::Value;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    // An installed Redis 8 that is really the core's fake service (listens on its --port).
    let fake = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join("examples")
        .join(format!("fake_service{}", std::env::consts::EXE_SUFFIX));
    let program = paths
        .data
        .join("services")
        .join("bin")
        .join("redis")
        .join("8.10.3")
        .join(format!("redis-server{}", std::env::consts::EXE_SUFFIX));
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::copy(&fake, &program)
        .unwrap_or_else(|e| panic!("{}: {e} (build hearth-core's examples first)", fake.display()));
    (dir, paths)
}

fn hearth(paths: &HearthPaths, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("hearth").unwrap().env("HEARTH_HOME", &paths.root).current_dir(cwd).args(args).output().unwrap()
}

async fn with_core<T: Send + 'static>(paths: &HearthPaths, f: impl FnOnce() -> T + Send + 'static) -> T {
    let core = CoreState::new(paths.clone());
    let server = serve(paths, router(core.clone())).await.unwrap();
    let out = tokio::task::spawn_blocking(f).await.unwrap();
    hearth_core::services::run::stop_all(&core).await;
    server.shutdown().await;
    out
}

fn text(out: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[tokio::test(flavor = "multi_thread")]
async fn create_list_start_stop_and_delete() {
    let (d, paths) = home();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port().to_string();
    let p = paths.clone();
    let cwd = d.path().to_path_buf();
    let outs = with_core(&paths, move || {
        vec![
            hearth(&p, &cwd, &["services:create", "redis", "--service-version=8", &format!("--port={port}")]),
            hearth(&p, &cwd, &["services:start", "redis"]),
            hearth(&p, &cwd, &["services:list", "--json"]),
            hearth(&p, &cwd, &["restart", "redis"]),
            hearth(&p, &cwd, &["services:stop", "redis"]),
            hearth(&p, &cwd, &["services:delete", "redis"]),
            hearth(&p, &cwd, &["services:delete", "redis", "--force"]),
            hearth(&p, &cwd, &["services:list"]),
        ]
    })
    .await;
    for (i, out) in outs.iter().enumerate() {
        if i == 5 {
            continue;
        }
        assert_eq!(out.status.code(), Some(0), "step {i}: {}", text(out));
    }
    assert!(String::from_utf8_lossy(&outs[0].stdout).starts_with("Created redis (Redis 8)"), "{}", text(&outs[0]));
    let list: Value = serde_json::from_slice(&outs[2].stdout).unwrap();
    assert_eq!(list[0]["running"], true);
    assert_eq!(outs[5].status.code(), Some(1), "delete asks for --force first");
    assert_eq!(
        String::from_utf8_lossy(&outs[5].stderr),
        "This deletes Redis 8 and all its data. Run it again with --force.\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&outs[7].stdout),
        "No services yet. Create one with: hearth services:create <type>\n"
    );
}
