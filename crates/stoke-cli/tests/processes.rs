//! A site's processes from the CLI.

use std::path::Path;

use assert_cmd::Command;
use stoke_core::ipc::{router, serve, CoreState};
use stoke_core::processes::store::{self, Proc};
use stoke_core::StokePaths;

fn home() -> (tempfile::TempDir, StokePaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(dir.path().join("stoke"));
    paths.ensure().unwrap();
    let shop = dir.path().join("shop");
    std::fs::create_dir_all(&shop).unwrap();
    stoke_core::sites::registry::link(&paths, "shop", &shop).unwrap();
    let proc = Proc { name: "app".into(), command: "php artisan queue:listen".into(), autostart: false };
    store::add(&paths, "shop", proc).unwrap();
    (dir, paths)
}

fn stoke(paths: &StokePaths, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("stoke").unwrap().env("STOKE_HOME", &paths.root).current_dir(cwd).args(args).output().unwrap()
}

fn text(out: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[tokio::test(flavor = "multi_thread")]
async fn list_and_refuse_an_unknown_process() {
    let (d, paths) = home();
    let core = CoreState::new(paths.clone());
    let server = serve(&paths, router(core.clone())).await.unwrap();
    let (p, cwd) = (paths.clone(), d.path().to_path_buf());
    let outs = tokio::task::spawn_blocking(move || {
        vec![
            stoke(&p, &cwd, &["processes"]),
            stoke(&p, &cwd, &["processes:list", "--json"]),
            stoke(&p, &cwd, &["processes:start", "shop", "nope"]),
            stoke(&p, &cwd, &["processes:stop", "shop", "app"]),
        ]
    })
    .await
    .unwrap();
    server.shutdown().await;
    assert_eq!(text(&outs[0]).trim(), "shop  app  stopped  php artisan queue:listen");
    let json: serde_json::Value = serde_json::from_slice(&outs[1].stdout).unwrap();
    assert_eq!(json["sites"][0]["processes"][0]["name"], "app");
    assert!(!outs[2].status.success());
    assert_eq!(text(&outs[2]).trim(), "shop has no process named nope");
    assert!(outs[3].status.success(), "{}", text(&outs[3]));
    assert_eq!(text(&outs[3]).trim(), "shop app is stopped");
}

#[test]
fn without_processes_the_list_says_how_to_add_one() {
    let dir = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(dir.path().join("stoke"));
    paths.ensure().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let out = rt.block_on(async {
        let core = CoreState::new(paths.clone());
        let server = serve(&paths, router(core)).await.unwrap();
        let (p, cwd) = (paths.clone(), dir.path().to_path_buf());
        let out = tokio::task::spawn_blocking(move || stoke(&p, &cwd, &["processes"])).await.unwrap();
        server.shutdown().await;
        out
    });
    assert_eq!(text(&out).trim(), "No processes yet. Add them on Stoke's Processes page.");
}
