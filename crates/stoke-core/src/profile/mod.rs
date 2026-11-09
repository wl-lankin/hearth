//! SPX, the profiler (macOS builds only): its key and the folder its reports go to.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{atomic, CoreError, StokePaths};

pub mod api;
pub mod reports;

#[derive(Serialize, Deserialize)]
struct SpxConfig {
    key: String,
}

/// Where SPX writes its reports.
pub fn data_dir(paths: &StokePaths) -> PathBuf {
    paths.data.join("spx")
}

/// The key SPX asks for before it profiles a request or shows its web UI: made once, then kept.
/// 32 hex characters (it goes into the Caddyfile unquoted); anything else is replaced.
pub fn spx_key(paths: &StokePaths) -> Result<String, CoreError> {
    // php.ini and the Caddyfile must get the same key, also when both ask for the first one at once.
    static MAKING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _making = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    let file = paths.config.join("spx.json");
    let saved = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<SpxConfig>(&s).ok());
    if let Some(config) = saved.filter(|c| c.key.len() == 32 && c.key.chars().all(|ch| ch.is_ascii_hexdigit())) {
        return Ok(config.key);
    }
    let config = SpxConfig { key: crate::services::store::secret() };
    let json = serde_json::to_string_pretty(&config).map_err(|e| CoreError::Php(e.to_string()))?;
    atomic::write(&file, json.as_bytes())?;
    Ok(config.key)
}

#[cfg(test)]
mod tests;
