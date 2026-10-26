use assert_cmd::Command;
use hearth_core::ipc::{router, serve, CoreState};
use hearth_core::php::service::BUILD_FILE;
use hearth_core::php::store::{self, install_dir, VERSION_FILE};
use hearth_core::HearthPaths;
use serde_json::json;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    (dir, paths)
}

/// What a finished install leaves behind, without downloading anything.
fn fake_install(paths: &HearthPaths, minor: &str, version: &str) {
    let dir = install_dir(paths, minor);
    std::fs::create_dir_all(&dir).unwrap();
    let build = json!({
        "minor": minor, "version": version, "channel": "stable", "os": "windows", "arch": "x64",
        "url": "https://example.test/php.zip", "sha256": "0".repeat(64), "format": "zip", "root": "",
        "extensions": {
            "redis": { "kind": "dll", "version": "6.3.0", "url": "u", "sha256": "s", "file": "php_redis.dll" },
            "xdebug": { "kind": "zend_dll", "version": "3.5.3", "url": "u", "sha256": "s", "file": "php_xdebug.dll" }
        }
    });
    std::fs::write(dir.join(BUILD_FILE), build.to_string()).unwrap();
    std::fs::write(dir.join(VERSION_FILE), version).unwrap();
    store::register(paths, minor, &["redis".into(), "xdebug".into()]).unwrap();
}

fn hearth(paths: &HearthPaths, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("hearth").unwrap().env("HEARTH_HOME", &paths.root).args(args).output().unwrap()
}

async fn with_core<T: Send + 'static>(paths: &HearthPaths, f: impl FnOnce() -> T + Send + 'static) -> T {
    let server = serve(paths, router(CoreState::new(paths.clone()))).await.unwrap();
    let out = tokio::task::spawn_blocking(f).await.unwrap();
    server.shutdown().await;
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn php_list_shows_installs_and_the_default() {
    let (_d, paths) = home();
    fake_install(&paths, "8.4", "8.4.27");
    fake_install(&paths, "8.3", "8.3.36");
    let p = paths.clone();
    let out = with_core(&paths, move || hearth(&p, &["php:list"])).await;
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "PHP 8.4.27  default  redis on, xdebug off\nPHP 8.3.36           redis on, xdebug off\n"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn php_list_json_is_the_ipc_result() {
    let (_d, paths) = home();
    fake_install(&paths, "8.4", "8.4.27");
    let p = paths.clone();
    let out = with_core(&paths, move || hearth(&p, &["php:list", "--json"])).await;
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["default"], "8.4");
}

#[tokio::test(flavor = "multi_thread")]
async fn php_use_of_a_missing_version_exits_1() {
    let (_d, paths) = home();
    let p = paths.clone();
    let out = with_core(&paths, move || hearth(&p, &["php:use", "9.9"])).await;
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8(out.stderr).unwrap(), "PHP 9.9 is not installed\n");
}

#[tokio::test(flavor = "multi_thread")]
async fn php_ext_switches_an_extension() {
    let (_d, paths) = home();
    fake_install(&paths, "8.4", "8.4.27");
    let p = paths.clone();
    let out = with_core(&paths, move || hearth(&p, &["php:ext", "8.4", "xdebug", "on"])).await;
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(store::load(&paths).extensions["8.4"]["xdebug"]);
}

#[test]
fn php_commands_need_the_core() {
    let (_d, paths) = home();
    let out = hearth(&paths, &["php:list"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8(out.stderr).unwrap(), "Hearth core is not running. Start the Hearth app.\n");
}

#[test]
fn running_php_without_any_install_explains_what_to_do() {
    let (_d, paths) = home();
    let out = hearth(&paths, &["php", "-v"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "No PHP version is installed. Install one with: hearth php:install 8.4\n"
    );
}

/// The fake PHP from the examples, put where an installed version's PHP lives.
fn fake_php(paths: &HearthPaths, minor: &str) {
    fake_install(paths, minor, &format!("{minor}.0"));
    let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    let name = if cfg!(windows) { "fake_php.exe" } else { "fake_php" };
    let target = if cfg!(windows) {
        install_dir(paths, minor).join("php.exe")
    } else {
        std::fs::create_dir_all(install_dir(paths, minor).join("bin")).unwrap();
        install_dir(paths, minor).join("bin").join("php")
    };
    std::fs::copy(deps.parent().unwrap().join("examples").join(name), target).unwrap();
}

fn hearth_in(paths: &HearthPaths, cwd: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("hearth").unwrap().env("HEARTH_HOME", &paths.root).current_dir(cwd).args(args).output().unwrap()
}

fn projects(dir: &std::path::Path) -> std::path::PathBuf {
    let projects = dir.join("Projects");
    for name in ["shop", "blog"] {
        std::fs::create_dir_all(projects.join(name).join("public")).unwrap();
        std::fs::write(projects.join(name).join("public").join("index.php"), "<?php").unwrap();
        std::fs::create_dir_all(projects.join(name).join("app")).unwrap();
    }
    projects
}

fn ran_version(out: &std::process::Output) -> String {
    let text = String::from_utf8_lossy(&out.stdout);
    let exe = text.lines().find_map(|l| l.strip_prefix("exe:")).unwrap_or_default().replace('\\', "/");
    exe.split("/versions/").nth(1).and_then(|r| r.split('/').next()).unwrap_or_default().to_string()
}

#[test]
fn php_runs_the_version_of_the_project_you_are_in() {
    let (d, paths) = home();
    fake_php(&paths, "8.3");
    fake_php(&paths, "8.4");
    store::set_default(&paths, "8.4").unwrap();
    let projects = projects(d.path());
    hearth_core::sites::registry::park(&paths, &projects).unwrap();
    hearth_core::sites::registry::isolate(&paths, "shop", Some("8.3")).unwrap();
    assert_eq!(ran_version(&hearth_in(&paths, &projects.join("shop").join("app"), &["php", "-v"])), "8.3");
    assert_eq!(ran_version(&hearth_in(&paths, &projects.join("blog"), &["php", "-v"])), "8.4");
    assert_eq!(ran_version(&hearth_in(&paths, &projects.join("blog"), &["php", "--site=shop", "-v"])), "8.3");
}

#[test]
fn arguments_and_the_exit_code_pass_through_untouched() {
    let (d, paths) = home();
    fake_php(&paths, "8.4");
    store::set_default(&paths, "8.4").unwrap();
    let out = hearth_in(&paths, d.path(), &["php", "pkg:^1.2", "--filter=A|B", "two words", "say \"hi\"", "--exit=7"]);
    assert_eq!(out.status.code(), Some(7));
    let text = String::from_utf8_lossy(&out.stdout);
    for expected in ["arg:pkg:^1.2", "arg:--filter=A|B", "arg:two words", "arg:say \"hi\""] {
        assert!(text.lines().any(|l| l == expected), "{expected} missing in:\n{text}");
    }
    assert!(
        text.lines().any(|l| l.starts_with("phprc:") && l.replace('\\', "/").ends_with("config/php/8.4")),
        "{text}"
    );
}

#[test]
fn composer_uses_the_projects_php_too() {
    let (d, paths) = home();
    fake_php(&paths, "8.3");
    fake_php(&paths, "8.4");
    store::set_default(&paths, "8.4").unwrap();
    let projects = projects(d.path());
    hearth_core::sites::registry::link(&paths, "shop", &projects.join("shop")).unwrap();
    hearth_core::sites::registry::isolate(&paths, "shop", Some("8.3")).unwrap();
    let out = hearth_in(&paths, &projects.join("shop"), &["composer", "require", "psr/log:^3.0"]);
    assert_eq!(ran_version(&out), "8.3");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.lines().any(|l| l == "arg:psr/log:^3.0"), "{text}");
}
