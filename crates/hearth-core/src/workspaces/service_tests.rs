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
    // Hearth stores the resolved path (CI's temp folder is an 8.3 name like RUNNER~1).
    assert_eq!(
        path,
        std::fs::canonicalize(core.paths.data.join("workspaces").join("shop").join("feature-x"))
            .map(|p| PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\").to_string()))
            .unwrap()
    );
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

/// A site that is itself a worktree (its `.git` is a file): a workspace of it still works.
#[tokio::test(flavor = "multi_thread")]
async fn a_site_whose_git_folder_is_a_file_can_still_branch() {
    let (d, core, shop) = home();
    let outside = d.path().join("shop-hotfix");
    sh(&shop, &["worktree", "add", "-q", "-b", "hotfix", &outside.to_string_lossy()]);
    call(&core, "workspaces.adopt", json!({ "path": outside })).await.unwrap();
    let ws = call(&core, "workspaces.create", json!({ "site": "hotfix.shop", "name": "deeper" })).await.unwrap();
    assert_eq!(ws["workspace"]["site"], "deeper.hotfix.shop");
}

/// Worktrees inside the repository (`.worktrees/`, agents' folders): each one's parent is the
/// repository's own site, never an earlier adopted worktree.
#[tokio::test(flavor = "multi_thread")]
async fn worktrees_inside_the_repository_all_get_the_main_site_as_parent() {
    let (_d, core, shop) = home();
    for name in ["a", "b"] {
        let dir = shop.join(".worktrees").join(name);
        sh(&shop, &["worktree", "add", "-q", "-b", name, &dir.to_string_lossy()]);
        let ws = call(&core, "workspaces.adopt", json!({ "path": dir })).await.unwrap();
        assert_eq!(ws["workspace"]["parentSite"], "shop", "{name}");
        assert_eq!(ws["workspace"]["appDir"], "", "{name}");
        assert_eq!(ws["workspace"]["site"], format!("{name}.shop"));
    }
}

/// A failure after the site was linked takes everything back: link, worktree, record, and the
/// branch the create made.
#[tokio::test(flavor = "multi_thread")]
async fn a_failure_after_linking_takes_everything_back() {
    let (_d, core, shop) = home();
    // The registry cannot be written: the last step before the marker fails.
    std::fs::create_dir_all(core.paths.config.join("workspaces.json")).unwrap();
    let err = call(&core, "workspaces.create", json!({ "site": "shop", "name": "late" })).await;
    assert!(err.is_err());
    assert!(!registry::list(&core.paths).iter().any(|s| s.name == "late.shop"), "unlinked");
    assert!(!core.paths.data.join("workspaces").join("shop").join("late").exists());
    assert!(!sh(&shop, &["branch", "--list", "late"]).contains("late"), "the new branch is gone too");
}

/// An adopted worktree that git cannot remove (a file is open) is never deleted by hand.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn an_adopted_folder_is_never_deleted_by_hand() {
    let (d, core, shop) = home();
    let outside = d.path().join("shop-open");
    sh(&shop, &["worktree", "add", "-q", "-b", "open", &outside.to_string_lossy()]);
    call(&core, "workspaces.adopt", json!({ "path": outside })).await.unwrap();
    // Held without delete sharing (Rust's default allows deleting an open file).
    let held = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).share_mode(0).open(outside.join("app.txt")).unwrap()
    };
    let done = call(&core, "workspaces.delete", json!({ "name": "open" })).await.unwrap();
    assert!(outside.join("app.txt").is_file(), "left to git, which could not");
    assert!(done["notes"][0].as_str().unwrap_or_default().contains("could not be removed"), "{done}");
    drop(held);
}

/// Links inside `vendor` (Composer path repositories) are links in the copy too.
#[tokio::test(flavor = "multi_thread")]
async fn links_in_dependencies_are_recreated_not_dropped() {
    let (d, core, shop) = home();
    let package = d.path().join("acme-pkg");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("Pkg.php"), "<?php").unwrap();
    let link = shop.join("vendor").join("acme");
    std::fs::create_dir_all(&link).unwrap();
    let link = link.join("pkg");
    #[cfg(windows)]
    {
        let out = std::process::Command::new("cmd")
            .arg("/c")
            .arg("mklink")
            .arg("/J")
            .arg(&link)
            .arg(&package)
            .output()
            .unwrap();
        assert!(out.status.success());
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&package, &link).unwrap();
    let ws = call(&core, "workspaces.create", json!({ "site": "shop", "name": "linked" })).await.unwrap();
    let copied = PathBuf::from(ws["workspace"]["path"].as_str().unwrap()).join("vendor").join("acme").join("pkg");
    assert!(copied.join("Pkg.php").is_file(), "the package is reachable");
    assert!(
        std::fs::symlink_metadata(&copied).unwrap().file_type().is_symlink() || copied.read_link().is_ok(),
        "as a link"
    );
}

/// A merge that worked but could not be pushed reports the merge, with the push as a note.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_push_after_a_merge_is_a_note_not_a_failure() {
    let (_d, core, shop) = home();
    let ws = call(&core, "workspaces.create", json!({ "site": "shop", "name": "pushme" })).await.unwrap();
    let path = PathBuf::from(ws["workspace"]["path"].as_str().unwrap());
    std::fs::write(path.join("p.txt"), "p\n").unwrap();
    sh(&path, &["add", "p.txt"]);
    sh(&path, &["commit", "-q", "-m", "p"]);
    // No origin: the push fails.
    let done = call(&core, "workspaces.merge", json!({ "name": "pushme", "push": true })).await.unwrap();
    assert_eq!(done["message"], "Merged pushme into main of shop");
    assert!(done["notes"][0].as_str().unwrap_or_default().starts_with("main could not be pushed"), "{done}");
    assert!(shop.join("p.txt").is_file());
}
