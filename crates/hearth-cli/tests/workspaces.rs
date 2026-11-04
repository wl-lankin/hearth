//! Workspaces from the CLI, in Herd's command shapes.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use hearth_core::ipc::{router, serve, CoreState};
use hearth_core::HearthPaths;

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A home with the site `shop`: a git repo with one commit and an `.env`.
fn home() -> (tempfile::TempDir, HearthPaths, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    let shop = dir.path().join("shop");
    std::fs::create_dir_all(shop.join("public")).unwrap();
    std::fs::write(shop.join("public").join("index.php"), "<?php").unwrap();
    std::fs::write(shop.join(".gitignore"), ".env\n").unwrap();
    git(&shop, &["init", "-q", "-b", "main"]);
    git(&shop, &["config", "user.email", "t@example.com"]);
    git(&shop, &["config", "user.name", "Test"]);
    git(&shop, &["add", "."]);
    git(&shop, &["commit", "-q", "-m", "start"]);
    std::fs::write(shop.join(".env"), "APP_URL=http://shop.test:8080\n").unwrap();
    hearth_core::sites::registry::link(&paths, "shop", &shop).unwrap();
    (dir, paths, shop)
}

fn hearth(paths: &HearthPaths, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("hearth").unwrap().env("HEARTH_HOME", &paths.root).current_dir(cwd).args(args).output().unwrap()
}

fn text(out: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[tokio::test(flavor = "multi_thread")]
async fn make_list_switch_and_delete_a_workspace() {
    let (d, paths, shop) = home();
    let plain = d.path().join("plain");
    std::fs::create_dir_all(plain.join("public")).unwrap();
    std::fs::write(plain.join("public").join("index.php"), "<?php").unwrap();
    hearth_core::sites::registry::link(&paths, "plain", &plain).unwrap();
    let mut core = CoreState::new(paths.clone());
    core.web_sync = false;
    let server = serve(&paths, router(core)).await.unwrap();
    let (p, cwd, plain2) = (paths.clone(), shop.clone(), plain.clone());
    let outs = tokio::task::spawn_blocking(move || {
        vec![
            hearth(&p, &cwd, &["workspace", "feature-x"]),
            hearth(&p, &cwd, &["workspaces"]),
            hearth(&p, &cwd, &["workspaces", "--json"]),
            hearth(&p, &cwd, &["workspace:switch", "feature-x"]),
            hearth(&p, &cwd, &["workspace:delete", "feature-x", "--force"]),
            hearth(&p, &cwd, &["workspaces"]),
            hearth(&p, &plain2, &["workspace"]),
        ]
    })
    .await
    .unwrap();
    server.shutdown().await;
    let made = text(&outs[0]);
    assert!(outs[0].status.success(), "{made}");
    assert!(made.contains("Workspace feature-x is ready at http://feature-x.shop.test:8080"), "{made}");
    assert!(made.contains("It shares the parent's database"), "{made}");
    let list = text(&outs[1]);
    assert!(list.contains("feature-x.shop") && list.contains("http://feature-x.shop.test:8080"), "{list}");
    let json: serde_json::Value = serde_json::from_slice(&outs[2].stdout).unwrap();
    assert_eq!(json["workspaces"][0]["branch"], "feature-x");
    let path = paths.data.join("workspaces").join("shop").join("feature-x");
    // Compared as text: the folder is gone by now (the delete ran after the switch). Hearth prints
    // the resolved path (CI's temp folder is an 8.3 name like RUNNER~1; macOS's is a link), so the
    // expected one is built from the resolved home, which still exists.
    let resolved = std::fs::canonicalize(&paths.data).unwrap().join("workspaces").join("shop").join("feature-x");
    let norm = |p: &str| p.trim().trim_start_matches(r"\\?\").replace('\\', "/").to_lowercase();
    assert_eq!(norm(&text(&outs[3])), norm(&resolved.to_string_lossy()), "switch prints the folder");
    assert!(outs[4].status.success(), "{}", text(&outs[4]));
    assert!(
        text(&outs[4]).contains("Workspace feature-x is deleted; its branch feature-x stays"),
        "{}",
        text(&outs[4])
    );
    assert!(!path.exists());
    assert!(text(&outs[5]).contains("No workspaces yet"), "{}", text(&outs[5]));
    assert!(!outs[6].status.success());
    assert_eq!(text(&outs[6]).trim(), "Workspaces need a git repository. Run `git init` and commit first.");
}

#[test]
fn workspace_is_no_longer_a_missing_feature() {
    let (d, paths, _shop) = home();
    let out = hearth(&paths, d.path(), &["workspace:frobnicate"]);
    assert!(!text(&out).contains("Hearth has no workspaces yet"), "{}", text(&out));
    let both = hearth(&paths, d.path(), &["workspace", "x", "--secure", "--no-secure"]);
    assert_eq!(both.status.code(), Some(2), "--secure and --no-secure exclude each other: {}", text(&both));
}
