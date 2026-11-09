//! The mail.* IPC methods.

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::mail::{smtp, store};
use crate::CoreError;

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(INVALID_PARAMS, message)
}

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params.get(key).and_then(Value::as_str).ok_or_else(|| invalid(format!("{key} must be a string")))
}

fn json_of<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The catcher's state and the `.env` lines that send an app's mail to it.
fn status(core: &CoreState) -> Value {
    let port = smtp::configured_port(&core.paths);
    let env = format!(
        "MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT={port}\nMAIL_USERNAME=\"${{APP_NAME}}\"\nMAIL_PASSWORD=null"
    );
    let mut value = json_of(core.mail.status());
    value["env"] = json!(env);
    value
}

async fn set_port(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let port = params
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p > 0)
        .ok_or_else(|| invalid("port must be a number between 1 and 65535"))?;
    let now = core.mail.status();
    if !(now.listening && now.port == port) {
        core.mail.start(&core.paths, port).await.map_err(failed)?;
    }
    smtp::set_configured_port(&core.paths, port).map_err(failed)?;
    Ok(status(core))
}

fn save_attachment(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let id = text(params, "id")?;
    let path = text(params, "path")?;
    let index = params
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|i| u32::try_from(i).ok())
        .ok_or_else(|| invalid("index must be a number"))?;
    let (_, bytes) = store::attachment(&core.paths, id, index).map_err(failed)?;
    std::fs::write(path, bytes).map_err(|e| failed(CoreError::io(path, e)))?;
    Ok(Value::Null)
}

fn clear(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let site = match params.get("site") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) => Some(Some(s.as_str())),
        Some(_) => return Err(invalid("site must be a string or null")),
    };
    store::clear(&core.paths, site).map(|()| Value::Null).map_err(failed)
}

/// Answers a mail.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let paths = &core.paths;
    let result = match method {
        "mail.status" => Ok(status(core)),
        "mail.list" => Ok(json_of(store::list(paths))),
        "mail.get" => text(params, "id").and_then(|id| store::get(paths, id).map(json_of).map_err(failed)),
        "mail.raw" => text(params, "id").and_then(|id| {
            store::raw(paths, id)
                .map(|(text, truncated)| json!({ "text": text, "truncated": truncated }))
                .map_err(failed)
        }),
        "mail.saveAttachment" => save_attachment(core, params),
        "mail.delete" => {
            text(params, "id").and_then(|id| store::delete(paths, id).map(|()| Value::Null).map_err(failed))
        }
        "mail.clear" => clear(core, params),
        "mail.port" => set_port(core, params).await,
        _ => return None,
    };
    Some(result)
}
