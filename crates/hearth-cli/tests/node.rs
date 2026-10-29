use std::path::{Path, PathBuf};

use assert_cmd::Command;
use hearth_core::node::store::{bin_dir, node_program, set_default, version_dir};
use hearth_core::php::service::BUILD_FILE;
use hearth_core::HearthPaths;
use serde_json::json;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    (dir, paths)
}

fn fake_node() -> PathBuf {
    let cli = assert_cmd::cargo::cargo_bin("hearth");
    cli.parent().unwrap().join("examples").join(format!("fake_node{}", std::env::consts::EXE_SUFFIX))
}

/// An installed Node whose `node` is the fake program, with npm's script in place.
fn fake_install(paths: &HearthPaths, version: &str) -> PathBuf {
    let dir = version_dir(paths, version);
    let program = node_program(&dir);
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::copy(fake_node(), &program).unwrap();
    let lib = if cfg!(windows) { dir.clone() } else { dir.join("lib") };
    let npm = lib.join("node_modules").join("npm").join("bin");
    std::fs::create_dir_all(&npm).unwrap();
    std::fs::write(npm.join("npm-cli.js"), "").unwrap();
    let build = json!({
        "major": 1, "version": version, "lts": null, "os": "windows", "arch": "x64",
        "url": "u", "sha256": "0".repeat(64), "format": "zip", "root": "",
    });
    std::fs::write(dir.join(BUILD_FILE), build.to_string()).unwrap();
    dir
}

fn run(program: &Path, paths: &HearthPaths, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(program).env("HEARTH_HOME", &paths.root).current_dir(cwd).args(args).output().unwrap()
}

fn hearth() -> PathBuf {
    assert_cmd::cargo::cargo_bin("hearth")
}

fn project(d: &tempfile::TempDir, nvmrc: &str) -> PathBuf {
    let dir = d.path().join("project");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join(".nvmrc"), nvmrc).unwrap();
    dir.join("src")
}

#[test]
fn node_runs_the_projects_version_with_args_and_exit_code_passed_through() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    let v20 = fake_install(&paths, "20.19.5");
    set_default(&paths, Some("22.21.1")).unwrap();
    let cwd = project(&d, "v20\n");
    let out = run(&hearth(), &paths, &cwd, &["node", "-e", "x y", "--", "exit=7"]);
    assert_eq!(out.status.code(), Some(7), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("version v20.19.5\nargs -e|x y|--|exit=7\npath {}\n", bin_dir(&v20).display())
    );
}

#[test]
fn npm_runs_its_script_with_the_same_node() {
    let (d, paths) = home();
    let v22 = fake_install(&paths, "22.21.1");
    set_default(&paths, Some("22.21.1")).unwrap();
    let out = run(&hearth(), &paths, d.path(), &["npm", "run", "dev"]);
    let text = String::from_utf8(out.stdout).unwrap();
    let lib = if cfg!(windows) { v22.clone() } else { v22.join("lib") };
    let script = lib.join("node_modules").join("npm").join("bin").join("npm-cli.js");
    assert!(text.contains(&format!("args {}|run|dev", script.display())), "{text}");
}

#[test]
fn copied_under_a_node_name_the_cli_answers_to_it() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    set_default(&paths, Some("22.21.1")).unwrap();
    let copy = d.path().join(format!("node{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(hearth(), &copy).unwrap();
    let out = run(&copy, &paths, d.path(), &["--version"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("version v22.21.1"), "{text}");
    assert!(text.contains("args --version"), "{text}");
}

#[test]
fn a_missing_version_exits_1_with_the_install_command() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    set_default(&paths, Some("22.21.1")).unwrap();
    let cwd = project(&d, "18\n");
    let out = run(&hearth(), &paths, &cwd, &["node", "-v"]);
    assert_eq!(out.status.code(), Some(1));
    let file = cwd.parent().unwrap().join(".nvmrc");
    assert_eq!(
        String::from_utf8(out.stderr).unwrap().trim_end(),
        format!(
            "This project wants Node 18 ({}), which is not installed. Install it with: hearth node:install 18",
            file.display()
        )
    );
}
