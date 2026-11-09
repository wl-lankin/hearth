use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError};
use crate::StokePaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

const ENV: &str = "APP_NAME=Shop\r\n# database\r\nDB_CONNECTION=mysql\r\nDB_HOST=10.0.0.5\r\nDB_PORT=3306\r\nDB_DATABASE=shop\r\nDB_USERNAME=app\r\nDB_PASSWORD=secret\r\nQUEUE_CONNECTION=database\r\nCACHE_STORE=database\r\n";

fn home() -> (tempfile::TempDir, CoreState, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(d.path().join("stoke"));
    paths.ensure().unwrap();
    let shop = d.path().join("shop");
    std::fs::create_dir_all(shop.join("public")).unwrap();
    std::fs::write(shop.join("public").join("index.php"), "<?php").unwrap();
    std::fs::write(shop.join(".env"), ENV).unwrap();
    std::fs::write(shop.join(".env.example"), "APP_NAME=\nAPP_KEY=\nDB_CONNECTION=sqlite\nQUEUE_CONNECTION=\n")
        .unwrap();
    crate::sites::registry::link(&paths, "shop", &shop).unwrap();
    crate::sites::registry::proxy(&paths, "vite", "http://localhost:5173").unwrap();
    let mut core = CoreState::new(paths);
    core.web_sync = false;
    (d, core, shop)
}

#[tokio::test(flavor = "multi_thread")]
async fn values_are_read_set_and_unset_keeping_the_file() {
    let (_d, core, shop) = home();
    let all = call(&core, "env.get", json!({ "site": "shop" })).await.unwrap();
    assert_eq!(all["values"]["DB_PASSWORD"], "secret");
    assert_eq!(all["values"].as_object().unwrap().len(), 9);
    let one = call(&core, "env.get", json!({ "site": "shop", "key": "DB_HOST" })).await.unwrap();
    assert_eq!(one, json!({ "key": "DB_HOST", "value": "10.0.0.5" }));
    assert_eq!(call(&core, "env.get", json!({ "site": "shop", "key": "NOPE" })).await.unwrap()["value"], Value::Null);
    call(&core, "env.set", json!({ "site": "shop", "key": "APP_NAME", "value": "My Shop" })).await.unwrap();
    call(&core, "env.set", json!({ "site": "shop", "key": "NEW_KEY", "value": "x" })).await.unwrap();
    call(&core, "env.unset", json!({ "site": "shop", "key": "DB_PASSWORD" })).await.unwrap();
    let text = std::fs::read_to_string(shop.join(".env")).unwrap();
    assert!(text.starts_with("APP_NAME='My Shop'\r\n# database\r\n"), "{text}");
    assert!(text.ends_with("NEW_KEY=x\r\n") && !text.contains("DB_PASSWORD"), "{text}");
    let bad = call(&core, "env.set", json!({ "site": "shop", "key": "BAD KEY", "value": "x" })).await.unwrap_err();
    assert!(bad.message.contains("BAD KEY"), "{}", bad.message);
    let before = std::fs::read_to_string(shop.join(".env")).unwrap();
    let bad = call(&core, "env.set", json!({ "site": "shop", "key": "PEM", "value": "a\nb" })).await.unwrap_err();
    assert!(bad.message.contains("line break"), "{}", bad.message);
    assert_eq!(std::fs::read_to_string(shop.join(".env")).unwrap(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn check_compares_with_the_example() {
    let (_d, core, _shop) = home();
    let check = call(&core, "env.check", json!({ "site": "shop" })).await.unwrap();
    assert_eq!(check["missing"], json!(["APP_KEY"]));
    assert_eq!(
        check["extra"],
        json!(["CACHE_STORE", "DB_DATABASE", "DB_HOST", "DB_PASSWORD", "DB_PORT", "DB_USERNAME"])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_service_takes_over_a_category_and_unmanaged_gives_the_file_back() {
    let (_d, core, shop) = home();
    let paths = &core.paths;
    crate::services::store::create(paths, "mariadb", "11.8", 3307, Some("Shop DB")).unwrap();
    crate::services::store::create(paths, "redis", "8", 6380, None).unwrap();
    let db = call(&core, "env.useService", json!({ "site": "shop", "category": "database", "selection": "mariadb" }))
        .await
        .unwrap();
    assert_eq!(db["changed"]["DB_PORT"], "3307");
    let cache = call(&core, "env.useService", json!({ "site": "shop", "category": "cache", "selection": "redis" }))
        .await
        .unwrap();
    assert_eq!(cache["changed"]["CACHE_STORE"], "redis");
    call(&core, "env.useService", json!({ "site": "shop", "category": "queue", "selection": "sync" })).await.unwrap();
    // A second change of a category keeps the first originals.
    call(&core, "env.useService", json!({ "site": "shop", "category": "database", "selection": "sqlite" }))
        .await
        .unwrap();
    let text = std::fs::read_to_string(shop.join(".env")).unwrap();
    assert!(
        text.contains("DB_CONNECTION=sqlite") && text.contains("CACHE_STORE=redis") && text.contains("REDIS_PORT=6380"),
        "{text}"
    );
    // SQLite: Laravel's default file, which now exists; a server's database name would be read as a path.
    assert!(crate::dotenv::get(&text, "DB_DATABASE").is_none() && text.contains("# DB_DATABASE=shop"), "{text}");
    assert!(shop.join("database").join("database.sqlite").is_file());
    for category in ["database", "cache", "queue"] {
        call(&core, "env.useService", json!({ "site": "shop", "category": category, "selection": "unmanaged" }))
            .await
            .unwrap();
    }
    assert_eq!(std::fs::read_to_string(shop.join(".env")).unwrap(), ENV, "exactly as before");
    let err = call(&core, "env.useService", json!({ "site": "shop", "category": "cache", "selection": "unmanaged" }))
        .await
        .unwrap_err();
    assert!(err.message.contains("nothing to restore"), "{}", err.message);
    let err = call(&core, "env.useService", json!({ "site": "shop", "category": "database", "selection": "oracle" }))
        .await
        .unwrap_err();
    assert!(err.message.contains("oracle"), "{}", err.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_gets_a_database_name_when_the_env_has_none() {
    let (_d, core, shop) = home();
    let env = "APP_NAME=Shop\nDB_CONNECTION=sqlite\n# DB_DATABASE=laravel\n";
    std::fs::write(shop.join(".env"), env).unwrap();
    crate::services::store::create(&core.paths, "mysql", "8.4", 3308, None).unwrap();
    let done = call(&core, "env.useService", json!({ "site": "shop", "category": "database", "selection": "mysql" }))
        .await
        .unwrap();
    assert_eq!(done["changed"]["DB_DATABASE"], "shop");
    assert_eq!(
        crate::dotenv::get(&std::fs::read_to_string(shop.join(".env")).unwrap(), "DB_DATABASE").as_deref(),
        Some("shop")
    );
    call(&core, "env.useService", json!({ "site": "shop", "category": "database", "selection": "unmanaged" }))
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(shop.join(".env")).unwrap(), env);
}

#[tokio::test(flavor = "multi_thread")]
async fn proxies_and_unknown_sites_have_no_env() {
    let (_d, core, _shop) = home();
    let err = call(&core, "env.get", json!({ "site": "vite" })).await.unwrap_err();
    assert!(err.message.contains("proxy"), "{}", err.message);
    let err = call(&core, "env.get", json!({ "site": "nope" })).await.unwrap_err();
    assert_eq!(err.message, "No site named nope.test");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_env_method_is_not_found() {
    let (_d, core, _shop) = home();
    let err = call(&core, "env.nope", json!({})).await.unwrap_err();
    assert_eq!(err.message, "unknown method: env.nope");
}
