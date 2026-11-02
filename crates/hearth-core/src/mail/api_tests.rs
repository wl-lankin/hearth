use serde_json::{json, Value};

use crate::ipc::{router, CoreState, RpcError};
use crate::mail::store::{self, Envelope};
use crate::HearthPaths;

async fn call(core: &CoreState, method: &str, params: Value) -> Result<Value, RpcError> {
    router(core.clone())(method.into(), params).await
}

fn home() -> (tempfile::TempDir, CoreState) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, CoreState::new(paths))
}

fn keep(core: &CoreState, site: Option<&str>, raw: &str) -> String {
    let envelope = Envelope { site: site.map(String::from), from: "a@x.test".into(), to: vec!["b@x.test".into()] };
    store::save(&core.paths, &envelope, raw.as_bytes()).unwrap().id
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[tokio::test]
async fn status_list_get_and_delete_through_the_core() {
    let (_d, core) = home();
    let status = call(&core, "mail.status", json!({})).await.unwrap();
    assert_eq!(status["listening"], json!(false));
    assert_eq!(
        status["env"],
        json!(
            "MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT=2525\nMAIL_USERNAME=\"${APP_NAME}\"\nMAIL_PASSWORD=null"
        )
    );
    let id = keep(
        &core,
        Some("shop"),
        "Subject: Hi\r\nContent-Type: text/html\r\n\r\n<a href=\"https://shop.test/x\">x</a>",
    );
    let list = call(&core, "mail.list", json!({})).await.unwrap();
    assert_eq!(list[0]["id"], json!(id));
    assert_eq!(
        (list[0]["subject"].clone(), list[0]["site"].clone(), list[0]["seen"].clone()),
        (json!("Hi"), json!("shop"), json!(false))
    );
    let mail = call(&core, "mail.get", json!({ "id": id })).await.unwrap();
    assert_eq!(mail["html"], json!("<a href=\"https://shop.test/x\">x</a>"));
    assert_eq!(mail["seen"], json!(true));
    let raw = call(&core, "mail.raw", json!({ "id": id })).await.unwrap();
    assert_eq!(raw["truncated"], json!(false));
    assert!(raw["text"].as_str().unwrap().starts_with("Subject: Hi"));
    call(&core, "mail.delete", json!({ "id": id })).await.unwrap();
    assert_eq!(call(&core, "mail.list", json!({})).await.unwrap(), json!([]));
    let gone = call(&core, "mail.get", json!({ "id": id })).await.unwrap_err();
    assert_eq!(gone.message, "This mail is gone");
    assert!(call(&core, "mail.get", json!({})).await.is_err());
}

#[tokio::test]
async fn clear_takes_a_site_null_or_nothing() {
    let (_d, core) = home();
    let (a, _b, _c) = (keep(&core, Some("a"), "x"), keep(&core, Some("b"), "x"), keep(&core, None, "x"));
    let sites = |v: Value| v.as_array().unwrap().iter().map(|m| m["site"].clone()).collect::<Vec<_>>();
    call(&core, "mail.clear", json!({ "site": "b" })).await.unwrap();
    assert_eq!(sites(call(&core, "mail.list", json!({})).await.unwrap()), [Value::Null, json!("a")]);
    call(&core, "mail.clear", json!({ "site": null })).await.unwrap();
    assert_eq!(sites(call(&core, "mail.list", json!({})).await.unwrap()), [json!("a")]);
    keep(&core, None, "x");
    call(&core, "mail.clear", json!({})).await.unwrap();
    assert_eq!(call(&core, "mail.list", json!({})).await.unwrap(), json!([]));
    assert!(call(&core, "mail.clear", json!({ "site": 7 })).await.is_err());
    let _ = a;
}

#[tokio::test]
async fn save_attachment_writes_the_bytes_to_the_chosen_path() {
    let (d, core) = home();
    let raw = "Subject: a\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nsee file\r\n\
--b\r\nContent-Type: text/csv; name=\"r.csv\"\r\nContent-Disposition: attachment; filename=\"r.csv\"\r\n\r\na,b\r\n--b--\r\n";
    let id = keep(&core, None, raw);
    let target = d.path().join("saved.csv");
    let params = json!({ "id": id, "index": 0, "path": target.to_string_lossy() });
    call(&core, "mail.saveAttachment", params).await.unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"a,b");
    let missing = json!({ "id": id, "index": 3, "path": target.to_string_lossy() });
    assert!(call(&core, "mail.saveAttachment", missing).await.is_err());
}

#[tokio::test]
async fn port_change_moves_the_catcher_and_a_taken_port_keeps_the_old_one() {
    let (_d, core) = home();
    let first = free_port();
    let status = call(&core, "mail.port", json!({ "port": first })).await.unwrap();
    assert_eq!((status["port"].clone(), status["listening"].clone()), (json!(first), json!(true)));
    assert!(status["env"].as_str().unwrap().contains(&format!("MAIL_PORT={first}")));
    let same = call(&core, "mail.port", json!({ "port": first })).await.unwrap();
    assert_eq!(same["listening"], json!(true), "asking for the port in use changes nothing");

    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = blocker.local_addr().unwrap().port();
    let err = call(&core, "mail.port", json!({ "port": taken })).await.unwrap_err();
    assert!(err.message.contains("is taken"), "{}", err.message);
    let status = call(&core, "mail.status", json!({})).await.unwrap();
    assert_eq!((status["port"].clone(), status["listening"].clone()), (json!(first), json!(true)));
    assert_eq!(crate::mail::smtp::configured_port(&core.paths), first, "only a port that works is saved");
    assert!(call(&core, "mail.port", json!({ "port": 0 })).await.is_err());
    core.mail.stop();
}
