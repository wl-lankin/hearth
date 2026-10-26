//! Herd's command shapes, as old scripts call them.

use std::path::Path;

use assert_cmd::Command;
use hearth_core::ipc::{router, serve, CoreState};
use hearth_core::php::store;
use hearth_core::HearthPaths;
use serde_json::{json, Value};

fn home() -> (tempfile::TempDir, HearthPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    (dir, paths)
}

fn project(dir: &Path) {
    std::fs::create_dir_all(dir.join("public")).unwrap();
    std::fs::write(dir.join("public").join("index.php"), "<?php").unwrap();
}

fn hearth(paths: &HearthPaths, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("hearth").unwrap().env("HEARTH_HOME", &paths.root).current_dir(cwd).args(args).output().unwrap()
}

async fn with_core<T: Send + 'static>(paths: &HearthPaths, f: impl FnOnce() -> T + Send + 'static) -> T {
    let server = serve(paths, router(CoreState::new(paths.clone()))).await.unwrap();
    let out = tokio::task::spawn_blocking(f).await.unwrap();
    server.shutdown().await;
    out
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn ok(out: &std::process::Output) {
    assert_eq!(out.status.code(), Some(0), "{}{}", stdout(out), String::from_utf8_lossy(&out.stderr));
}

fn json_of(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", stdout(out)))
}

#[tokio::test(flavor = "multi_thread")]
async fn link_with_secure_and_isolate_then_the_listings() {
    let (d, paths) = home();
    let shop = d.path().join("shop");
    project(&shop);
    let p = paths.clone();
    let outs = with_core(&paths, move || {
        vec![
            hearth(&p, &shop, &["link", "--secure", "--isolate=8.3"]),
            hearth(&p, &shop, &["proxy", "vite.test", "http://localhost:5173", "--secure"]),
            hearth(&p, &shop, &["proxies", "--json"]),
            hearth(&p, &shop, &["isolated", "--json"]),
            hearth(&p, &shop, &["secured", "--json"]),
            hearth(&p, &shop, &["site:info", "--json"]),
            hearth(&p, &shop, &["which"]),
            hearth(&p, &shop, &["unsecure", "--all"]),
            hearth(&p, &shop, &["secured", "--json"]),
        ]
    })
    .await;
    for out in &outs {
        ok(out);
    }
    assert_eq!(json_of(&outs[2]), json!([{ "name": "vite", "upstream": "http://localhost:5173", "secured": true }]));
    assert_eq!(json_of(&outs[3]), json!([{ "name": "shop", "php": "8.3" }]));
    assert_eq!(json_of(&outs[4]), json!(["shop", "vite"]));
    let info = json_of(&outs[5]);
    assert_eq!(
        (info["name"].as_str(), info["url"].as_str(), info["php"].as_str()),
        (Some("shop"), Some("https://shop.test:8443"), Some("8.3"))
    );
    assert!(stdout(&outs[6]).starts_with("shop.test"), "{}", stdout(&outs[6]));
    assert_eq!(json_of(&outs[8]), json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn parked_paths_forget_and_domains_with_test() {
    let (d, paths) = home();
    let projects = d.path().join("Projects");
    project(&projects.join("blog"));
    let p = paths.clone();
    let outs = with_core(&paths, move || {
        vec![
            hearth(&p, &projects, &["park"]),
            hearth(&p, &projects, &["parked", "--json"]),
            hearth(&p, &projects, &["paths", "--json"]),
            hearth(&p, &projects, &["secure", "blog.test"]),
            hearth(&p, &projects, &["forget"]),
            hearth(&p, &projects, &["parked", "--json"]),
        ]
    })
    .await;
    for out in &outs {
        ok(out);
    }
    let parked = json_of(&outs[1]);
    assert_eq!(parked.as_array().map(Vec::len), Some(1));
    assert_eq!(json_of(&outs[2]), parked, "paths lists the same folders");
    assert_eq!(stdout(&outs[3]), "blog is now served at https://blog.test:8443\n");
    assert_eq!(json_of(&outs[5]), json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn tld_loopback_restart_and_which_php() {
    let (d, paths) = home();
    std::fs::create_dir_all(store::install_dir(&paths, "8.4")).unwrap();
    std::fs::write(store::install_dir(&paths, "8.4").join(store::VERSION_FILE), "8.4.27").unwrap();
    store::register(&paths, "8.4", &[]).unwrap();
    store::set_default(&paths, "8.4").unwrap();
    let p = paths.clone();
    let cwd = d.path().to_path_buf();
    let outs = with_core(&paths, move || {
        vec![
            hearth(&p, &cwd, &["tld"]),
            hearth(&p, &cwd, &["loopback"]),
            hearth(&p, &cwd, &["restart"]),
            hearth(&p, &cwd, &["which-php"]),
            hearth(&p, &cwd, &["use", "8.9"]),
        ]
    })
    .await;
    for out in &outs[..4] {
        ok(out);
    }
    assert_eq!(stdout(&outs[0]), "test\n");
    assert_eq!(stdout(&outs[1]), "127.0.0.1\n");
    let php = stdout(&outs[3]).trim().replace('\\', "/");
    assert!(php.contains("/versions/8.4/"), "{php}");
    assert_eq!(outs[4].status.code(), Some(1), "use is php:use");
}

#[test]
fn commands_of_later_milestones_say_what_is_missing() {
    let (d, paths) = home();
    for (args, line) in [
        (vec!["mail:list"], "Hearth has no mail catcher yet\n"),
        (vec!["services:list", "--json"], "Hearth has no services (databases, Redis, ...) yet\n"),
        (vec!["workspace", "feature"], "Hearth has no workspaces yet\n"),
        (vec!["studio"], "Hearth has no Studio, and none is planned\n"),
        (vec!["frobnicate"], "Hearth has no `frobnicate` command (see `hearth --help`)\n"),
    ] {
        let out = hearth(&paths, d.path(), &args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), line, "{args:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_listing_speaks_json() {
    let (d, paths) = home();
    let p = paths.clone();
    let cwd = d.path().to_path_buf();
    let outs = with_core(&paths, move || {
        ["status", "php:list", "links", "parked", "proxies", "isolated", "secured"]
            .map(|cmd| (cmd, hearth(&p, &cwd, &[cmd, "--json"])))
    })
    .await;
    for (cmd, out) in outs {
        ok(&out);
        assert!(serde_json::from_slice::<Value>(&out.stdout).is_ok(), "{cmd}: {}", stdout(&out));
    }
}
