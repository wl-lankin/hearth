//! The node.* IPC methods. Versions are named by a spec ("22", "lts/*", "22.21.1"), resolved against
//! the catalog for an install and against what is installed otherwise.

use std::path::Path;

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::php::manifest::{fetch, NodeBuild};
use crate::CoreError;

use super::install::{self, available};
use super::resolve::{find_spec, matches, parse_spec, resolve, Spec};
use super::store::{default_version, installed, set_default};

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(INVALID_PARAMS, message)
}

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params.get(key).and_then(Value::as_str).ok_or_else(|| invalid(format!("{key} must be a string")))
}

/// A spec that names versions ("system" names none).
fn spec_of(params: &Value) -> Result<Spec, RpcError> {
    match parse_spec(text(params, "spec")?) {
        Ok(Spec::Default) | Err(_) => Err(invalid("Name a Node version, like 22 or lts/*")),
        Ok(spec) => Ok(spec),
    }
}

/// The newest installed version `spec` asks for.
fn installed_version(core: &CoreState, spec: &Spec) -> Result<String, RpcError> {
    installed(&core.paths)
        .into_iter()
        .find(|i| matches(spec, &i.version, i.lts.as_deref()))
        .map(|i| i.version)
        .ok_or_else(|| failed(CoreError::Php(format!("Node {spec} is not installed"))))
}

fn list(core: &CoreState) -> Value {
    let rows: Vec<Value> = installed(&core.paths)
        .into_iter()
        .map(|i| json!({ "version": i.version, "major": major(&i.version), "lts": i.lts }))
        .collect();
    json!({ "installed": rows, "default": default_version(&core.paths) })
}

fn major(version: &str) -> u32 {
    version.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0)
}

async fn builds(core: &CoreState) -> Result<Vec<NodeBuild>, RpcError> {
    let manifest = fetch(&core.paths, &core.catalog.url, &core.catalog.public_key).await.map_err(failed)?;
    Ok(available(&manifest).into_iter().cloned().collect())
}

async fn available_list(core: &CoreState) -> Result<Value, RpcError> {
    let mine = installed(&core.paths);
    let rows: Vec<Value> = builds(core)
        .await?
        .into_iter()
        .map(|b| {
            let installed = mine.iter().any(|i| i.version == b.version);
            json!({ "major": b.major, "version": b.version, "lts": b.lts, "installed": installed })
        })
        .collect();
    Ok(Value::Array(rows))
}

/// Starts installing the newest catalog build `spec` asks for, as a background job.
async fn start_install(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let spec = spec_of(params)?;
    let build = builds(core)
        .await?
        .into_iter()
        .find(|b| matches(&spec, &b.version, b.lts.as_deref()))
        .ok_or_else(|| failed(CoreError::Php(format!("The catalog has no Node {spec} for this computer"))))?;
    let label = format!("Installing Node {}", build.version);
    let (id, new) = core.jobs.start("node.install", &format!("node.install:{}", build.version), &label);
    if new {
        let (worker, build) = (core.clone(), build.clone());
        tokio::spawn(async move {
            worker.jobs.progress(id, 0.0, &format!("Downloading Node {}", build.version));
            let result = install::install(&worker, &build).await.map(|_| ());
            worker.jobs.finish(id, result.map_err(|e| e.to_string()));
        });
    }
    Ok(json!({ "jobId": id, "version": build.version }))
}

/// The version a folder runs, and the file that asks for it (none: the default). A version that
/// is not installed comes back as `error`, so the app can show it next to the site.
fn which(core: &CoreState, dir: &Path) -> Value {
    let found = match find_spec(dir).transpose() {
        Ok(found) => found,
        Err(e) => return json!({ "version": null, "file": null, "error": e.to_string() }),
    };
    let wanted = found.as_ref().map(|f| (&f.spec, f.file.as_path()));
    let file = found.as_ref().map(|f| f.file.clone());
    match resolve(&installed(&core.paths), default_version(&core.paths).as_deref(), wanted) {
        Ok(choice) => json!({ "version": choice.version, "file": file, "error": null }),
        Err(e) => json!({ "version": null, "file": file, "error": e.to_string() }),
    }
}

/// Answers a node.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let result = match method {
        "node.list" => Ok(list(core)),
        "node.available" => available_list(core).await,
        "node.install" => start_install(core, params).await,
        "node.uninstall" => match spec_of(params).and_then(|s| installed_version(core, &s)) {
            Ok(version) => {
                install::uninstall(core, &version).await.map(|()| json!({ "version": version })).map_err(failed)
            }
            Err(e) => Err(e),
        },
        "node.default" => match spec_of(params).and_then(|s| installed_version(core, &s)) {
            Ok(version) => {
                set_default(&core.paths, Some(&version)).map(|()| json!({ "version": version })).map_err(failed)
            }
            Err(e) => Err(e),
        },
        "node.which" => text(params, "dir").map(|dir| which(core, Path::new(dir))),
        _ => return None,
    };
    Some(result)
}
