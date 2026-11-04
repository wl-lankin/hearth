use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError};
use crate::processes::output::read;
use crate::processes::run::{key, log_path, spec_for};
use crate::processes::store::Proc;
use crate::HearthPaths;

#[test]
fn a_process_runs_in_the_platform_shell_in_its_folder_with_hearths_path() {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    let site = d.path().join("shop");
    let proc = Proc { name: "vite".into(), command: r#"npm run dev && echo "done""#.into(), autostart: false };
    let spec = spec_for(&paths, "shop", &site, &proc);
    assert_eq!(spec.key, key("shop", "vite"));
    assert_eq!(spec.key, "proc:shop:vite");
    assert_eq!(spec.cwd.as_deref(), Some(site.as_path()));
    assert_eq!(spec.log, log_path(&paths, "shop", "vite"));
    assert_eq!(spec.log, paths.logs.join("processes").join("shop").join("vite.log"));
    if cfg!(windows) {
        assert!(spec.program.to_string_lossy().to_lowercase().ends_with("cmd.exe"), "{:?}", spec.program);
        assert!(spec.raw_args, "cmd reads its own quoting");
        let args: Vec<String> = spec.args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["/d", "/s", "/c", r#""npm run dev && echo "done"""#]);
    } else {
        assert_eq!(spec.program, Path::new("/bin/sh"));
        let args: Vec<String> = spec.args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["-c", r#"npm run dev && echo "done""#]);
    }
    let path = spec.env.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v.to_string_lossy().into_owned()).unwrap();
    assert!(path.starts_with(&paths.bin.to_string_lossy().into_owned()), "{path}");
    assert!(spec.env.iter().any(|(k, v)| k == "NO_COLOR" && v == "1"));
}

#[test]
fn output_reads_new_text_from_an_offset_and_starts_over_after_a_reset() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("x.log");
    assert_eq!(read(&log, None).text, "", "no log yet");
    std::fs::write(&log, "\x1b[32mready\x1b[0m in 300 ms\n\x1b]0;title\x07next\n").unwrap();
    let first = read(&log, None);
    assert_eq!(first.text, "ready in 300 ms\nnext\n");
    assert!(!first.reset);
    std::fs::OpenOptions::new().append(true).open(&log).unwrap().write_all(b"more \xff\n").unwrap();
    let second = read(&log, Some(first.offset));
    assert_eq!(second.text, "more \u{fffd}\n");
    std::fs::write(&log, "fresh\n").unwrap();
    let third = read(&log, Some(second.offset));
    assert!(third.reset, "the log is shorter than the offset: emptied by a restart");
    assert_eq!(third.text, "fresh\n");
}

#[test]
fn a_long_log_returns_its_last_256_kb_from_a_line_start() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("x.log");
    let line = "y".repeat(99) + "\n";
    std::fs::write(&log, line.repeat(5000)).unwrap();
    let out = read(&log, None);
    assert!(out.text.len() <= 256 * 1024 && out.text.len() > 250 * 1024, "{}", out.text.len());
    assert!(out.text.starts_with('y') && out.text.lines().all(|l| l.len() == 99));
    assert_eq!(out.offset, 500_000);
}

use std::io::Write as _;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

fn php() -> Option<String> {
    std::env::var("HEARTH_TEST_PHP").ok().filter(|p| Path::new(p).is_file())
}

fn home() -> (tempfile::TempDir, CoreState) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    let shop = d.path().join("Projects").join("shop");
    std::fs::create_dir_all(&shop).unwrap();
    std::fs::write(shop.join("artisan"), "<?php").unwrap();
    crate::sites::registry::link(&paths, "shop", &shop).unwrap();
    (d, CoreState::new(paths))
}

async fn output(core: &CoreState) -> String {
    call(core, "processes.output", json!({ "site": "shop", "name": "app" })).await.unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn until(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check().await {
        assert!(Instant::now() < deadline, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn start_read_restart_and_stop_a_process() {
    let Some(php) = php() else {
        eprintln!("skipped: set HEARTH_TEST_PHP");
        return;
    };
    let (_d, core) = home();
    let command = format!(r#""{php}" -n -r "echo 'hi from '.basename(getcwd()).PHP_EOL; sleep(30);""#);
    call(&core, "processes.add", json!({ "site": "shop", "name": "app", "command": command, "autostart": false }))
        .await
        .unwrap();
    call(&core, "processes.start", json!({ "site": "shop", "name": "app" })).await.unwrap();
    until("the output arrives", async || output(&core).await.contains("hi from shop")).await;
    let list = call(&core, "processes.list", Value::Null).await.unwrap();
    let row = &list["sites"][0]["processes"][0];
    assert_eq!(
        (list["sites"][0]["site"].as_str(), row["name"].as_str(), row["running"].as_bool()),
        (Some("shop"), Some("app"), Some(true))
    );
    call(&core, "processes.restart", json!({ "site": "shop", "name": "app" })).await.unwrap();
    until("the restarted process writes again", async || output(&core).await.trim_end() == "hi from shop").await;
    call(&core, "processes.stop", json!({ "site": "shop", "name": "app" })).await.unwrap();
    let list = call(&core, "processes.list", Value::Null).await.unwrap();
    assert_eq!(list["sites"][0]["processes"][0]["running"], json!(false));
    core.supervisor.stop_all();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_that_fails_at_once_shows_why() {
    let (_d, core) = home();
    call(&core, "processes.add", json!({ "site": "shop", "name": "app", "command": "echo boom && exit 3" }))
        .await
        .unwrap();
    // Start says so itself (the CLI and the MCP have no page to look at).
    let err = call(&core, "processes.start", json!({ "site": "shop", "name": "app" })).await.unwrap_err();
    assert!(err.message.contains("boom"), "{}", err.message);
    let list = call(&core, "processes.list", Value::Null).await.unwrap();
    let row = &list["sites"][0]["processes"][0];
    assert_eq!((row["running"].as_bool(), row["waiting"].as_bool()), (Some(false), Some(true)), "{row}");
    let error = row["lastError"].as_str().unwrap();
    assert!(error.starts_with("exited") && error.contains("boom") && !error.contains("proc:"), "{error}");
    assert!(output(&core).await.contains("boom"));
    core.supervisor.stop_all();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_that_finishes_is_done_not_failed() {
    let (_d, core) = home();
    call(&core, "processes.add", json!({ "site": "shop", "name": "app", "command": "echo built" })).await.unwrap();
    call(&core, "processes.start", json!({ "site": "shop", "name": "app" })).await.unwrap();
    let list = call(&core, "processes.list", Value::Null).await.unwrap();
    let row = &list["sites"][0]["processes"][0];
    assert_eq!(
        (row["running"].as_bool(), row["waiting"].as_bool(), row["restarts"].as_u64()),
        (Some(false), Some(false), Some(0))
    );
    assert!(row["lastError"].is_null());
    assert!(output(&core).await.contains("built"));
}

#[tokio::test(flavor = "multi_thread")]
async fn output_only_reads_a_known_process_and_commands_are_one_line() {
    let (_d, core) = home();
    let err = call(&core, "processes.output", json!({ "site": "..", "name": "../../x" })).await.unwrap_err();
    assert_eq!(err.message, ".. has no process named ../../x");
    let err =
        call(&core, "processes.add", json!({ "site": "shop", "name": "x", "command": "a\nb" })).await.unwrap_err();
    assert_eq!(err.message, "The command must be one line");
}
#[tokio::test(flavor = "multi_thread")]
async fn unknown_processes_and_sites_are_refused() {
    let (_d, core) = home();
    let err = call(&core, "processes.start", json!({ "site": "shop", "name": "nope" })).await.unwrap_err();
    assert_eq!(err.message, "shop has no process named nope");
    let err = call(&core, "processes.add", json!({ "site": "gone", "name": "x", "command": "y" })).await.unwrap_err();
    assert_eq!(err.message, "No site named gone.test");
    let suggested = call(&core, "processes.suggest", json!({ "site": "shop" })).await.unwrap();
    assert_eq!(suggested[0]["name"], json!("queue"));
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_a_site_stops_and_forgets_its_processes() {
    let Some(php) = php() else {
        eprintln!("skipped: set HEARTH_TEST_PHP");
        return;
    };
    let (_d, core) = home();
    let command = format!(r#""{php}" -n -r "sleep(30);""#);
    call(&core, "processes.add", json!({ "site": "shop", "name": "app", "command": command })).await.unwrap();
    call(&core, "processes.start", json!({ "site": "shop", "name": "app" })).await.unwrap();
    assert!(core.supervisor.keys().contains(&key("shop", "app")));
    call(&core, "sites.unlink", json!({ "name": "shop" })).await.unwrap();
    assert!(!core.supervisor.keys().contains(&key("shop", "app")), "stopped");
    assert_eq!(call(&core, "processes.list", Value::Null).await.unwrap()["sites"], json!([]));
    core.supervisor.stop_all();
}
