use std::path::PathBuf;

use serde_json::{json, Value};

use crate::db::test_server::TestServer;
use crate::ipc::{router, CoreState, RpcError};
use crate::StokePaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

fn home(env: &str) -> (tempfile::TempDir, CoreState, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(d.path().join("stoke"));
    paths.ensure().unwrap();
    let shop = d.path().join("shop");
    std::fs::create_dir_all(shop.join("public")).unwrap();
    std::fs::create_dir_all(shop.join("database")).unwrap();
    std::fs::write(shop.join("public").join("index.php"), "<?php").unwrap();
    std::fs::write(shop.join(".env"), env).unwrap();
    crate::sites::registry::link(&paths, "shop", &shop).unwrap();
    let mut core = CoreState::new(paths);
    core.web_sync = false;
    (d, core, shop)
}

fn env_db(shop: &std::path::Path) -> Option<String> {
    crate::dotenv::get(&std::fs::read_to_string(shop.join(".env")).unwrap(), "DB_DATABASE")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_connection_comes_from_the_env() {
    let env = "DB_CONNECTION=mariadb\nDB_HOST=10.0.0.5\nDB_PORT=3310\nDB_DATABASE=shop\nDB_USERNAME=app\nDB_PASSWORD='s3 cret'\n";
    let (_d, core, _shop) = home(env);
    let c = call(&core, "db.connection", json!({ "site": "shop" })).await.unwrap();
    assert_eq!(
        c,
        json!({ "driver": "mysql", "host": "10.0.0.5", "port": 3310, "username": "app", "password": "s3 cret", "database": "shop" })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn without_db_connection_it_is_sqlite_and_other_drivers_are_named() {
    let (_d, core, shop) = home("APP_NAME=Shop\n");
    let c = call(&core, "db.connection", json!({ "site": "shop" })).await.unwrap();
    assert_eq!(c["driver"], "sqlite");
    assert_eq!(PathBuf::from(c["database"].as_str().unwrap()), shop.join("database").join("database.sqlite"));

    // A server without DB_DATABASE uses Laravel's default database, `laravel`: never dropped.
    std::fs::write(shop.join(".env"), "DB_CONNECTION=mysql\nDB_PORT=1\n").unwrap();
    let err = call(&core, "db.drop", json!({ "site": "shop", "name": "laravel" })).await.unwrap_err();
    assert!(err.message.contains("uses `laravel`"), "{}", err.message);

    std::fs::write(shop.join(".env"), "DB_CONNECTION=sqlsrv\nDB_DATABASE=shop\n").unwrap();
    for method in ["db.connection", "db.list", "db.create"] {
        let err = call(&core, method, json!({ "site": "shop" })).await.unwrap_err();
        assert!(err.message.contains("sqlsrv") && err.message.contains("MySQL"), "{method}: {}", err.message);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_databases_are_files_in_the_database_folder() {
    let (_d, core, shop) = home("DB_CONNECTION=sqlite\n");
    let made = call(&core, "db.create", json!({ "site": "shop", "name": "reports" })).await.unwrap();
    assert_eq!(made["created"], true);
    assert!(shop.join("database").join("reports.sqlite").is_file());
    let again = call(&core, "db.create", json!({ "site": "shop", "name": "reports" })).await.unwrap();
    assert_eq!(again["created"], false);

    call(&core, "db.use", json!({ "site": "shop", "name": "archive", "create": true })).await.unwrap();
    let archive = shop.join("database").join("archive.sqlite");
    assert!(archive.is_file());
    assert_eq!(env_db(&shop).map(PathBuf::from), Some(archive.clone()));

    let list = call(&core, "db.list", json!({ "site": "shop" })).await.unwrap();
    assert_eq!(list["databases"], json!(["archive", "reports"]));
    assert_eq!(list["current"], "archive");

    for current in ["Archive", "archive.sqlite", "ARCHIVE.sqlite"] {
        let err = call(&core, "db.drop", json!({ "site": "shop", "name": current })).await.unwrap_err();
        assert!(err.message.contains("uses"), "{current}: {}", err.message);
    }
    // The same file under another spelling of its path is still the current one.
    let other_spelling = shop.join("database").join("..").join("database").join("archive.sqlite");
    let env = std::fs::read_to_string(shop.join(".env")).unwrap();
    let env = crate::dotenv::set(&env, &[("DB_DATABASE", &other_spelling.to_string_lossy())]);
    std::fs::write(shop.join(".env"), env).unwrap();
    let err = call(&core, "db.drop", json!({ "site": "shop", "name": "archive" })).await.unwrap_err();
    assert!(err.message.contains("uses"), "{}", err.message);
    assert!(archive.is_file());
    let err = call(&core, "db.create", json!({ "site": "shop", "name": "two\nlines" })).await.unwrap_err();
    assert!(err.message.contains("not a database name"), "{}", err.message);
    call(&core, "db.drop", json!({ "site": "shop", "name": "reports" })).await.unwrap();
    assert!(!shop.join("database").join("reports.sqlite").exists() && archive.is_file());
    let err = call(&core, "db.drop", json!({ "site": "shop", "name": "../.env" })).await.unwrap_err();
    assert!(err.message.contains("../.env"), "{}", err.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_lists_creates_switches_and_drops_but_never_the_current_one() {
    let Some(server) = TestServer::start() else {
        eprintln!("skipped: set STOKE_TEST_MYSQLD to a mysqld or mariadbd");
        return;
    };
    let env = format!(
        "DB_CONNECTION=mariadb\nDB_HOST=127.0.0.1\nDB_PORT={}\nDB_DATABASE=shop\nDB_USERNAME=root\nDB_PASSWORD=\n",
        server.port
    );
    let (_d, core, shop) = home(&env);
    server.sql("CREATE DATABASE shop; CREATE DATABASE `old stuff`;");
    let made = call(&core, "db.create", json!({ "site": "shop", "name": "shop_testing" })).await.unwrap();
    assert_eq!(made["created"], true);
    let list = call(&core, "db.list", json!({ "site": "shop" })).await.unwrap();
    assert_eq!(list["databases"], json!(["old stuff", "shop", "shop_testing"]), "no system schemas");
    assert_eq!(list["current"], "shop");

    call(&core, "db.use", json!({ "site": "shop", "name": "shop_next", "create": true })).await.unwrap();
    assert_eq!(env_db(&shop).as_deref(), Some("shop_next"));
    assert!(server.sql("SHOW DATABASES").lines().any(|l| l == "shop_next"));
    let err = call(&core, "db.use", json!({ "site": "shop", "name": "bad\nname", "create": true })).await.unwrap_err();
    assert!(err.message.contains("not a database name"), "{}", err.message);
    let err = call(&core, "db.use", json!({ "site": "shop", "name": "missing" })).await.unwrap_err();
    assert!(err.message.contains("missing"), "{}", err.message);
    assert_eq!(env_db(&shop).as_deref(), Some("shop_next"), "unchanged after a failed use");

    for current in ["shop_next", "SHOP_NEXT", "`shop_next`", "'shop_next'", " shop_next "] {
        let err = call(&core, "db.drop", json!({ "site": "shop", "name": current })).await.unwrap_err();
        assert!(err.message.contains("uses"), "{current}: {}", err.message);
    }
    call(&core, "db.drop", json!({ "site": "shop", "name": "old stuff" })).await.unwrap();
    let left = server.sql("SHOW DATABASES");
    assert!(!left.lines().any(|l| l == "old stuff") && left.lines().any(|l| l == "shop_next"), "{left}");
}
