use std::path::Path;

use assert_cmd::Command;
use hearth_core::ipc::{router, serve, CoreState};
use hearth_core::HearthPaths;

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

fn text(out: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[tokio::test(flavor = "multi_thread")]
async fn link_uses_the_current_folder_and_links_lists_it() {
    let (d, paths) = home();
    let shop = d.path().join("My Shop");
    project(&shop);
    let p = paths.clone();
    let (link, links, json) = with_core(&paths, move || {
        (hearth(&p, &shop, &["link"]), hearth(&p, &shop, &["links"]), hearth(&p, &shop, &["sites", "--json"]))
    })
    .await;
    assert_eq!(link.status.code(), Some(0), "{}", text(&link));
    assert_eq!(String::from_utf8(link.stdout).unwrap(), "my-shop is now served at http://my-shop.test:8080\n");
    assert!(String::from_utf8_lossy(&links.stdout).starts_with("my-shop.test  linked  "), "{}", text(&links));
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["sites"][0]["name"], "my-shop");
}

#[tokio::test(flavor = "multi_thread")]
async fn park_proxy_isolate_and_their_undo() {
    let (d, paths) = home();
    let projects = d.path().join("Projects");
    project(&projects.join("blog"));
    let p = paths.clone();
    let outs = with_core(&paths, move || {
        vec![
            hearth(&p, &projects, &["park"]),
            hearth(&p, &projects, &["proxy", "vite", "http://localhost:5173"]),
            hearth(&p, &projects.join("blog"), &["isolate", "8.3"]),
            hearth(&p, &projects, &["isolate", "8.2", "--site", "blog"]),
            hearth(&p, &projects, &["unisolate", "--site", "blog"]),
            hearth(&p, &projects, &["unproxy", "vite"]),
            hearth(&p, &projects, &["unpark"]),
            hearth(&p, &projects, &["unlink", "nope"]),
        ]
    })
    .await;
    for out in &outs[..7] {
        assert_eq!(out.status.code(), Some(0), "{}", text(out));
    }
    assert_eq!(String::from_utf8_lossy(&outs[2].stdout), "blog now runs on PHP 8.3\n");
    assert_eq!(outs[7].status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&outs[7].stderr), "nope is not a linked site\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn secure_secured_and_unsecure() {
    let (d, paths) = home();
    let shop = d.path().join("shop");
    project(&shop);
    let p = paths.clone();
    let outs = with_core(&paths, move || {
        vec![
            hearth(&p, &shop, &["link"]),
            hearth(&p, &shop, &["secure"]),
            hearth(&p, &shop, &["secured"]),
            hearth(&p, &shop, &["secured", "--json"]),
            hearth(&p, &shop, &["unsecure", "shop"]),
            hearth(&p, &shop, &["dns:setup"]),
        ]
    })
    .await;
    for out in &outs[..5] {
        assert_eq!(out.status.code(), Some(0), "{}", text(out));
    }
    assert_eq!(String::from_utf8_lossy(&outs[1].stdout), "shop is now served at https://shop.test:8443\n");
    assert_eq!(String::from_utf8_lossy(&outs[2].stdout), "https://shop.test:8443\n");
    let json: serde_json::Value = serde_json::from_slice(&outs[3].stdout).unwrap();
    assert_eq!(json, serde_json::json!(["shop"]));
    assert_eq!(String::from_utf8_lossy(&outs[4].stdout), "shop is now served at http://shop.test:8080\n");
    assert_eq!(outs[5].status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&outs[5].stderr), "This Hearth home does not manage .test names\n");
}
