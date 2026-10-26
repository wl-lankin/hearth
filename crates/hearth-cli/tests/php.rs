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
