use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::json;
use stoke_core::node::store::{bin_dir, node_program, set_default, version_dir};
use stoke_core::php::service::BUILD_FILE;
use stoke_core::StokePaths;

fn home() -> (tempfile::TempDir, StokePaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(dir.path().join("stoke"));
    paths.ensure().unwrap();
    (dir, paths)
}

fn fake_node() -> PathBuf {
    let cli = assert_cmd::cargo::cargo_bin("stoke");
    cli.parent().unwrap().join("examples").join(format!("fake_node{}", std::env::consts::EXE_SUFFIX))
}

/// An installed Node whose `node` is the fake program, with npm's script in place.
fn fake_install(paths: &StokePaths, version: &str) -> PathBuf {
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

fn run(program: &Path, paths: &StokePaths, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(program).env("STOKE_HOME", &paths.root).current_dir(cwd).args(args).output().unwrap()
}

fn stoke() -> PathBuf {
    assert_cmd::cargo::cargo_bin("stoke")
}

fn project(d: &tempfile::TempDir, nvmrc: &str) -> PathBuf {
    let dir = d.path().join("project");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join(".nvmrc"), nvmrc).unwrap();
    // The child sees its folder through symlinks resolved (macOS: /var is /private/var).
    #[cfg(unix)]
    let dir = dir.canonicalize().unwrap();
    dir.join("src")
}

#[test]
fn node_runs_the_projects_version_with_args_and_exit_code_passed_through() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    let v20 = fake_install(&paths, "20.19.5");
    set_default(&paths, Some("22.21.1")).unwrap();
    let cwd = project(&d, "v20\n");
    let out = run(&stoke(), &paths, &cwd, &["node", "-e", "x y", "--", "exit=7"]);
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
    let out = run(&stoke(), &paths, d.path(), &["npm", "run", "dev"]);
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
    std::fs::copy(stoke(), &copy).unwrap();
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
    let out = run(&stoke(), &paths, &cwd, &["node", "-v"]);
    assert_eq!(out.status.code(), Some(1));
    let file = cwd.parent().unwrap().join(".nvmrc");
    assert_eq!(
        String::from_utf8(out.stderr).unwrap().trim_end(),
        format!(
            "This project wants Node 18 ({}), which is not installed. Install it with: stoke node:install 18",
            file.display()
        )
    );
}

async fn with_core<T: Send + 'static>(paths: &StokePaths, f: impl FnOnce() -> T + Send + 'static) -> T {
    use stoke_core::ipc::{router, serve, CoreState};
    let server = serve(paths, router(CoreState::new(paths.clone()))).await.unwrap();
    let out = tokio::task::spawn_blocking(f).await.unwrap();
    server.shutdown().await;
    out
}

/// The catalog answers 404, but only after 12 s: longer than the CLI's usual 10 s for an answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_slow_lookup_ends_with_its_own_error_not_a_timeout() {
    use std::io::{Read, Write};
    use stoke_core::ipc::{router, serve, Catalog, CoreState};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            std::thread::spawn(move || {
                let _ = stream.read(&mut [0u8; 4096]);
                std::thread::sleep(std::time::Duration::from_secs(12));
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            });
        }
    });
    let (d, paths) = home();
    let mut core = CoreState::new(paths.clone());
    core.catalog = Catalog { url: format!("http://{addr}/manifest.json"), public_key: core.catalog.public_key.clone() };
    core.node_dist = format!("http://{addr}/dist");
    let server = serve(&paths, router(core)).await.unwrap();
    let (p, cwd) = (paths.clone(), d.path().to_path_buf());
    let out = tokio::task::spawn_blocking(move || run(&stoke(), &p, &cwd, &["node:install", "20.11.1"])).await.unwrap();
    server.shutdown().await;
    assert_eq!(out.status.code(), Some(1));
    let said = text(&out);
    assert!(said.contains("manifest.json") && said.contains("404"), "{said}");
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
            .map(|c| run(&stoke(), &p, &cwd, &c.split(' ').collect::<Vec<_>>()))
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
async fn nvm_commands_are_stokes() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    fake_install(&paths, "20.19.5");
    set_default(&paths, Some("22.21.1")).unwrap();
    let nvm = d.path().join(format!("nvm{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(stoke(), &nvm).unwrap();
    let (p, cwd) = (paths.clone(), d.path().to_path_buf());
    let outs = with_core(&paths, move || {
        [
            run(&nvm, &p, &cwd, &["use", "20"]),
            run(&stoke(), &p, &cwd, &["nvm", "ls"]),
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
    assert_eq!(texts[3], "Stoke's nvm knows install, uninstall, use, list, current and which\n");
}

#[test]
fn node_which_says_why_without_the_core() {
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    fake_install(&paths, "20.19.5");
    set_default(&paths, Some("22.21.1")).unwrap();
    let cwd = project(&d, "20\n");
    let out = run(&stoke(), &paths, &cwd, &["node:which"]);
    assert_eq!(text(&out), format!("Node 20.19.5 (.nvmrc in {})\n", cwd.parent().unwrap().display()));
    let out = run(&stoke(), &paths, d.path(), &["node:which"]);
    assert_eq!(text(&out), "Node 22.21.1 (default)\n");
}

/// Ctrl+C must reach the program: the launcher itself ignores it, but that setting must not be
/// handed on. Runs in its own hidden console, so the test runner gets no Ctrl+C.
#[cfg(windows)]
#[test]
fn ctrl_c_reaches_the_program_on_windows() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let (d, paths) = home();
    fake_install(&paths, "22.21.1");
    set_default(&paths, Some("22.21.1")).unwrap();
    let out = std::process::Command::new(fake_node())
        .env("STOKE_HOME", &paths.root)
        .current_dir(d.path())
        .arg("clean-console")
        .arg(stoke())
        .args(["node", "ctrlc"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("ctrl+c true"), "{text}");
}
