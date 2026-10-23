//! The php.* and jobs.* IPC methods, and the JSON shapes the app UI shares with them.

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::php::manifest::{builds_for, compare_versions, fetch, Arch, Os};
use crate::php::{service, store};
use crate::{CoreError, HearthPaths};

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(INVALID_PARAMS, message)
}

/// User-facing errors keep their message; the code tells frontends it is not a protocol problem.
fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params.get(key).and_then(Value::as_str).ok_or_else(|| invalid(format!("{key} must be a string")))
}

fn flag(params: &Value, key: &str) -> Result<bool, RpcError> {
    params.get(key).and_then(Value::as_bool).ok_or_else(|| invalid(format!("{key} must be true or false")))
}

/// Installed versions with their extension switches, and the default.
pub fn list(paths: &HearthPaths) -> Value {
    let state = store::load(paths);
    let installed: Vec<Value> = store::installed(paths)
        .into_iter()
        .map(|i| {
            let extensions = state.extensions.get(&i.minor).cloned().unwrap_or_default();
            json!({ "minor": i.minor, "version": i.version, "extensions": extensions })
        })
        .collect();
    json!({ "installed": installed, "default": state.default })
}

/// The catalog's builds for this computer, newest first, with what is installed.
pub async fn available(core: &CoreState) -> Result<Value, CoreError> {
    let manifest = fetch(&core.paths, &core.catalog.url, &core.catalog.public_key).await?;
    let installed = store::installed(&core.paths);
    let builds: Vec<Value> = builds_for(&manifest, Os::current(), Arch::current())
        .into_iter()
        .map(|b| {
            let mine = installed.iter().find(|i| i.minor == b.minor);
            json!({
                "minor": b.minor,
                "version": b.version,
                "channel": b.channel,
                "installedVersion": mine.map(|i| i.version.clone()),
                "updateAvailable": mine.is_some_and(|i| compare_versions(&b.version, &i.version).is_gt()),
            })
        })
        .collect();
    Ok(Value::Array(builds))
}

/// Answers a php.* / jobs.* / tools.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let paths = &core.paths;
    let result = match method {
        "php.list" => Ok(list(paths)),
        "php.available" => available(core).await.map_err(failed),
        "php.install" => text(params, "minor").map(|minor| json!({ "jobId": service::install(core, minor) })),
        "php.uninstall" => match text(params, "minor") {
            Ok(minor) => service::uninstall(core, minor).map(|d| json!({ "default": d })).map_err(failed),
            Err(e) => Err(e),
        },
        "php.use" => match text(params, "minor") {
            Ok(minor) => service::use_version(paths, minor).map(|()| Value::Null).map_err(failed),
            Err(e) => Err(e),
        },
        "php.extension" => match (text(params, "minor"), text(params, "name"), flag(params, "on")) {
            (Ok(minor), Ok(name), Ok(on)) => {
                service::set_extension(paths, minor, name, on).map(|()| Value::Null).map_err(failed)
            }
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Err(e),
        },
        "php.ini.get" => match text(params, "minor") {
            Ok(minor) => service::ini_parts(paths, minor)
                .map(|(generated, user)| json!({ "generated": generated, "user": user }))
                .map_err(failed),
            Err(e) => Err(e),
        },
        "php.ini.set" => match (text(params, "minor"), text(params, "user")) {
            (Ok(minor), Ok(user)) => service::set_ini_user(paths, minor, user).map(|()| Value::Null).map_err(failed),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "tools.laravel" => Ok(json!({ "jobId": service::install_laravel_installer(core) })),
        "terminal.get" => crate::path_env::terminal_status(paths)
            .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
            .map_err(failed),
        "terminal.set" => match flag(params, "on") {
            Ok(on) => crate::path_env::set_terminal(paths, on)
                .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
                .map_err(failed),
            Err(e) => Err(e),
        },
        "jobs.get" => match params.get("id").and_then(Value::as_u64) {
            Some(id) => core
                .jobs
                .get(id)
                .map(|job| serde_json::to_value(job).unwrap_or(Value::Null))
                .ok_or_else(|| invalid(format!("unknown job {id}"))),
            None => Err(invalid("id must be a number")),
        },
        "jobs.list" => Ok(serde_json::to_value(core.jobs.list()).unwrap_or(Value::Null)),
        _ => return None,
    };
    Some(result)
}
