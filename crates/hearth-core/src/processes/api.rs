//! The processes.* IPC methods.

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::processes::output;
use crate::processes::run::{self, key, log_path};
use crate::processes::store::{self, Proc};
use crate::processes::suggest;
use crate::CoreError;

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(INVALID_PARAMS, message)
}

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn text<'a>(params: &'a Value, name: &str) -> Result<&'a str, RpcError> {
    params.get(name).and_then(Value::as_str).ok_or_else(|| invalid(format!("{name} must be a string")))
}

/// Every site's processes with their state.
fn list(core: &CoreState) -> Value {
    let status = core.supervisor.status();
    let sites: Vec<Value> = store::load(&core.paths)
        .into_iter()
        .map(|(site, procs)| {
            let rows: Vec<Value> = procs
                .iter()
                .map(|p| {
                    let s = status.iter().find(|s| s.key == key(&site, &p.name));
                    json!({
                        "name": p.name, "command": p.command, "autostart": p.autostart,
                        "running": s.is_some_and(|s| s.running),
                        "pid": s.and_then(|s| s.pid),
                        "restarts": s.map_or(0, |s| s.restarts),
                        "lastError": s.and_then(|s| s.last_error.clone()),
                    })
                })
                .collect();
            json!({ "site": site, "processes": rows })
        })
        .collect();
    json!({ "sites": sites })
}

/// Runs a blocking process action (a stop waits up to 2 s for the old process).
async fn blocking(f: impl FnOnce() -> Result<(), CoreError> + Send + 'static) -> Result<Value, RpcError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))?.map_err(failed)?;
    Ok(Value::Null)
}

/// Answers a processes.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let paths = &core.paths;
    let site_name = || -> Result<(String, String), RpcError> {
        Ok((text(params, "site")?.to_string(), text(params, "name")?.to_string()))
    };
    let result = match method {
        "processes.list" => Ok(list(core)),
        "processes.add" => (|| {
            let site = text(params, "site")?;
            run::site_dir(paths, site).map_err(failed)?;
            let proc = Proc {
                name: text(params, "name")?.to_string(),
                command: text(params, "command")?.to_string(),
                autostart: params.get("autostart").and_then(Value::as_bool).unwrap_or(false),
            };
            store::add(paths, site, proc).map(|()| Value::Null).map_err(failed)
        })(),
        "processes.remove" => match site_name() {
            Ok((site, name)) => {
                let core2 = core.clone();
                blocking(move || {
                    run::stop(&core2, &site, &name)?;
                    store::remove(&core2.paths, &site, &name)
                })
                .await
            }
            Err(e) => Err(e),
        },
        "processes.start" | "processes.restart" => match site_name() {
            Ok((site, name)) => {
                let core2 = core.clone();
                blocking(move || run::start(&core2, &site, &name)).await
            }
            Err(e) => Err(e),
        },
        "processes.stop" => match site_name() {
            Ok((site, name)) => {
                let core2 = core.clone();
                blocking(move || run::stop(&core2, &site, &name)).await
            }
            Err(e) => Err(e),
        },
        "processes.output" => site_name().map(|(site, name)| {
            let offset = params.get("offset").and_then(Value::as_u64);
            json!(output::read(&log_path(paths, &site, &name), offset))
        }),
        "processes.suggest" => text(params, "site").and_then(|site| {
            let dir = run::site_dir(paths, site).map_err(failed)?;
            Ok(json!(suggest::for_dir(&dir)))
        }),
        _ => return None,
    };
    Some(result)
}
