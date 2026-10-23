//! The sites.* and web.* IPC methods. Changes are saved at once; the web server follows in the
//! background (`web.status` shows the result).

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::sites::registry;
use crate::web::service::{current_status, schedule_sync};
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

fn dir(params: &Value) -> Result<PathBuf, RpcError> {
    text(params, "dir").map(PathBuf::from)
}

/// Runs a registry change under the sites lock, then lets the web server follow.
fn change(core: &CoreState, f: impl FnOnce() -> Result<(), CoreError>) -> Result<Value, RpcError> {
    {
        let _guard = core.sites_lock.lock().unwrap_or_else(|e| e.into_inner());
        f().map_err(failed)?;
    }
    schedule_sync(core);
    Ok(Value::Null)
}

/// The sites, parked folders and HTTP port.
pub fn list(core: &CoreState) -> Value {
    let config = registry::load(&core.paths);
    json!({
        "sites": registry::list(&core.paths),
        "parked": config.parked,
        "httpPort": config.http_port,
    })
}

/// Answers a sites.* / web.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let paths = &core.paths;
    let result = match method {
        "sites.list" => Ok(list(core)),
        "web.status" => Ok(serde_json::to_value(current_status(core)).unwrap_or(Value::Null)),
        "sites.park" => dir(params).and_then(|d| change(core, || registry::park(paths, &d))),
        "sites.unpark" => dir(params).and_then(|d| change(core, || registry::unpark(paths, &d))),
        "sites.link" => match (text(params, "name"), dir(params)) {
            (Ok(name), Ok(d)) => change(core, || registry::link(paths, name, &d)),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "sites.unlink" => text(params, "name").and_then(|n| change(core, || registry::unlink(paths, n))),
        "sites.proxy" => match (text(params, "name"), text(params, "upstream")) {
            (Ok(name), Ok(upstream)) => change(core, || registry::proxy(paths, name, upstream)),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "sites.unproxy" => text(params, "name").and_then(|n| change(core, || registry::unproxy(paths, n))),
        "sites.isolate" => match (text(params, "name"), params.get("minor")) {
            (Ok(name), Some(Value::Null) | None) => change(core, || registry::isolate(paths, name, None)),
            (Ok(name), Some(Value::String(minor))) => change(core, || registry::isolate(paths, name, Some(minor))),
            (Ok(_), Some(_)) => Err(invalid("minor must be a string like \"8.4\" or null")),
            (Err(e), _) => Err(e),
        },
        "sites.port" => match params.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()) {
            Some(port) => change(core, || registry::set_http_port(paths, port)),
            None => Err(invalid("port must be a number between 1 and 65535")),
        },
        _ => return None,
    };
    Some(result)
}
