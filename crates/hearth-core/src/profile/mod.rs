//! SPX, the profiler (macOS builds only): its key and the folder its reports go to.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{atomic, CoreError, HearthPaths};

pub mod api;
pub mod reports;

#[derive(Serialize, Deserialize)]
struct SpxConfig {
    key: String,
}

/// Where SPX writes its reports.
pub fn data_dir(paths: &HearthPaths) -> PathBuf {
    paths.data.join("spx")
}

/// The key SPX asks for before it profiles a request or shows its web UI: made once, then kept.
pub fn spx_key(paths: &HearthPaths) -> Result<String, CoreError> {
    let file = paths.config.join("spx.json");
    let saved = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<SpxConfig>(&s).ok());
    if let Some(config) = saved.filter(|c| c.key.len() == 32) {
        return Ok(config.key);
    }
    let config = SpxConfig { key: crate::services::store::secret() };
    let json = serde_json::to_string_pretty(&config).map_err(|e| CoreError::Php(e.to_string()))?;
    atomic::write(&file, json.as_bytes())?;
    Ok(config.key)
}

#[cfg(test)]
mod tests;
