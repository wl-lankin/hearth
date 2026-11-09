//! `stoke mcp`: the MCP server agents talk to, with Herd's tool surface.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};
use stoke_core::StokePaths;

/// A running `stoke mcp` with a test home, talked to line by line.
pub struct Server {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Server {
    pub fn start(paths: &StokePaths, cwd: &Path) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_stoke"))
            .arg("mcp")
            .env("STOKE_HOME", &paths.root)
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

    pub fn send_bytes(&mut self, bytes: &[u8]) {
        self.input.write_all(bytes).unwrap();
        self.input.write_all(b"\n").unwrap();
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

fn home() -> (tempfile::TempDir, StokePaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(dir.path().join("stoke"));
    paths.ensure().unwrap();
    (dir, paths)
}

#[test]
fn the_server_speaks_mcp_with_herds_ten_tools() {
    let (d, paths) = home();
    let mut s = Server::start(&paths, d.path());
    let init = s.request(1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "Stoke");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    let instructions = init["result"]["instructions"].as_str().unwrap_or_default();
    for part in ["site info", "site error", "`env`", "`db`", "use_service"] {
        assert!(instructions.contains(part), "{part}: {instructions}");
    }
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
    assert_eq!(text, "Stoke is not running. Start the Stoke app.");
    let (text, error) = s.tool(4, "site", json!({ "action": "fly" }));
    assert!(error);
    assert!(text.starts_with("Unknown action \"fly\". Use one of: info, list,"), "{text}");
    let (text, error) = s.tool(5, "site", json!({ "action": "isolate", "phpVersion": null }));
    assert!(error);
    assert_eq!(text, "The isolate action needs: phpVersion.");
    let (text, _) = s.tool(6, "mail", json!({ "action": "show", "id": "" }));
    assert_eq!(text, "The show action needs: id.", "an empty value is no value");
    // Batches (dropped from MCP in 2025-06-18) and non-objects get an answer, not silence.
    s.send(r#"[{"jsonrpc":"2.0","id":7,"method":"ping"}]"#);
    assert_eq!(s.reply()["error"]["code"], -32600);
    s.send("5");
    assert_eq!(s.reply()["error"]["code"], -32600);
    // A line that is no UTF-8 is a parse error, and the server keeps going.
    s.send_bytes(b"{\xff}");
    assert_eq!(s.reply()["error"]["code"], -32700);
    assert_eq!(s.request(8, "ping", json!({}))["result"], json!({}));
}

/// A home with the site `shop` (a Laravel-like folder with a log), and a core serving it.
fn shop_home() -> (tempfile::TempDir, StokePaths, std::path::PathBuf) {
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
    stoke_core::sites::registry::link(&paths, "shop", &shop).unwrap();
    (d, paths, shop)
}

fn mail(paths: &StokePaths, subject: &str) {
    let envelope = stoke_core::mail::store::Envelope {
        site: Some("shop".into()),
        from: "app@shop.test".into(),
        to: vec!["anna@example.com".into()],
    };
    let raw = format!(
        "From: app@shop.test\r\nTo: anna@example.com\r\nSubject: {subject}\r\nContent-Type: text/plain\r\n\r\nConfirm here: https://shop.test/confirm?t=1 and http://shop.test/help\r\n"
    );
    stoke_core::mail::store::save(paths, &envelope, raw.as_bytes()).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_actions_answer_from_stokes_core() {
    let (_d, paths, shop) = shop_home();
    mail(&paths, "Welcome");
    let mut core = stoke_core::ipc::CoreState::new(paths.clone());
    core.web_sync = false;
    // mail wait needs a running catcher (on a free port, never 2525).
    let smtp = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    core.mail.start(&paths, smtp).await.unwrap();
    let server = stoke_core::ipc::serve(&paths, stoke_core::ipc::router(core.clone())).await.unwrap();
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
            ("site", json!({ "action": "error" })),
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
    core.mail.stop();
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
    assert!(ok(11).contains("No exception"), "{}", results[11].0);
    assert!(results[12].1 && results[12].0.contains("Studio"), "{:?}", results[12]);
    assert!(ok(13).contains("Subject: Welcome") || ok(13).contains("Welcome"), "{}", results[13].0);
    let links: Value = serde_json::from_str(&ok(14)).unwrap();
    assert_eq!(links, json!(["https://shop.test/confirm?t=1", "http://shop.test/help"]));
    assert!(ok(15).contains("Your code"), "{}", results[15].0);
    assert!(results[16].1 && results[16].0.contains("No matching mail"), "{:?}", results[16]);
}

#[test]
fn mcp_install_registers_stoke_in_the_projects_mcp_json() {
    let (d, paths) = home();
    let project = d.path().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_stoke"))
        .args(["mcp:install", "--agent", "claude", "--agent", "vscode"])
        .env("STOKE_HOME", &paths.root)
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(".mcp.json") && text.contains("mcp.json"), "{text}");
    let config: Value = serde_json::from_str(&std::fs::read_to_string(project.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(config["mcpServers"]["stoke"]["args"], json!(["mcp"]));
    assert!(project.join(".vscode").join("mcp.json").is_file());
    let bad = Command::new(env!("CARGO_BIN_EXE_stoke"))
        .args(["mcp:install", "--agent", "emacs"])
        .env("STOKE_HOME", &paths.root)
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(!bad.status.success());
}

/// A fake Redis 8 in the home (the core's `fake_service` example listening on its --port).
fn fake_redis(paths: &StokePaths) {
    let fake = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join("examples")
        .join(format!("fake_service{}", std::env::consts::EXE_SUFFIX));
    let program = paths
        .data
        .join("services")
        .join("bin")
        .join("redis")
        .join("8.10.3")
        .join(format!("redis-server{}", std::env::consts::EXE_SUFFIX));
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::copy(&fake, &program)
        .unwrap_or_else(|e| panic!("{}: {e} (build stoke-core's examples first)", fake.display()));
}

#[tokio::test(flavor = "multi_thread")]
async fn install_starts_the_service_and_link_takes_folders_and_names_as_given() {
    let (d, paths, _shop) = shop_home();
    fake_redis(&paths);
    let api = d.path().join("api");
    std::fs::create_dir_all(api.join("public")).unwrap();
    std::fs::write(api.join("public").join("index.php"), "<?php").unwrap();
    std::fs::write(api.join(".env"), "APP_NAME=Api\nAPP_URL=http://localhost\n").unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut core = stoke_core::ipc::CoreState::new(paths.clone());
    core.web_sync = false;
    let server = stoke_core::ipc::serve(&paths, stoke_core::ipc::router(core.clone())).await.unwrap();
    let (p, cwd) = (paths.clone(), d.path().join("shop"));
    let results = tokio::task::spawn_blocking(move || {
        let mut s = Server::start(&p, &cwd);
        vec![
            s.tool(1, "services", json!({ "action": "install", "type": "redis", "version": "8", "port": port })),
            s.tool(2, "services", json!({ "action": "list" })),
            // A relative folder is relative to where the server runs; a `.test` name is the site name.
            s.tool(3, "site", json!({ "action": "link", "path": "../api", "name": "Api.test" })),
            s.tool(4, "insights", json!({ "action": "capture", "enabled": "true" })),
            s.tool(5, "mail", json!({ "action": "list", "since": "a while back" })),
        ]
    })
    .await
    .unwrap();
    stoke_core::services::run::stop_all(&core).await;
    server.shutdown().await;
    assert!(!results[0].1, "{}", results[0].0);
    let list: Value = serde_json::from_str(&results[1].0).unwrap();
    assert_eq!(list[0]["running"], true, "install also starts it: {list}");
    assert!(!results[2].1, "{}", results[2].0);
    assert_eq!(results[2].0, "api is now served at http://api.test:8080 (APP_URL in its .env points there now)");
    let sites = stoke_core::sites::registry::list(&paths);
    assert!(
        sites.iter().any(|s| s.name == "api" && s.path.as_deref().is_some_and(|p| p.ends_with("api"))),
        "linked under its name"
    );
    assert!(std::fs::read_to_string(api.join(".env")).unwrap().contains("APP_URL=http://api.test:8080"));
    assert!(!results[3].1 && results[3].0.contains(" on"), "a string flag counts: {}", results[3].0);
    assert!(results[4].1 && results[4].0.contains("since"), "an unreadable since is an error: {}", results[4].0);
}

#[tokio::test(flavor = "multi_thread")]
async fn env_db_and_site_error_work_through_the_server() {
    let (_d, paths, shop) = shop_home();
    let env = "APP_NAME=Shop\nDB_CONNECTION=sqlite\nQUEUE_CONNECTION=database\n";
    std::fs::write(shop.join(".env"), env).unwrap();
    std::fs::write(shop.join(".env.example"), "APP_NAME=\nAPP_KEY=\nDB_CONNECTION=\n").unwrap();
    std::fs::write(
        shop.join("storage").join("logs").join("laravel.log"),
        "[2026-11-04 10:00:09] local.ERROR: boom {\"exception\":\"[object] (RuntimeException(code: 0): boom at /srv/shop/app/A.php:7)\n[stacktrace]\n#0 {main}\n\"}\n",
    )
    .unwrap();
    stoke_core::services::store::create(&paths, "redis", "8", 6390, None).unwrap();
    let mut core = stoke_core::ipc::CoreState::new(paths.clone());
    core.web_sync = false;
    let server = stoke_core::ipc::serve(&paths, stoke_core::ipc::router(core.clone())).await.unwrap();
    let (p, cwd) = (paths.clone(), shop.clone());
    let results = tokio::task::spawn_blocking(move || {
        let mut s = Server::start(&p, &cwd);
        let mut out = vec![];
        for (tool, args) in [
            ("env", json!({ "action": "get" })),
            ("env", json!({ "action": "get", "key": "APP_NAME" })),
            ("env", json!({ "action": "set", "key": "APP_NAME", "value": "My Shop" })),
            ("env", json!({ "action": "check" })),
            ("env", json!({ "action": "use_service", "category": "queue", "selection": "redis" })),
            ("env", json!({ "action": "use_service", "category": "queue", "selection": "unmanaged" })),
            ("env", json!({ "action": "unset", "key": "APP_NAME" })),
            ("db", json!({ "action": "connection" })),
            ("db", json!({ "action": "create", "name": "reports" })),
            ("site", json!({ "action": "error" })),
            ("env", json!({ "action": "get", "key": "NOPE" })),
            ("env", json!({ "action": "set", "key": "DB_PORT", "value": 3306 })),
            ("env", json!({ "action": "set", "key": "APP_DEBUG", "value": true })),
            ("env", json!({ "action": "set", "key": "MAIL_FROM", "value": "" })),
            ("env", json!({ "action": "set", "key": "LIST", "value": [1, 2] })),
        ] {
            out.push(s.tool(out.len() as u64 + 1, tool, args));
        }
        out
    })
    .await
    .unwrap();
    server.shutdown().await;
    let ok = |i: usize| {
        assert!(!results[i].1, "call {i} failed: {}", results[i].0);
        results[i].0.clone()
    };
    let all: Value = serde_json::from_str(&ok(0)).unwrap();
    assert_eq!(all["QUEUE_CONNECTION"], "database");
    assert!(ok(1).contains("Shop"), "{}", results[1].0);
    ok(2);
    let check: Value = serde_json::from_str(&ok(3)).unwrap();
    assert_eq!((check["missing"].clone(), check["extra"].clone()), (json!(["APP_KEY"]), json!(["QUEUE_CONNECTION"])));
    assert!(ok(4).contains("REDIS_PORT") && ok(4).contains("6390"), "{}", results[4].0);
    ok(5);
    ok(6);
    // After unmanaged and unset; the sets that follow append their keys.
    let text = std::fs::read_to_string(shop.join(".env")).unwrap();
    assert!(text.starts_with("DB_CONNECTION=sqlite\nQUEUE_CONNECTION=database\nDB_PORT="), "{text}");
    let conn: Value = serde_json::from_str(&ok(7)).unwrap();
    assert_eq!(conn["driver"], "sqlite");
    assert!(ok(8).contains("reports") && shop.join("database").join("reports.sqlite").is_file(), "{}", results[8].0);
    let e: Value = serde_json::from_str(&ok(9)).unwrap();
    assert_eq!((e["class"].as_str(), e["line"].as_u64()), (Some("RuntimeException"), Some(7)));
    assert!(results[10].1 && results[10].0.contains("NOPE"), "{:?}", results[10]);
    for i in 11..=13 {
        ok(i);
    }
    let text = std::fs::read_to_string(shop.join(".env")).unwrap();
    assert!(
        text.contains("DB_PORT=3306\n") && text.contains("APP_DEBUG=true\n") && text.contains("MAIL_FROM=\n"),
        "{text}"
    );
    assert!(results[14].1 && results[14].0.contains("text"), "{:?}", results[14]);
}
