use std::path::Path;

use crate::processes::store::{self, Proc};
use crate::processes::suggest;
use crate::HearthPaths;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, paths)
}

fn proc(name: &str, command: &str) -> Proc {
    Proc { name: name.into(), command: command.into(), autostart: false }
}

#[test]
fn processes_are_kept_per_site_and_names_are_checked() {
    let (_d, p) = home();
    store::add(&p, "shop", proc("vite", "npm run dev")).unwrap();
    store::add(&p, "shop", Proc { autostart: true, ..proc("queue", "php artisan queue:listen --tries=1") }).unwrap();
    store::add(&p, "blog", proc("vite", "npm run dev")).unwrap();
    let all = store::load(&p);
    assert_eq!(all["shop"].iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["vite", "queue"]);
    assert!(all["shop"][1].autostart);
    let dup = store::add(&p, "shop", proc("vite", "npx vite")).unwrap_err().to_string();
    assert_eq!(dup, "shop already has a process named vite");
    for bad in ["Vite", "a b", "", &"x".repeat(33)] {
        let e = store::add(&p, "shop", proc(bad, "x")).unwrap_err().to_string();
        assert_eq!(e, "Use 1 to 32 lowercase letters, digits or dashes for the name", "{bad:?}");
    }
    assert_eq!(store::add(&p, "shop", proc("x", "  ")).unwrap_err().to_string(), "The command is empty");
    store::remove(&p, "shop", "vite").unwrap();
    assert_eq!(store::load(&p)["shop"].len(), 1);
    assert!(store::remove(&p, "shop", "vite").is_err());
    store::forget_site(&p, "shop").unwrap();
    assert!(!store::load(&p).contains_key("shop"));
    assert!(store::load(&p).contains_key("blog"));
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

#[test]
fn a_laravel_project_suggests_its_dev_processes_in_order() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "artisan", "<?php");
    write(d.path(), "package.json", r#"{"scripts":{"dev":"vite","build":"vite build"}}"#);
    write(
        d.path(),
        "composer.json",
        r#"{"scripts":{"dev":["Composer\\Config::disableProcessTimeout","npx concurrently ..."]}}"#,
    );
    write(
        d.path(),
        "composer.lock",
        r#"{"packages":[{"name":"laravel/framework"},{"name":"laravel/reverb"}],"packages-dev":[{"name":"laravel/pail"}]}"#,
    );
    let names: Vec<(String, String)> = suggest::for_dir(d.path()).into_iter().map(|x| (x.name, x.command)).collect();
    let want = [
        ("vite", "npm run dev"),
        ("dev", "composer run dev"),
        ("queue", "php artisan queue:listen --tries=1"),
        ("schedule", "php artisan schedule:work"),
        ("pail", "php artisan pail"),
        ("reverb", "php artisan reverb:start"),
    ];
    assert_eq!(names, want.map(|(a, b)| (a.to_string(), b.to_string())));
}

#[test]
fn a_plain_folder_suggests_nothing_and_broken_files_are_ignored() {
    let d = tempfile::tempdir().unwrap();
    assert!(suggest::for_dir(d.path()).is_empty());
    write(d.path(), "package.json", "{ not json");
    write(d.path(), "composer.lock", "[]");
    assert!(suggest::for_dir(d.path()).is_empty());
    assert!(suggest::for_dir(&d.path().join("missing")).is_empty());
}
