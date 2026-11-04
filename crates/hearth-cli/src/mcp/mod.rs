//! `hearth mcp`: an MCP server over stdio (newline-delimited JSON-RPC 2.0) with Herd's MCP tool
//! surface, so agents set up for Herd keep working. Every action is a call to Hearth's core.
//! stdout carries only protocol lines.

mod actions;
pub mod install;
pub mod site;
pub mod tools;

use std::io::{BufRead, Write};
use std::path::PathBuf;

use hearth_core::HearthPaths;
use serde_json::{json, Value};

const DEFAULT_PROTOCOL: &str = "2025-03-26";

const INSTRUCTIONS: &str = "Hearth serves the user's PHP sites locally at http(s)://<site>.test: never start a web server (no `php artisan serve`, no `php -S`).
- Start with `site info` to learn the site's URL, PHP version and services.
- Debugging: `logs read` for the log and its exceptions, `insights analyze` for N+1, duplicate and slow queries (pass the printed cursor as `after` next time).
- Mail: call `mail wait`, then trigger the sending; it returns the mail.
- Use `workspaces create` instead of `git worktree add`: the branch gets its own .test domain and .env.
- Use `processes` for long-running dev processes (Vite, queue workers), not background shell jobs.
Site-scoped actions take an optional `site` (a name or a path inside it); the default is the project this server was started in.";

/// Where the server was started: `SITE_PATH` when it names a folder, else the working folder.
pub fn start_dir() -> PathBuf {
    std::env::var_os("SITE_PATH")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Answers one request; `None` for a notification.
async fn answer(paths: &HearthPaths, message: &Value) -> Option<Value> {
    // Batches were dropped from MCP (2025-06-18); anything but one object is an invalid request.
    if !message.is_object() {
        return Some(error(
            &Value::Null,
            -32600,
            "Send one JSON-RPC request object per line (batches are not supported)",
        ));
    }
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
    Some(match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(DEFAULT_PROTOCOL);
            reply(
                &id,
                json!({
                    "protocolVersion": version,
                    "serverInfo": { "name": "Hearth", "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "prompts": { "listChanged": false },
                        "resources": { "listChanged": false, "subscribe": false },
                    },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => reply(&id, json!({})),
        "tools/list" => {
            let list: Vec<Value> = tools::TOOLS
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description(), "inputSchema": t.schema() }))
                .collect();
            reply(&id, json!({ "tools": list }))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match tools::find(name) {
                None => error(&id, -32602, &format!("Unknown tool: {name}")),
                Some(tool) => {
                    let (text, is_error) = match tool.action(&args) {
                        Err(e) => (e, true),
                        Ok(action) => match actions::run(paths, tool.name, action.name, &args).await {
                            Ok(text) => (text, false),
                            Err(e) => (e, true),
                        },
                    };
                    let mut result = json!({ "content": [{ "type": "text", "text": text }] });
                    if is_error {
                        result["isError"] = json!(true);
                    }
                    reply(&id, result)
                }
            }
        }
        "prompts/list" => reply(&id, json!({ "prompts": [] })),
        "resources/list" => reply(&id, json!({ "resources": [] })),
        _ => error(&id, -32601, &format!("No handler found for method {method}")),
    })
}

/// Serves until stdin closes. Requests are answered one at a time, in order.
pub async fn serve(paths: &HearthPaths) {
    let mut input = std::io::stdin().lock();
    let mut out = std::io::stdout().lock();
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match input.read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        // A line that is no UTF-8 is a parse error, not the end of the conversation.
        let answer = match std::str::from_utf8(&bytes) {
            Err(_) => Some(error(&Value::Null, -32700, "Parse error: the line is not UTF-8")),
            Ok(line) if line.trim().is_empty() => continue,
            Ok(line) => match serde_json::from_str::<Value>(line.trim()) {
                Ok(message) => answer(paths, &message).await,
                Err(e) => Some(error(&Value::Null, -32700, &format!("Parse error: {e}"))),
            },
        };
        if let Some(answer) = answer {
            if writeln!(out, "{answer}").and_then(|()| out.flush()).is_err() {
                break;
            }
        }
    }
}
