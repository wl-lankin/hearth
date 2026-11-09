//! The workspaces.* IPC methods. Making and deleting run git and copy folders: off the async threads.

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::workspaces::database::Mode;
use crate::workspaces::registry;
use crate::workspaces::service::{self, CreateOptions};
use crate::CoreError;

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(INVALID_PARAMS, message)
}

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn text<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str)
}

fn flag(params: &Value, key: &str) -> Option<bool> {
    params.get(key).and_then(Value::as_bool)
}

fn options(params: &Value) -> Result<CreateOptions, RpcError> {
    Ok(CreateOptions {
        site: text(params, "site").unwrap_or_default().to_string(),
        name: text(params, "name").map(str::to_string),
        database: Mode::parse(text(params, "database").unwrap_or_default()).map_err(|e| invalid(e.to_string()))?,
        secure: flag(params, "secure"),
        migrate: flag(params, "migrate").unwrap_or(true),
        by: text(params, "by").unwrap_or("app").to_string(),
    })
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, CoreError> + Send + 'static) -> Result<T, RpcError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))?.map_err(failed)
}

/// Answers a workspaces.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let core2 = core.clone();
    let result = match method {
        "workspaces.list" => {
            let site = text(params, "site").map(str::to_string);
            let all = registry::load(&core.paths);
            let list: Vec<_> = all.into_iter().filter(|r| site.as_deref().is_none_or(|s| r.parent_site == s)).collect();
            Ok(json!({ "workspaces": list }))
        }
        "workspaces.create" => match options(params) {
            Ok(o) => blocking(move || service::create(&core2, o))
                .await
                .map(|m| json!({ "workspace": m.record, "notes": m.notes })),
            Err(e) => Err(e),
        },
        "workspaces.adopt" => match (text(params, "path").map(PathBuf::from), options(params)) {
            (Some(path), Ok(o)) => blocking(move || service::adopt(&core2, &path, o))
                .await
                .map(|m| json!({ "workspace": m.record, "notes": m.notes })),
            (None, _) => Err(invalid("path must be a string")),
            (_, Err(e)) => Err(e),
        },
        "workspaces.delete" => match text(params, "name").map(str::to_string) {
            Some(name) => {
                let (keep_db, keep_files) =
                    (flag(params, "keepDatabase").unwrap_or(false), flag(params, "keepFiles").unwrap_or(false));
                blocking(move || service::delete(&core2, &name, keep_db, keep_files))
                    .await
                    .map(|n| json!({ "notes": n }))
            }
            None => Err(invalid("name must be a string")),
        },
        "workspaces.merge" => match text(params, "name").map(str::to_string) {
            Some(name) => {
                let (push, delete) = (flag(params, "push").unwrap_or(false), flag(params, "delete").unwrap_or(false));
                blocking(move || service::merge(&core2, &name, push, delete))
                    .await
                    .map(|(message, notes)| json!({ "message": message, "notes": notes }))
            }
            None => Err(invalid("name must be a string")),
        },
        "workspaces.find" => match text(params, "cwd") {
            Some(cwd) => Ok(json!({ "workspace": service::find_by_dir(&core.paths, std::path::Path::new(cwd)) })),
            None => Err(invalid("cwd must be a string")),
        },
        _ => return None,
    };
    Some(result)
}
