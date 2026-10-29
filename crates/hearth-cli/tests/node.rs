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

async fn with_core<T: Send + 'static>(paths: &HearthPaths, f: impl FnOnce() -> T + Send + 'static) -> T {
    use hearth_core::ipc::{router, serve, CoreState};
    let server = serve(paths, router(CoreState::new(paths.clone()))).await.unwrap();
    let out = tokio::task::spawn_blocking(f).await.unwrap();
    server.shutdown().await;
    out
}

fn text(out: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[tokio::test(flavor = "multi_thread")]
async fn list_default_and_uninstall_through_the_core() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    fake_install(&paths, "20.19.5");
    set_default(&paths, Some("22.21.1")).unwrap();
    let (p, cwd) = (paths.clone(), d.path().to_path_buf());
    let outs = with_core(&paths, move || {
        ["node:list", "node:default 20", "node:list", "node:uninstall 20", "node:list", "node:default 18"]
            .map(|c| run(&hearth(), &p, &cwd, &c.split(' ').collect::<Vec<_>>()))
    })
    .await;
    let texts: Vec<String> = outs.iter().map(text).collect();
    assert_eq!(texts[0], "Node 22.21.1  default\nNode 20.19.5\n");
    assert_eq!(texts[1], "Node 20.19.5 is the default now.\n");
    assert_eq!(texts[2], "Node 22.21.1\nNode 20.19.5  default\n");
    assert_eq!(texts[3], "Node 20.19.5 is removed.\n");
    assert_eq!(texts[4], "Node 22.21.1  default\n");
    assert_eq!((outs[5].status.code(), texts[5].as_str()), (Some(1), "Node 18 is not installed\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn nvm_commands_are_hearths() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    fake_install(&paths, "20.19.5");
    set_default(&paths, Some("22.21.1")).unwrap();
    let nvm = d.path().join(format!("nvm{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(hearth(), &nvm).unwrap();
    let (p, cwd) = (paths.clone(), d.path().to_path_buf());
    let outs = with_core(&paths, move || {
        [
            run(&nvm, &p, &cwd, &["use", "20"]),
            run(&hearth(), &p, &cwd, &["nvm", "ls"]),
            run(&nvm, &p, &cwd, &["current"]),
            run(&nvm, &p, &cwd, &["alias", "x"]),
        ]
    })
    .await;
    let texts: Vec<String> = outs.iter().map(text).collect();
    assert_eq!(texts[0], "Node 20.19.5 is the default now.\n");
    assert_eq!(texts[1], "Node 22.21.1\nNode 20.19.5  default\n");
    assert_eq!(texts[2], "Node 20.19.5 (default)\n");
    assert_eq!(outs[3].status.code(), Some(1));
    assert_eq!(texts[3], "Hearth's nvm knows install, uninstall, use, list, current and which\n");
}

#[test]
fn node_which_says_why_without_the_core() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    fake_install(&paths, "20.19.5");
    set_default(&paths, Some("22.21.1")).unwrap();
    let cwd = project(&d, "20\n");
    let out = run(&hearth(), &paths, &cwd, &["node:which"]);
    assert_eq!(text(&out), format!("Node 20.19.5 (.nvmrc in {})\n", cwd.parent().unwrap().display()));
    let out = run(&hearth(), &paths, d.path(), &["node:which"]);
    assert_eq!(text(&out), "Node 22.21.1 (default)\n");
}
