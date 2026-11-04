use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError};
use crate::sites::registry;
use crate::workspaces::git_tests::{repo, sh};
use crate::HearthPaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

/// A home with the site `shop`: a git repo with public/index.php, `.env` and `.env.testing`
/// (gitignored, as in Laravel), linked and isolated to PHP 8.3.
fn home() -> (tempfile::TempDir, CoreState, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    let shop = repo(d.path());
    std::fs::create_dir_all(shop.join("public")).unwrap();
    std::fs::write(shop.join("public").join("index.php"), "<?php").unwrap();
    std::fs::write(shop.join(".gitignore"), ".env\n.env.testing\n/vendor\n").unwrap();
    sh(&shop, &["add", "."]);
    sh(&shop, &["commit", "-q", "-m", "app"]);
    std::fs::write(
        shop.join(".env"),
        "APP_NAME=Shop\r\nAPP_URL=http://shop.test:8080\r\nSESSION_DOMAIN=shop.test\r\nSANCTUM_STATEFUL_DOMAINS=shop.test:8080,localhost\r\nDB_CONNECTION=sqlite\r\n",
    )
    .unwrap();
    std::fs::write(shop.join(".env.testing"), "APP_ENV=testing\n").unwrap();
    std::fs::create_dir_all(shop.join("vendor").join("x")).unwrap();
    std::fs::write(shop.join("vendor").join("x").join("y.php"), "<?php").unwrap();
    std::fs::write(shop.join("composer.lock"), "{}").unwrap();
    sh(&shop, &["add", "composer.lock"]);
    sh(&shop, &["commit", "-q", "-m", "lock"]);
    registry::link(&paths, "shop", &shop).unwrap();
    registry::isolate(&paths, "shop", Some("8.3")).unwrap();
    let mut core = CoreState::new(paths);
    core.web_sync = false;
    (d, core, shop)
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_is_a_worktree_with_its_own_host_env_and_settings() {
    let (_d, core, shop) = home();
    let ws = call(&core, "workspaces.create", json!({ "site": "shop", "name": "feature/X" })).await.unwrap();
    let w = &ws["workspace"];
    assert_eq!(w["site"], "feature-x.shop");
    assert_eq!(w["url"], "http://feature-x.shop.test:8080");
    assert_eq!((w["branch"].as_str(), w["baseBranch"].as_str()), (Some("feature/X"), Some("main")));
    let path = PathBuf::from(w["path"].as_str().unwrap());
    assert_eq!(path, core.paths.data.join("workspaces").join("shop").join("feature-x"));
    assert_eq!(sh(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "feature/X");
    // Its own .env (rewritten) and .env.testing (copied as is), the vendor folder of an equal lock.
    let env = read(&path.join(".env"));
    assert!(env.contains("APP_URL=http://feature-x.shop.test:8080\r\n"), "{env}");
    assert!(env.contains("SESSION_DOMAIN=feature-x.shop.test\r\n"), "{env}");
    assert!(env.contains("SANCTUM_STATEFUL_DOMAINS=feature-x.shop.test:8080,localhost\r\n"), "{env}");
    assert_eq!(read(&path.join(".env.testing")), "APP_ENV=testing\n");
    assert!(path.join("vendor").join("x").join("y.php").is_file());
    assert_eq!(read(&shop.join(".env")).matches("shop.test").count(), 3, "the parent's .env is untouched");
    // Linked, with the parent's PHP; listed; marked.
    let site = registry::list(&core.paths).into_iter().find(|s| s.name == "feature-x.shop").unwrap();
    assert_eq!(site.php.as_deref(), Some("8.3"));
    assert!(path.join(".hearth-workspace.json").is_file());
    assert!(read(&shop.join(".git").join("info").join("exclude")).contains(".hearth-workspace.json"));
    let list = call(&core, "workspaces.list", json!({ "site": "shop" })).await.unwrap();
    assert_eq!(list["workspaces"][0]["site"], "feature-x.shop");
    let found = call(&core, "workspaces.find", json!({ "cwd": path.join("public") })).await.unwrap();
    assert_eq!(found["workspace"]["site"], "feature-x.shop");
    // A second one with the same branch name gets another host and folder.
    let again = call(&core, "workspaces.create", json!({ "site": "shop", "name": "feature-x" })).await.unwrap();
    assert_eq!(again["workspace"]["site"], "feature-x-2.shop");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failure_after_the_worktree_leaves_nothing_behind() {
    let (_d, core, shop) = home();
    // An .env that cannot be copied (a folder) fails the create after git made the worktree.
    std::fs::remove_file(shop.join(".env")).unwrap();
    std::fs::create_dir(shop.join(".env")).unwrap();
    let err = call(&core, "workspaces.create", json!({ "site": "shop", "name": "broken" })).await.unwrap_err();
    assert!(!err.message.is_empty());
    assert!(!core.paths.data.join("workspaces").join("shop").join("broken").exists(), "the worktree is gone");
    assert!(!sh(&shop, &["worktree", "list"]).contains("broken"));
    assert!(!registry::list(&core.paths).iter().any(|s| s.name.starts_with("broken")));
    assert_eq!(call(&core, "workspaces.list", Value::Null).await.unwrap()["workspaces"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn merge_brings_the_work_home_and_delete_cleans_up_but_keeps_the_branch() {
    let (_d, core, shop) = home();
    let ws = call(&core, "workspaces.create", json!({ "site": "shop", "name": "feature-x" })).await.unwrap();
    let path = PathBuf::from(ws["workspace"]["path"].as_str().unwrap());
    std::fs::write(path.join("new.txt"), "work\n").unwrap();
    let err = call(&core, "workspaces.merge", json!({ "name": "feature-x" })).await.unwrap_err();
    assert_eq!(err.message, "The workspace feature-x has uncommitted changes: commit them first");
    sh(&path, &["add", "new.txt"]);
    sh(&path, &["commit", "-q", "-m", "work"]);
    let merged = call(&core, "workspaces.merge", json!({ "name": "feature-x" })).await.unwrap();
    assert_eq!(merged["message"], "Merged feature-x into main of shop");
    assert!(shop.join("new.txt").is_file());
    call(&core, "workspaces.delete", json!({ "name": "feature-x.shop" })).await.unwrap();
    assert!(!path.exists());
    assert!(!registry::list(&core.paths).iter().any(|s| s.name == "feature-x.shop"));
    assert!(sh(&shop, &["branch", "--list", "feature-x"]).contains("feature-x"), "the branch stays");
    assert_eq!(call(&core, "workspaces.list", Value::Null).await.unwrap()["workspaces"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worktree_made_by_hand_is_adopted_and_its_files_are_kept_on_delete() {
    let (d, core, shop) = home();
    let outside = d.path().join("shop-hotfix");
    sh(&shop, &["worktree", "add", "-q", "-b", "hotfix", &outside.to_string_lossy()]);
    let ws = call(&core, "workspaces.adopt", json!({ "path": outside })).await.unwrap();
    assert_eq!(ws["workspace"]["site"], "hotfix.shop");
    assert_eq!(ws["workspace"]["kind"], "worktree");
    assert!(outside.join(".env").is_file(), "seeded from the parent");
    call(&core, "workspaces.delete", json!({ "name": "hotfix", "keepFiles": true })).await.unwrap();
    assert!(outside.join("app.txt").is_file(), "kept");
    assert!(!registry::list(&core.paths).iter().any(|s| s.name == "hotfix.shop"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_without_git_is_refused_with_how_to_start() {
    let (d, core, _shop) = home();
    let plain = d.path().join("plain");
    std::fs::create_dir_all(plain.join("public")).unwrap();
    std::fs::write(plain.join("public").join("index.php"), "<?php").unwrap();
    registry::link(&core.paths, "plain", &plain).unwrap();
    let err = call(&core, "workspaces.create", json!({ "site": "plain" })).await.unwrap_err();
    assert_eq!(err.message, "Workspaces need a git repository. Run `git init` and commit first.");
    let err = call(&core, "workspaces.create", json!({ "site": "nope" })).await.unwrap_err();
    assert_eq!(err.message, "No site named nope.test");
}
