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
