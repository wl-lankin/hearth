//! The dumps.* IPC methods.

use serde_json::{json, Value};

use crate::insights::collector::{self, CollectorConfig};
use crate::insights::{logs, prepend};
use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::CoreError;

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(INVALID_PARAMS, message)
}

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn status(core: &CoreState) -> Value {
    let mut value = serde_json::to_value(core.collector.status()).unwrap_or(Value::Null);
    value["enabled"] = json!(collector::config(&core.paths).dumps);
    value
}

/// Saves the setting, rewrites php.ini of every PHP and lets the pools restart with it.
fn enable(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let on = params.get("on").and_then(Value::as_bool).ok_or_else(|| invalid("on must be true or false"))?;
    {
        let _php = core.php_lock.lock().unwrap_or_else(|e| e.into_inner());
        let config = CollectorConfig { dumps: on, ..collector::config(&core.paths) };
        collector::set_config(&core.paths, &config).map_err(failed)?;
        prepend::apply_all(&core.paths).map_err(failed)?;
    }
    if core.web_sync {
        crate::web::service::schedule_sync(core);
    }
    Ok(status(core))
}

async fn set_port(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let port = params
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p > 0)
        .ok_or_else(|| invalid("port must be a number between 1 and 65535"))?;
    let now = core.collector.status();
    if !(now.listening && now.port == port) {
        core.collector.start(port).await.map_err(failed)?;
    }
    let config = CollectorConfig { port, ..collector::config(&core.paths) };
    collector::set_config(&core.paths, &config).map_err(failed)?;
    // The script names the port; php.ini only names the script, so it stays as it is.
    prepend::write_prepend(&core.paths, port).map_err(failed)?;
    Ok(status(core))
}

fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params.get(key).and_then(Value::as_str).ok_or_else(|| invalid(format!("{key} must be a string")))
}

/// A log's entries; with the `size` the page already has, just `unchanged` when the file kept it.
fn read_log(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let (site, file) = (text(params, "site")?, text(params, "file")?);
    let size = logs::size(&core.paths, site, file).map_err(failed)?;
    if params.get("size").and_then(Value::as_u64) == Some(size) {
        return Ok(json!({ "size": size, "unchanged": true }));
    }
    let (entries, size) = logs::read(&core.paths, site, file).map_err(failed)?;
    Ok(json!({ "size": size, "entries": entries }))
}

/// Answers a dumps.* or logs.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let result = match method {
        "dumps.status" => Ok(status(core)),
        // `after`: only the dumps newer than that id (the page's poll).
        "dumps.list" => {
            let dumps = match params.get("after").and_then(Value::as_u64) {
                Some(after) => core.collector.dumps_after(after),
                None => core.collector.dumps(),
            };
            Ok(serde_json::to_value(dumps).unwrap_or(Value::Null))
        }
        "dumps.clear" => {
            core.collector.clear();
            Ok(Value::Null)
        }
        "dumps.enable" => enable(core, params),
        "dumps.port" => set_port(core, params).await,
        "logs.sites" => Ok(serde_json::to_value(logs::sites(&core.paths)).unwrap_or(Value::Null)),
        "logs.read" => read_log(core, params),
        "logs.clear" => match (text(params, "site"), text(params, "file")) {
            (Ok(site), Ok(file)) => logs::clear(&core.paths, site, file).map(|()| Value::Null).map_err(failed),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        _ => return None,
    };
    Some(result)
}
