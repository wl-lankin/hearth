//! The services.* IPC methods.

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::php::manifest::{fetch, Os};
use crate::services::install::{available, display_name};
use crate::services::{kinds, ports, run, store};
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

fn port_of(params: &Value) -> Result<Option<u16>, RpcError> {
    match params.get("port") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p > 0)
            .map(Some)
            .ok_or_else(|| invalid("port must be a number between 1 and 65535")),
    }
}

/// What this computer can run, per kind and line (newest version first in the catalog's order),
/// plus kinds that cannot run here and why.
async fn available_list(core: &CoreState) -> Result<Value, RpcError> {
    let manifest = fetch(&core.paths, &core.catalog.url, &core.catalog.public_key).await.map_err(failed)?;
    let rows: Vec<Value> = available(&manifest)
        .into_iter()
        .map(|b| json!({ "kind": b.kind, "line": b.line, "version": b.version, "name": display_name(&b.kind) }))
        .collect();
    let mut missing = vec![];
    if Os::current() == Os::Windows {
        missing.push(
            json!({ "kind": "typesense", "name": "Typesense", "reason": "Not available on Windows (it needs WSL)" }),
        );
    }
    Ok(json!({ "builds": rows, "unavailable": missing }))
}

async fn create(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let kind = text(params, "kind")?;
    let line = text(params, "line")?;
    let name = params.get("name").and_then(Value::as_str);
    if !kinds::known(kind) {
        return Err(invalid(format!("Hearth cannot run {kind}")));
    }
    let port = port_of(params)?.unwrap_or_else(|| ports::suggest(&core.paths, kind));
    let probe = store::Instance {
        id: String::new(),
        kind: kind.into(),
        line: line.into(),
        port,
        autostart: false,
        name: String::new(),
        secret: String::new(),
    };
    ports::check(&core.paths, &kinds::ports(&probe), None).map_err(failed)?;
    let instance = store::create(&core.paths, kind, line, port, name).map_err(failed)?;
    Ok(serde_json::to_value(instance).unwrap_or(Value::Null))
}

async fn set_port(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let id = text(params, "id")?;
    let port = port_of(params)?.ok_or_else(|| invalid("port must be a number between 1 and 65535"))?;
    run::set_port(core, id, port).await.map(|()| Value::Null).map_err(failed)
}

/// Answers a services.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let paths = &core.paths;
    let result = match method {
        "services.available" => available_list(core).await,
        "services.list" => Ok(serde_json::to_value(run::status(core)).unwrap_or(Value::Null)),
        "services.create" => create(core, params).await,
        "services.suggestPort" => text(params, "kind").map(|k| json!(ports::suggest(paths, k))),
        "services.start" => match text(params, "id") {
            Ok(id) => run::start(core, id).await.map(|()| Value::Null).map_err(failed),
            Err(e) => Err(e),
        },
        "services.stop" => match text(params, "id") {
            Ok(id) => match store::get(paths, id) {
                Ok(_) => run::stop(core, id).await.map(|()| Value::Null).map_err(failed),
                Err(e) => Err(failed(e)),
            },
            Err(e) => Err(e),
        },
        "services.delete" => match text(params, "id") {
            Ok(id) => run::delete(core, id).await.map(|()| Value::Null).map_err(failed),
            Err(e) => Err(e),
        },
        "services.port" => set_port(core, params).await,
        "services.autostart" => match (text(params, "id"), params.get("on").and_then(Value::as_bool)) {
            (Ok(id), Some(on)) => store::set_autostart(paths, id, on).map(|()| Value::Null).map_err(failed),
            (Err(e), _) => Err(e),
            (_, None) => Err(invalid("on must be true or false")),
        },
        "services.env" => match text(params, "id").and_then(|id| store::get(paths, id).map_err(failed)) {
            Ok(instance) => {
                let lines: Vec<String> = kinds::env(&instance).into_iter().map(|(k, v)| format!("{k}={v}")).collect();
                Ok(json!(lines.join("\n")))
            }
            Err(e) => Err(e),
        },
        "services.log" => match text(params, "id").and_then(|id| store::get(paths, id).map_err(failed)) {
            Ok(instance) => {
                let text = std::fs::read_to_string(store::log_file(paths, &instance)).unwrap_or_default();
                let tail: Vec<&str> = text.lines().rev().take(200).collect();
                Ok(json!(tail.into_iter().rev().collect::<Vec<_>>().join("\n")))
            }
            Err(e) => Err(e),
        },
        _ => return None,
    };
    Some(result)
}
