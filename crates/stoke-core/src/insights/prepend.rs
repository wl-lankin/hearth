//! The script php.ini loads before every PHP run (`auto_prepend_file`) to send dumps to Stoke.

use std::path::PathBuf;

use crate::insights::collector::{self, CollectorConfig};
use crate::{atomic, CoreError, StokePaths};

const SCRIPT: &str = include_str!("prepend.php");

/// Named like Symfony's own file with dump() in it: Laravel finds a dump's line by that name.
pub fn script_path(paths: &StokePaths) -> PathBuf {
    let parts = ["collector", "symfony", "var-dumper", "Resources", "functions", "dump.php"];
    parts.iter().fold(paths.data.clone(), |p, part| p.join(part))
}

/// Writes the script for `config`'s port and switches; an unchanged script is left alone (PHP
/// caches it by mtime).
pub fn write_prepend(paths: &StokePaths, config: &CollectorConfig) -> Result<PathBuf, CoreError> {
    let path = script_path(paths);
    let flag = |on: bool| if on { "true" } else { "false" };
    let content = SCRIPT
        .replace("__STOKE_PORT__", &config.port.to_string())
        .replace("__STOKE_DUMPS__", flag(config.dumps))
        .replace("__STOKE_REQUESTS__", flag(config.requests));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        atomic::write(&path, content.as_bytes())?;
    }
    Ok(path)
}

/// The script php.ini should load: written for the current settings when dumps or requests are on
/// or a site is debugged (the script starts its Xdebug session), `None` otherwise.
pub fn for_ini(paths: &StokePaths) -> Result<Option<PathBuf>, CoreError> {
    let config = collector::config(paths);
    let debugging = !crate::sites::registry::load(paths).debug.is_empty();
    if !config.dumps && !config.requests && !debugging {
        return Ok(None);
    }
    write_prepend(paths, &config).map(Some)
}

/// Rewrites the script and the php.ini of every installed PHP after a dumps setting or the debugged
/// sites changed.
pub fn apply_all(paths: &StokePaths) -> Result<(), CoreError> {
    for installed in crate::php::store::installed(paths) {
        crate::php::service::apply_config(paths, &installed.minor)?;
    }
    for_ini(paths).map(|_| ())
}
