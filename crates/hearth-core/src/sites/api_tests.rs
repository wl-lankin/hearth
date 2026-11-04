use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError, INVALID_PARAMS};
use crate::HearthPaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

fn home() -> (tempfile::TempDir, CoreState, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    let projects = d.path().join("Projects");
    std::fs::create_dir_all(projects.join("shop").join("public")).unwrap();
    std::fs::write(projects.join("shop").join("public").join("index.php"), "<?php").unwrap();
    (d, CoreState::new(paths), projects)
}

#[tokio::test(flavor = "multi_thread")]
async fn park_link_proxy_and_list() {
    let (_d, core, projects) = home();
    call(&core, "sites.park", json!({ "dir": projects })).await.unwrap();
    call(&core, "sites.link", json!({ "name": "api.shop", "dir": projects.join("shop") })).await.unwrap();
    call(&core, "sites.proxy", json!({ "name": "vite", "upstream": "http://localhost:5173" })).await.unwrap();
    call(&core, "sites.isolate", json!({ "name": "shop", "minor": "8.3" })).await.unwrap();
    let list = call(&core, "sites.list", Value::Null).await.unwrap();
    let names: Vec<&str> = list["sites"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["api.shop", "shop", "vite"]);
    assert_eq!(list["sites"][1]["php"], "8.3");
    assert_eq!(list["httpPort"], 8080);
    assert_eq!(list["parked"][0], projects.to_string_lossy().as_ref());
    call(&core, "sites.isolate", json!({ "name": "shop", "minor": null })).await.unwrap();
    call(&core, "sites.unlink", json!({ "name": "api.shop" })).await.unwrap();
    call(&core, "sites.unproxy", json!({ "name": "vite" })).await.unwrap();
    call(&core, "sites.unpark", json!({ "dir": projects })).await.unwrap();
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["sites"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_port_and_the_web_status_are_reachable() {
    let (_d, core, _projects) = home();
    call(&core, "sites.port", json!({ "port": 8081 })).await.unwrap();
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["httpPort"], 8081);
    let status = call(&core, "web.status", Value::Null).await.unwrap();
    assert_eq!(status["httpPort"], 8081);
    assert_eq!(status["running"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_params_and_site_errors() {
    let (_d, core, _projects) = home();
    for (method, params) in [
        ("sites.park", json!({})),
        ("sites.link", json!({ "name": "x" })),
        ("sites.port", json!({ "port": "80" })),
        ("sites.port", json!({ "port": 70000 })),
        ("sites.isolate", json!({ "name": "x", "minor": 8.4 })),
    ] {
        assert_eq!(call(&core, method, params).await.unwrap_err().code, INVALID_PARAMS, "{method}");
    }
    let err = call(&core, "sites.unlink", json!({ "name": "nope" })).await.unwrap_err();
    assert_eq!(err.message, "nope is not a linked site");
}

#[tokio::test(flavor = "multi_thread")]
async fn sites_can_be_secured_and_the_status_shows_https() {
    let (_d, core, projects) = home();
    call(&core, "sites.link", json!({ "name": "shop", "dir": projects.join("shop") })).await.unwrap();
    call(&core, "sites.secure", json!({ "name": "shop" })).await.unwrap();
    let list = call(&core, "sites.list", Value::Null).await.unwrap();
    assert_eq!(list["sites"][0]["secured"], true);
    assert_eq!(list["httpsPort"], 8443);
    call(&core, "sites.unsecure", json!({ "name": "shop" })).await.unwrap();
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["sites"][0]["secured"], false);
    let status = call(&core, "web.status", Value::Null).await.unwrap();
    assert_eq!(status["httpsPort"], 8443);
    assert_eq!(status["caTrusted"], false);
    assert_eq!(status["dns"], json!({ "ready": false, "problem": null }));
    assert_eq!(call(&core, "sites.secure", json!({})).await.unwrap_err().code, INVALID_PARAMS);
    let err = call(&core, "sites.secure", json!({ "name": "nope" })).await.unwrap_err();
    assert_eq!(err.message, "No site named nope.test");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_test_home_never_sets_up_system_names() {
    let (_d, core, _projects) = home();
    let err = call(&core, "dns.setup", Value::Null).await.unwrap_err();
    assert_eq!(err.message, "This Hearth home does not manage .test names");
}

/// A minimal Herd folder: no parked paths, one proxy.
fn herd_folder(dir: &std::path::Path) -> std::path::PathBuf {
    let valet = dir.join("valet");
    std::fs::create_dir_all(valet.join("Nginx")).unwrap();
    std::fs::write(valet.join("config.json"), r#"{ "paths": [] }"#).unwrap();
    std::fs::write(
        valet.join("Nginx").join("vite.test.conf"),
        "location / {\n    proxy_pass http://localhost:5173;\n}\n",
    )
    .unwrap();
    valet
}

#[tokio::test(flavor = "multi_thread")]
async fn a_herd_import_is_planned_first_and_applied_on_request() {
    let (d, core, _projects) = home();
    let valet = herd_folder(d.path());
    let planned = call(&core, "herd.import", json!({ "from": valet, "apply": false })).await.unwrap();
    assert_eq!(planned["plan"]["proxy"], json!([["vite", "http://localhost:5173"]]));
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["sites"], json!([]));
    let applied = call(&core, "herd.import", json!({ "from": valet, "apply": true })).await.unwrap();
    assert_eq!(applied["failed"], json!([]));
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["sites"][0]["upstream"], "http://localhost:5173");
    let err = call(&core, "herd.import", json!({ "from": d.path().join("nope"), "apply": false })).await.unwrap_err();
    assert!(err.message.ends_with("is not a Herd folder"), "{}", err.message);
}

/// An installed PHP 8.4 (only its php.ini is written here), with or without Xdebug in its build.
fn fake_php(paths: &HearthPaths, with_xdebug: bool) {
    use crate::php::manifest::Os;
    let dir = crate::php::store::install_dir(paths, "8.4");
    std::fs::create_dir_all(&dir).unwrap();
    let extensions = if with_xdebug {
        json!({ "xdebug": { "kind": "zend_dll", "version": "3.5.3", "url": "u", "sha256": "s", "file": "php_xdebug.dll" } })
    } else {
        json!({})
    };
    let build = json!({
        "minor": "8.4", "version": "8.4.27", "channel": "stable", "os": Os::current(), "arch": "x64",
        "url": "u", "sha256": "0".repeat(64), "format": "zip", "root": "", "extensions": extensions
    });
    std::fs::write(dir.join(crate::php::service::BUILD_FILE), build.to_string()).unwrap();
    std::fs::write(dir.join(crate::php::store::VERSION_FILE), "8.4.27").unwrap();
    let names: Vec<String> = if with_xdebug { vec!["xdebug".into()] } else { vec![] };
    crate::php::store::register(paths, "8.4", &names).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn debugging_a_site_switches_xdebug_on_for_its_php() {
    let (_d, core, projects) = home();
    fake_php(&core.paths, true);
    call(&core, "sites.link", json!({ "name": "shop", "dir": projects.join("shop") })).await.unwrap();
    call(&core, "sites.debug", json!({ "name": "shop", "on": true })).await.unwrap();
    let ini = std::fs::read_to_string(crate::php::ini::ini_dir(&core.paths, "8.4").join("php.ini")).unwrap();
    assert!(ini.contains("zend_extension=xdebug") || ini.contains("php_xdebug"), "{ini}");
    let list = call(&core, "sites.list", Value::Null).await.unwrap();
    assert_eq!(list["sites"][0]["debug"], json!(true));
    call(&core, "sites.debug", json!({ "name": "shop", "on": false })).await.unwrap();
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["sites"][0]["debug"], json!(false));
    assert!(call(&core, "sites.debug", json!({ "name": "shop" })).await.is_err());
}

/// Xdebug cannot see the trigger itself (it is a FastCGI param and `$_ENV` stays empty): Hearth's
/// prepend script starts the session, so php.ini loads it while a site is debugged.
#[tokio::test(flavor = "multi_thread")]
async fn a_debugged_site_loads_the_prepend_script_even_with_dumps_off() {
    let (_d, core, projects) = home();
    fake_php(&core.paths, true);
    let off = crate::insights::collector::CollectorConfig { dumps: false, requests: false, ..Default::default() };
    crate::insights::collector::set_config(&core.paths, &off).unwrap();
    let ini = || std::fs::read_to_string(crate::php::ini::ini_dir(&core.paths, "8.4").join("php.ini")).unwrap();
    call(&core, "sites.link", json!({ "name": "shop", "dir": projects.join("shop") })).await.unwrap();
    call(&core, "sites.debug", json!({ "name": "shop", "on": true })).await.unwrap();
    assert!(ini().contains("auto_prepend_file"), "{}", ini());
    call(&core, "sites.debug", json!({ "name": "shop", "on": false })).await.unwrap();
    assert!(!ini().contains("auto_prepend_file"), "{}", ini());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_php_without_xdebug_is_a_clear_error_and_nothing_changes() {
    let (_d, core, projects) = home();
    fake_php(&core.paths, false);
    call(&core, "sites.link", json!({ "name": "shop", "dir": projects.join("shop") })).await.unwrap();
    let err = call(&core, "sites.debug", json!({ "name": "shop", "on": true })).await.unwrap_err();
    assert!(err.message.contains("no xdebug"), "{}", err.message);
    assert_eq!(call(&core, "sites.list", Value::Null).await.unwrap()["sites"][0]["debug"], json!(false));
    call(&core, "sites.proxy", json!({ "name": "vite", "upstream": "http://localhost:5173" })).await.unwrap();
    assert!(call(&core, "sites.debug", json!({ "name": "vite", "on": true })).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn debug_status_reports_a_listener() {
    let (_d, core, _projects) = home();
    let status = call(&core, "debug.status", Value::Null).await.unwrap();
    assert_eq!(status["port"], json!(9003));
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let mine = call(&core, "debug.status", json!({ "port": port })).await.unwrap();
    assert_eq!(mine["listening"], json!(true));
    let name = std::env::current_exe().unwrap().file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(mine["program"], json!(name));
    drop(socket);
    assert_eq!(call(&core, "debug.status", json!({ "port": port })).await.unwrap()["listening"], json!(false));
}

/// A site isolated to a removed PHP is served on the default one: that one gets Xdebug.
#[tokio::test(flavor = "multi_thread")]
async fn a_site_on_a_removed_php_debugs_on_the_one_it_runs_on() {
    let (_d, core, projects) = home();
    fake_php(&core.paths, true);
    call(&core, "sites.link", json!({ "name": "shop", "dir": projects.join("shop") })).await.unwrap();
    call(&core, "sites.isolate", json!({ "name": "shop", "minor": "8.3" })).await.unwrap();
    call(&core, "sites.debug", json!({ "name": "shop", "on": true })).await.unwrap();
    let ini = std::fs::read_to_string(crate::php::ini::ini_dir(&core.paths, "8.4").join("php.ini")).unwrap();
    assert!(ini.contains("zend_extension=xdebug"), "{ini}");
}
