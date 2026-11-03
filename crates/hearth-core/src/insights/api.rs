//! The dumps.* IPC methods.

use serde_json::{json, Value};

use crate::insights::collector::{self, CollectorConfig};
use crate::insights::prepend;
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
    let config = collector::config(&core.paths);
    value["enabled"] = json!(config.dumps);
    value["requests"] = json!(config.requests);
    value
}

/// Switches dumps (`requests: false`) or requests on or off: saves the setting, rewrites php.ini of
/// every PHP and lets the pools restart with it.
fn enable(core: &CoreState, params: &Value, requests: bool) -> Result<Value, RpcError> {
    let on = params.get("on").and_then(Value::as_bool).ok_or_else(|| invalid("on must be true or false"))?;
    {
        let _php = core.php_lock.lock().unwrap_or_else(|e| e.into_inner());
        let now = collector::config(&core.paths);
        let config =
            if requests { CollectorConfig { requests: on, ..now } } else { CollectorConfig { dumps: on, ..now } };
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
    prepend::write_prepend(&core.paths, &config).map_err(failed)?;
    Ok(status(core))
}

/// Answers a dumps.* method, or `None` when `method` is not one of them.
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
        "dumps.enable" => enable(core, params, false),
        "requests.enable" => enable(core, params, true),
        // `after`: only the requests newer than that id (the page's poll); summaries without queries.
        "requests.list" => {
            let after = params.get("after").and_then(Value::as_u64).unwrap_or(0);
            Ok(serde_json::to_value(core.collector.requests_after(after)).unwrap_or(Value::Null))
        }
        "requests.get" => match params.get("id").and_then(Value::as_u64) {
            Some(id) => core
                .collector
                .request(id)
                .map(|r| serde_json::to_value(r).unwrap_or(Value::Null))
                .ok_or_else(|| failed(CoreError::Php("This request is gone".into()))),
            None => Err(invalid("id must be a number")),
        },
        "requests.clear" => {
            core.collector.clear_requests();
            Ok(Value::Null)
        }
        "dumps.port" => set_port(core, params).await,
        _ => return None,
    };
    Some(result)
}
