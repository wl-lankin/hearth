//! The script php.ini loads before every PHP run (`auto_prepend_file`) to send dumps to Hearth.

use std::path::PathBuf;

use crate::insights::collector;
use crate::{atomic, CoreError, HearthPaths};

const SCRIPT: &str = include_str!("prepend.php");

pub fn script_path(paths: &HearthPaths) -> PathBuf {
    paths.data.join("collector").join("prepend.php")
}

/// Writes the script for `port`; an unchanged script is left alone (PHP caches it by mtime).
pub fn write_prepend(paths: &HearthPaths, port: u16) -> Result<PathBuf, CoreError> {
    let path = script_path(paths);
    let content = SCRIPT.replace("__HEARTH_PORT__", &port.to_string());
    if std::fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        atomic::write(&path, content.as_bytes())?;
    }
    Ok(path)
}

/// The script php.ini should load: written for the configured port when dumps are on, `None` when off.
pub fn for_ini(paths: &HearthPaths) -> Result<Option<PathBuf>, CoreError> {
    let config = collector::config(paths);
    if !config.dumps {
        return Ok(None);
    }
    write_prepend(paths, config.port).map(Some)
}

/// Rewrites the script and the php.ini of every installed PHP after a dumps setting changed.
pub fn apply_all(paths: &HearthPaths) -> Result<(), CoreError> {
    for installed in crate::php::store::installed(paths) {
        crate::php::service::apply_config(paths, &installed.minor)?;
    }
    for_ini(paths).map(|_| ())
}
