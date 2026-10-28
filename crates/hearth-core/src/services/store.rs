//! Service instances, kept in `<config>/services.json`. Each has its own data folder
//! (`<data>/services/<id>`), log (`<logs>/services/<id>.log`) and port.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::services::install::display_name;
use crate::services::kinds;
use crate::{atomic, CoreError, HearthPaths};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    /// `mysql`, then `mysql-2`, ...
    pub id: String,
    pub kind: String,
    /// The version line ("8.4"); the newest installed patch of it runs.
    pub line: String,
    pub port: u16,
    #[serde(default)]
    pub autostart: bool,
    pub name: String,
    /// The generated key or password of kinds that need one (Meilisearch, Typesense, Silo).
    #[serde(default)]
    pub secret: String,
}

/// One load-change-save of services.json at a time (the app and a CLI call may create at once).
static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn file(paths: &HearthPaths) -> PathBuf {
    paths.config.join("services.json")
}

pub fn load(paths: &HearthPaths) -> Vec<Instance> {
    std::fs::read(file(paths)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(paths: &HearthPaths, instances: &[Instance]) -> Result<(), CoreError> {
    let mut text = serde_json::to_string_pretty(instances).map_err(|e| CoreError::Php(e.to_string()))?;
    text.push('\n');
    // It holds the search keys and the Silo password.
    atomic::write_private(&file(paths), text.as_bytes())
}

pub fn get(paths: &HearthPaths, id: &str) -> Result<Instance, CoreError> {
    load(paths).into_iter().find(|i| i.id == id).ok_or_else(|| CoreError::Php(format!("No service named {id}")))
}

/// 32 random hex characters. The standard library's hasher keys come from the OS's random
/// source, so two fresh hashers give 128 random bits: enough for a local development key.
fn secret() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..2)
        .map(|_| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u8(0);
            format!("{:016x}", h.finish())
        })
        .collect()
}

pub fn create(
    paths: &HearthPaths,
    kind: &str,
    line: &str,
    port: u16,
    name: Option<&str>,
) -> Result<Instance, CoreError> {
    if !kinds::known(kind) {
        return Err(CoreError::Php(format!("Hearth cannot run {kind}")));
    }
    let _one = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut instances = load(paths);
    let id = (1..)
        .map(|n| if n == 1 { kind.to_string() } else { format!("{kind}-{n}") })
        .find(|id| !instances.iter().any(|i| &i.id == id))
        .unwrap_or_else(|| kind.to_string());
    let name = match name {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ if kind == "silo" => display_name(kind).to_string(),
        _ => format!("{} {line}", display_name(kind)),
    };
    let secret = if kinds::needs_secret(kind) { secret() } else { String::new() };
    let instance = Instance { id, kind: kind.into(), line: line.into(), port, autostart: false, name, secret };
    instances.push(instance.clone());
    save(paths, &instances)?;
    Ok(instance)
}

/// Removes the instance and its data. Callers stop it first.
pub fn delete(paths: &HearthPaths, id: &str) -> Result<(), CoreError> {
    let _one = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let instance = get(paths, id)?;
    let data = data_dir(paths, &instance);
    if data.exists() {
        std::fs::remove_dir_all(&data).map_err(|e| CoreError::io(&data, e))?;
    }
    let rest: Vec<Instance> = load(paths).into_iter().filter(|i| i.id != id).collect();
    save(paths, &rest)
}

fn change(paths: &HearthPaths, id: &str, f: impl FnOnce(&mut Instance)) -> Result<(), CoreError> {
    let _one = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut instances = load(paths);
    let instance =
        instances.iter_mut().find(|i| i.id == id).ok_or_else(|| CoreError::Php(format!("No service named {id}")))?;
    f(instance);
    save(paths, &instances)
}

pub fn set_port(paths: &HearthPaths, id: &str, port: u16) -> Result<(), CoreError> {
    change(paths, id, |i| i.port = port)
}

pub fn set_autostart(paths: &HearthPaths, id: &str, on: bool) -> Result<(), CoreError> {
    change(paths, id, |i| i.autostart = on)
}

pub fn data_dir(paths: &HearthPaths, inst: &Instance) -> PathBuf {
    paths.data.join("services").join(&inst.id)
}

pub fn log_file(paths: &HearthPaths, inst: &Instance) -> PathBuf {
    paths.logs.join("services").join(format!("{}.log", inst.id))
}
