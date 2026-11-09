//! The profile.* IPC methods.

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR};
use crate::profile::{data_dir, reports, spx_key};
use crate::CoreError;

/// How many reports the Profiles page lists.
const LIST_LIMIT: usize = 200;

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

/// Whether any installed PHP can profile (SPX has macOS builds only), and the key its UI asks for.
fn status(core: &CoreState) -> Result<Value, RpcError> {
    let state = crate::php::store::load(&core.paths);
    let available = state.extensions.values().any(|e| e.contains_key("spx"));
    Ok(json!({ "available": available, "key": spx_key(&core.paths).map_err(failed)? }))
}

/// Answers a profile.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, _params: &Value) -> Option<Result<Value, RpcError>> {
    let dir = data_dir(&core.paths);
    let result = match method {
        "profile.status" => status(core),
        "profile.list" => {
            let reports = tokio::task::spawn_blocking(move || reports::list(&dir, LIST_LIMIT)).await;
            reports.map(|r| json!({ "reports": r })).map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))
        }
        "profile.clear" => reports::clear(&dir).map(|n| json!({ "deleted": n })).map_err(failed),
        _ => return None,
    };
    Some(result)
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
