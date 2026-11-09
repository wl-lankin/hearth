//! The env.* IPC methods.

use serde_json::{json, Map, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND};
use crate::CoreError;

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params.get(key).and_then(Value::as_str).ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("{key} is required")))
}

/// Answers an env.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let paths = &core.paths;
    let result = (|| -> Result<Value, RpcError> {
        match method {
            "env.get" => {
                let site = text(params, "site")?;
                match params.get("key").and_then(Value::as_str) {
                    Some(key) => {
                        let value = super::get(paths, site, key).map_err(failed)?;
                        Ok(json!({ "key": key, "value": value }))
                    }
                    None => {
                        let values: Map<String, Value> = super::values(paths, site)
                            .map_err(failed)?
                            .into_iter()
                            .map(|(k, v)| (k, Value::String(v)))
                            .collect();
                        Ok(json!({ "values": values }))
                    }
                }
            }
            "env.set" => {
                let (site, key, value) = (text(params, "site")?, text(params, "key")?, text(params, "value")?);
                super::set(paths, site, key, value).map_err(failed)?;
                Ok(json!({ "key": key, "value": value }))
            }
            "env.unset" => {
                let (site, key) = (text(params, "site")?, text(params, "key")?);
                super::unset(paths, site, key).map_err(failed)?;
                Ok(json!({ "key": key }))
            }
            "env.check" => {
                let (missing, extra) = super::check(paths, text(params, "site")?).map_err(failed)?;
                Ok(json!({ "missing": missing, "extra": extra }))
            }
            "env.useService" => {
                let (site, category, selection) =
                    (text(params, "site")?, text(params, "category")?, text(params, "selection")?);
                let changed = super::use_service(paths, site, category, selection).map_err(failed)?;
                Ok(json!({ "changed": changed }))
            }
            _ => Err(RpcError::new(METHOD_NOT_FOUND, format!("unknown method: {method}"))),
        }
    })();
    method.starts_with("env.").then_some(result)
}
