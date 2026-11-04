//! `hearth mcp`: the MCP server agents talk to, with Herd's tool surface.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use hearth_core::HearthPaths;
use serde_json::{json, Value};

/// A running `hearth mcp` with a test home, talked to line by line.
pub struct Server {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Server {
    pub fn start(paths: &HearthPaths, cwd: &Path) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_hearth"))
            .arg("mcp")
            .env("HEARTH_HOME", &paths.root)
            .env_remove("SITE_PATH")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Server { child, input, output }
    }

    pub fn send(&mut self, line: &str) {
        self.input.write_all(line.as_bytes()).unwrap();
        self.input.write_all(b"\r\n").unwrap();
        self.input.flush().unwrap();
    }

    pub fn reply(&mut self) -> Value {
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {line:?}"))
    }

    pub fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string());
        let reply = self.reply();
        assert_eq!(reply["id"], json!(id), "{reply}");
        reply
    }

    /// Calls a tool; the text of its result and whether it is an error.
    pub fn tool(&mut self, id: u64, name: &str, arguments: Value) -> (String, bool) {
        let reply = self.request(id, "tools/call", json!({ "name": name, "arguments": arguments }));
        let result = &reply["result"];
        (result["content"][0]["text"].as_str().unwrap_or_default().to_string(), result["isError"] == true)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn home() -> (tempfile::TempDir, HearthPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(dir.path().join("hearth"));
    paths.ensure().unwrap();
    (dir, paths)
}

#[test]
fn the_server_speaks_mcp_with_herds_ten_tools() {
    let (d, paths) = home();
    let mut s = Server::start(&paths, d.path());
    let init = s.request(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "Hearth");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(init["result"]["instructions"].as_str().unwrap_or_default().contains("site info"));
    // A notification gets no answer: the next line is the ping's.
    s.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    assert_eq!(s.request(2, "ping", json!({}))["result"], json!({}));
    let list = s.request(3, "tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["site", "env", "db", "mail", "insights", "logs", "processes", "services", "workspaces", "runtime"]
    );
    for t in tools {
        let schema = &t["inputSchema"];
        assert_eq!(schema["required"], json!(["action"]), "{}", t["name"]);
        assert!(schema["properties"]["action"]["enum"].as_array().is_some_and(|e| !e.is_empty()), "{}", t["name"]);
        assert!(t["description"].as_str().unwrap().contains("Actions (pass one as `action`):"), "{}", t["name"]);
    }
    assert_eq!(s.request(4, "prompts/list", json!({}))["result"]["prompts"], json!([]));
    assert_eq!(s.request(5, "resources/list", json!({}))["result"]["resources"], json!([]));
}

#[test]
fn errors_are_answered_as_mcp_expects() {
    let (d, paths) = home();
    let mut s = Server::start(&paths, d.path());
    assert_eq!(s.request(1, "nope/nope", json!({}))["error"]["code"], -32601);
    s.send("{ not json");
    assert_eq!(s.reply()["error"]["code"], -32700);
    let unknown = s.request(2, "tools/call", json!({ "name": "nope", "arguments": {} }));
    assert_eq!(unknown["error"]["code"], -32602);
    let (text, error) = s.tool(3, "runtime", json!({ "action": "status" }));
    assert!(error);
    assert_eq!(text, "Hearth is not running. Start the Hearth app.");
    let (text, error) = s.tool(4, "site", json!({ "action": "fly" }));
    assert!(error);
    assert!(text.starts_with("Unknown action \"fly\". Use one of: info, list,"), "{text}");
    let (text, error) = s.tool(5, "site", json!({ "action": "isolate", "phpVersion": null }));
    assert!(error);
    assert_eq!(text, "The isolate action needs: phpVersion.");
}

/// A home with the site `shop` (a Laravel-like folder with a log), and a core serving it.
fn shop_home() -> (tempfile::TempDir, HearthPaths, std::path::PathBuf) {
    let (d, paths) = home();
    let shop = d.path().join("shop");
    std::fs::create_dir_all(shop.join("public")).unwrap();
    std::fs::write(shop.join("public").join("index.php"), "<?php").unwrap();
    std::fs::create_dir_all(shop.join("storage").join("logs")).unwrap();
    std::fs::write(
        shop.join("storage").join("logs").join("laravel.log"),
        "[2026-11-04 10:00:00] local.ERROR: Division by zero {\"userId\":3}\n[2026-11-04 10:01:00] local.INFO: Order placed\n",
    )
    .unwrap();
    hearth_core::sites::registry::link(&paths, "shop", &shop).unwrap();
    (d, paths, shop)
}

fn mail(paths: &HearthPaths, subject: &str) {
    let envelope = hearth_core::mail::store::Envelope {
        site: Some("shop".into()),
        from: "app@shop.test".into(),
        to: vec!["anna@example.com".into()],
    };
    let raw = format!(
        "From: app@shop.test\r\nTo: anna@example.com\r\nSubject: {subject}\r\nContent-Type: text/plain\r\n\r\nConfirm here: https://shop.test/confirm?t=1 and http://shop.test/help\r\n"
    );
    hearth_core::mail::store::save(paths, &envelope, raw.as_bytes()).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_actions_answer_from_hearths_core() {
    let (_d, paths, shop) = shop_home();
    mail(&paths, "Welcome");
    let mut core = hearth_core::ipc::CoreState::new(paths.clone());
    core.web_sync = false;
    let server = hearth_core::ipc::serve(&paths, hearth_core::ipc::router(core)).await.unwrap();
    let (p, cwd) = (paths.clone(), shop.join("public"));
    let results = tokio::task::spawn_blocking(move || {
        let mut s = Server::start(&p, &cwd);
        let mut out = vec![];
        for (tool, args) in [
            ("site", json!({ "action": "list" })),
            ("site", json!({ "action": "info" })),
            ("site", json!({ "action": "isolate", "phpVersion": "8.3" })),
            ("site", json!({ "action": "info", "site": "SHOP" })),
            ("site", json!({ "action": "unisolate" })),
            ("mail", json!({ "action": "list", "to": "anna" })),
            ("logs", json!({ "action": "files" })),
            ("logs", json!({ "action": "read", "levels": "error" })),
            ("processes", json!({ "action": "list" })),
            ("workspaces", json!({ "action": "list" })),
            ("runtime", json!({ "action": "status" })),
            ("env", json!({ "action": "get" })),
            ("site", json!({ "action": "studio" })),
        ] {
            out.push(s.tool(out.len() as u64 + 1, tool, args));
        }
        let id = serde_json::from_str::<Value>(&out[5].0).unwrap()[0]["id"].as_str().unwrap().to_string();
        out.push(s.tool(90, "mail", json!({ "action": "show", "id": id })));
        out.push(s.tool(91, "mail", json!({ "action": "links", "id": id })));
        // wait: a mail sent after the call starts.
        let p2 = p.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            mail(&p2, "Your code");
        });
        out.push(s.tool(92, "mail", json!({ "action": "wait", "subject": "code", "timeout": 10 })));
        sender.join().unwrap();
        out.push(s.tool(93, "mail", json!({ "action": "wait", "subject": "never", "timeout": 1 })));
        out
    })
    .await
    .unwrap();
    server.shutdown().await;
    let ok = |i: usize| {
        assert!(!results[i].1, "call {i} failed: {}", results[i].0);
        results[i].0.clone()
    };
    assert!(ok(0).contains("\"shop\""), "{}", results[0].0);
    let info: Value = serde_json::from_str(&ok(1)).unwrap();
    assert_eq!(info["name"], "shop");
    assert_eq!(info["url"], "http://shop.test:8080");
    assert!(ok(2).contains("8.3"));
    let isolated: Value = serde_json::from_str(&ok(3)).unwrap();
    assert_eq!(isolated["php"], "8.3");
    ok(4);
    let mails: Value = serde_json::from_str(&ok(5)).unwrap();
    assert_eq!(mails[0]["subject"], "Welcome");
    assert!(ok(6).contains("laravel.log"));
    let entries: Value = serde_json::from_str(&ok(7)).unwrap();
    assert_eq!(entries.as_array().map(Vec::len), Some(1), "only the error");
    assert_eq!(entries[0]["level"], "error");
    ok(8);
    ok(9);
    assert!(ok(10).contains("\"version\""));
    assert!(results[11].1 && results[11].0.contains("not available in Hearth yet"), "{:?}", results[11]);
    assert!(results[12].1 && results[12].0.contains("Studio"), "{:?}", results[12]);
    assert!(ok(13).contains("Subject: Welcome") || ok(13).contains("Welcome"), "{}", results[13].0);
    let links: Value = serde_json::from_str(&ok(14)).unwrap();
    assert_eq!(links, json!(["https://shop.test/confirm?t=1", "http://shop.test/help"]));
    assert!(ok(15).contains("Your code"), "{}", results[15].0);
    assert!(results[16].1 && results[16].0.contains("No matching mail"), "{:?}", results[16]);
}
