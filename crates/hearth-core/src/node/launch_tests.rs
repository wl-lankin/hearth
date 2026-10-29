use std::ffi::OsString;
use std::path::Path;

use serde_json::json;

use super::launch::launch;
use super::store::{node_program, set_default, version_dir};
use crate::php::service::BUILD_FILE;
use crate::HearthPaths;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, paths)
}

/// What a finished install leaves behind, without downloading anything.
fn fake_install(paths: &HearthPaths, version: &str, lts: Option<&str>, corepack: bool) {
    let dir = version_dir(paths, version);
    let lib = if cfg!(windows) { dir.clone() } else { dir.join("lib") };
    for (file, keep) in [
        (node_program(&dir), true),
        (lib.join("node_modules/npm/bin/npm-cli.js"), true),
        (lib.join("node_modules/npm/bin/npx-cli.js"), true),
        (lib.join("node_modules/corepack/dist/corepack.js"), corepack),
    ] {
        if keep {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b"x").unwrap();
        }
    }
    let build = json!({
        "major": version.split('.').next().unwrap().parse::<u32>().unwrap(), "version": version, "lts": lts,
        "os": "windows", "arch": "x64", "url": "u", "sha256": "0".repeat(64), "format": "zip", "root": "",
    });
    std::fs::write(dir.join(BUILD_FILE), build.to_string()).unwrap();
}

fn parts_join(base: &Path, parts: &[&str]) -> std::path::PathBuf {
    parts.iter().fold(base.to_path_buf(), |p, part| p.join(part))
}

#[test]
fn node_runs_the_version_the_project_asks_for() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1", Some("Jod"), true);
    fake_install(&paths, "20.19.5", Some("Iron"), true);
    set_default(&paths, Some("22.21.1")).unwrap();
    let project = d.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join(".nvmrc"), "20\n").unwrap();

    let l = launch(&paths, "node", &project.join("src")).unwrap();
    let v20 = version_dir(&paths, "20.19.5");
    assert_eq!(l.program, node_program(&v20));
    assert!(l.args.is_empty());
    assert_eq!(l.version, "20.19.5");
    // Scripts that start `node` again (npm run) get the same version, and its global tools.
    assert_eq!(l.path_first, super::store::bin_dir(&v20));

    let elsewhere = d.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    assert_eq!(launch(&paths, "node", &elsewhere).unwrap().version, "22.21.1", "the default without a file");
}

#[test]
fn npm_npx_and_corepack_run_their_scripts_with_that_node() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1", Some("Jod"), true);
    let dir = version_dir(&paths, "22.21.1");
    let lib = if cfg!(windows) { dir.clone() } else { dir.join("lib") };
    for (name, script) in [
        ("npm", ["npm", "bin", "npm-cli.js"]),
        ("npx", ["npm", "bin", "npx-cli.js"]),
        ("corepack", ["corepack", "dist", "corepack.js"]),
    ] {
        let script = parts_join(&lib.join("node_modules"), &script);
        let l = launch(&paths, name, d.path()).unwrap();
        assert_eq!(l.program, node_program(&dir), "{name}");
        assert_eq!(l.args, [OsString::from(script)], "{name}");
    }
}

#[test]
fn a_version_without_corepack_says_so() {
    let (d, paths) = home();
    fake_install(&paths, "25.9.0", None, false);
    set_default(&paths, Some("25.9.0")).unwrap();
    let err = launch(&paths, "corepack", d.path()).unwrap_err().to_string();
    assert_eq!(err, "Node 25.9.0 comes without corepack");
}

#[test]
fn nothing_installed_or_a_missing_version_is_an_error() {
    let (d, paths) = home();
    let err = launch(&paths, "node", d.path()).unwrap_err().to_string();
    assert_eq!(err, "No Node version is installed in Hearth");
    fake_install(&paths, "22.21.1", Some("Jod"), true);
    set_default(&paths, Some("22.21.1")).unwrap();
    std::fs::write(d.path().join(".node-version"), "v18.20.8").unwrap();
    let err = launch(&paths, "npm", d.path()).unwrap_err().to_string();
    let file = d.path().join(".node-version");
    assert_eq!(
        err,
        format!(
            "This project wants Node 18.20.8 ({}), which is not installed. Install it with: hearth node:install 18.20.8",
            file.display()
        )
    );
    assert!(launch(&paths, "nodemon", Path::new(".")).is_err());
}
