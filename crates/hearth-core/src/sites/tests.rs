use std::path::Path;

use super::detect::docroot;
use super::registry::*;
use crate::HearthPaths;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let d = tempfile::tempdir().unwrap();
    let p = HearthPaths::at(d.path().join("hearth"));
    p.ensure().unwrap();
    (d, p)
}

fn project(dir: &Path, kind: &str) {
    std::fs::create_dir_all(dir).unwrap();
    match kind {
        "laravel" => {
            std::fs::create_dir_all(dir.join("public")).unwrap();
            std::fs::write(dir.join("public").join("index.php"), "<?php").unwrap();
            std::fs::write(dir.join("artisan"), "").unwrap();
        }
        "php" => std::fs::write(dir.join("index.php"), "<?php").unwrap(),
        "static" => std::fs::write(dir.join("index.html"), "<p>").unwrap(),
        _ => {}
    }
}

#[test]
fn docroots_are_detected() {
    let d = tempfile::tempdir().unwrap();
    for kind in ["laravel", "php", "static", "empty"] {
        project(&d.path().join(kind), kind);
    }
    assert_eq!(docroot(&d.path().join("laravel")).unwrap(), d.path().join("laravel").join("public"));
    assert_eq!(docroot(&d.path().join("php")).unwrap(), d.path().join("php"));
    assert_eq!(docroot(&d.path().join("static")).unwrap(), d.path().join("static"));
    assert_eq!(docroot(&d.path().join("empty")).unwrap_err(), "No index.php or index.html found");
}

#[test]
fn every_subfolder_of_a_parked_folder_is_a_site() {
    let (d, p) = home();
    let projects = d.path().join("Projects");
    project(&projects.join("Shop"), "laravel");
    project(&projects.join("blog site"), "php");
    project(&projects.join("notes"), "static");
    project(&projects.join(".git"), "php");
    park(&p, &projects).unwrap();
    let sites = list(&p);
    let names: Vec<&str> = sites.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["blog-site", "notes", "shop"]);
    let shop = sites.iter().find(|s| s.name == "shop").unwrap();
    assert_eq!((shop.kind, shop.docroot.clone()), (SiteKind::Parked, Some(projects.join("Shop").join("public"))));
}

#[test]
fn a_link_wins_over_a_parked_folder_of_the_same_name() {
    let (d, p) = home();
    project(&d.path().join("Projects").join("shop"), "php");
    project(&d.path().join("elsewhere"), "laravel");
    park(&p, &d.path().join("Projects")).unwrap();
    link(&p, "shop", &d.path().join("elsewhere")).unwrap();
    let sites = list(&p);
    assert_eq!(sites.len(), 1);
    assert_eq!((sites[0].kind, sites[0].path.clone()), (SiteKind::Linked, Some(d.path().join("elsewhere"))));
}

#[test]
fn a_deleted_link_folder_is_listed_with_its_problem() {
    let (d, p) = home();
    project(&d.path().join("gone"), "php");
    link(&p, "gone", &d.path().join("gone")).unwrap();
    std::fs::remove_dir_all(d.path().join("gone")).unwrap();
    assert_eq!(list(&p)[0].problem.as_deref(), Some("Folder missing"));
}

#[test]
fn names_and_upstreams_are_checked() {
    let (d, p) = home();
    project(&d.path().join("x"), "php");
    assert!(link(&p, "api.shop", &d.path().join("x")).is_ok());
    for bad in ["Shop!", "", "a..b", "-shop", &"a".repeat(64)] {
        assert_eq!(
            link(&p, bad, &d.path().join("x")).unwrap_err().to_string(),
            "Use lowercase letters, digits and dashes for the site name"
        );
    }
    // Caddy takes scheme, host and port only; anything else could also inject Caddyfile text.
    for bad in [
        "ftp://localhost:21",
        "http://localhost:5173/app",
        "http://localhost:5173 {",
        "http://localhost:5173\n:80 {",
        "http://local host:5173",
        "http://:5173",
        "http://localhost:99999",
        "http://localhost:51x3",
    ] {
        assert_eq!(
            proxy(&p, "vite", bad).unwrap_err().to_string(),
            "Use an address like http://localhost:5173",
            "{bad:?}"
        );
    }
    for good in ["http://localhost", "https://127.0.0.1:8443", "http://[::1]:5173", "http://my-app.local:3000/"] {
        assert!(proxy(&p, "ok", good).is_ok(), "{good}");
    }
    proxy(&p, "vite", "http://localhost:5173").unwrap();
    let vite = list(&p).into_iter().find(|s| s.name == "vite").unwrap();
    assert_eq!((vite.kind, vite.upstream.as_deref()), (SiteKind::Proxy, Some("http://localhost:5173")));
}

#[test]
fn isolation_persists_and_unlink_drops_it() {
    let (d, p) = home();
    project(&d.path().join("shop"), "laravel");
    link(&p, "shop", &d.path().join("shop")).unwrap();
    isolate(&p, "shop", Some("8.3")).unwrap();
    assert_eq!(list(&p)[0].php.as_deref(), Some("8.3"));
    assert_eq!(isolate(&p, "nope", Some("8.3")).unwrap_err().to_string(), "There is no site nope");
    unlink(&p, "shop").unwrap();
    assert!(load(&p).isolated.is_empty());
    assert_eq!(unlink(&p, "shop").unwrap_err().to_string(), "shop is not a linked site");
}

#[test]
fn the_http_port_defaults_to_8080() {
    let (_d, p) = home();
    assert_eq!(load(&p).http_port, 8080);
    set_http_port(&p, 8081).unwrap();
    assert_eq!(load(&p).http_port, 8081);
}
