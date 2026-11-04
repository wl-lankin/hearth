//! The actions: each one calls Hearth's core and turns the reply into the text an agent reads.

use hearth_core::ipc;
use hearth_core::{CoreError, HearthPaths};
use serde_json::Value;

/// A core error as an agent should read it.
pub fn readable(e: CoreError) -> String {
    match e {
        CoreError::NotRunning => "Hearth is not running. Start the Hearth app.".into(),
        CoreError::Rpc { message, .. } => message,
        other => other.to_string(),
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// Runs `tool`'s `action` with `args`; the text, or the error text.
pub async fn run(paths: &HearthPaths, tool: &str, action: &str, args: &Value) -> Result<String, String> {
    let _ = args;
    match (tool, action) {
        ("runtime", "status") => {
            ipc::call(paths, "core.status", Value::Null).await.map(|v| pretty(&v)).map_err(readable)
        }
        _ => Err(format!("`{tool} {action}` is not available in Hearth yet")),
    }
}
