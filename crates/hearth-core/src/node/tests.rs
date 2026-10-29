use std::path::Path;

use super::install::{available, install, uninstall};
use super::store::{current_link, default_version, installed, node_program, set_default, version_dir};
use crate::php::manifest::{fetch, NodeBuild};
use crate::php::service_tests::{fixture, Fixture};

async fn build(f: &Fixture, major: u32) -> NodeBuild {
    let manifest = fetch(&f.core.paths, &f.core.catalog.url, &f.core.catalog.public_key).await.unwrap();
    available(&manifest).into_iter().find(|b| b.major == major).unwrap().clone()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    std::fs::canonicalize(a).ok().is_some() && std::fs::canonicalize(a).ok() == std::fs::canonicalize(b).ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_catalog_lists_the_builds_for_this_computer() {
    let f = fixture(false);
    let manifest = fetch(&f.core.paths, &f.core.catalog.url, &f.core.catalog.public_key).await.unwrap();
    let majors: Vec<u32> = available(&manifest).into_iter().map(|b| b.major).collect();
    assert_eq!(majors, [22, 21, 20], "newest first, another computer's build left out");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_version_installed_becomes_the_default() {
    let f = fixture(false);
    let p = &f.core.paths;
    let dir = install(&f.core, &build(&f, 22).await).await.unwrap();
    assert_eq!(dir, version_dir(p, "22.21.1"));
    assert_eq!(std::fs::read(node_program(&dir)).unwrap(), b"22.21.1", "the archive's top folder is stripped");
    let list = installed(p);
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].version.as_str(), list[0].lts.as_deref()), ("22.21.1", Some("Jod")));
    assert_eq!(default_version(p).as_deref(), Some("22.21.1"));
    assert!(same_dir(&current_link(p), &dir), "current points to the default");
    assert_eq!(std::fs::read_dir(p.data.join("node").join("downloads")).map(|d| d.count()).unwrap_or(0), 0);

    install(&f.core, &build(&f, 20).await).await.unwrap();
    assert_eq!(default_version(p).as_deref(), Some("22.21.1"), "a second install keeps the default");
    assert_eq!(installed(p).iter().map(|i| i.version.as_str()).collect::<Vec<_>>(), ["22.21.1", "20.19.5"]);
    set_default(p, Some("20.19.5")).unwrap();
    assert!(same_dir(&current_link(p), &version_dir(p, "20.19.5")), "current follows the default");
    assert!(set_default(p, Some("18.0.0")).unwrap_err().to_string().contains("Node 18.0.0 is not installed"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_download_leaves_nothing_behind() {
    let f = fixture(false);
    let p = &f.core.paths;
    let err = install(&f.core, &build(&f, 21).await).await.unwrap_err().to_string();
    assert!(err.contains("damaged") || err.contains("does not match"), "{err}");
    assert!(installed(p).is_empty());
    assert!(!version_dir(p, "21.7.3").exists());
    assert_eq!(default_version(p), None);
    assert!(std::fs::symlink_metadata(current_link(p)).is_err(), "no current link");
}

#[tokio::test(flavor = "multi_thread")]
async fn uninstalling_the_default_moves_it_to_the_newest_remaining_version() {
    let f = fixture(false);
    let p = &f.core.paths;
    install(&f.core, &build(&f, 20).await).await.unwrap();
    install(&f.core, &build(&f, 22).await).await.unwrap();
    assert_eq!(default_version(p).as_deref(), Some("20.19.5"));

    uninstall(&f.core, "20.19.5").await.unwrap();
    assert!(!version_dir(p, "20.19.5").exists());
    assert_eq!(default_version(p).as_deref(), Some("22.21.1"));
    assert!(same_dir(&current_link(p), &version_dir(p, "22.21.1")));

    uninstall(&f.core, "22.21.1").await.unwrap();
    assert_eq!(default_version(p), None);
    assert!(std::fs::symlink_metadata(current_link(p)).is_err(), "the current link is removed with the last version");
    assert_eq!(uninstall(&f.core, "22.21.1").await.unwrap_err().to_string(), "Node 22.21.1 is not installed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_default_whose_folder_is_gone_counts_as_none() {
    let f = fixture(false);
    let p = &f.core.paths;
    install(&f.core, &build(&f, 22).await).await.unwrap();
    std::fs::remove_dir_all(version_dir(p, "22.21.1")).unwrap();
    assert_eq!(default_version(p), None);
}
