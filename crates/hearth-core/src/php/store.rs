//! Which PHP versions are installed, which one is the default, and which optional extensions are
//! switched on, kept in `<config>/php.json`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::php::manifest::compare_versions;
use crate::{atomic, CoreError, HearthPaths};

/// Written into an install folder after a complete unpack; a folder without it is not an install.
pub const VERSION_FILE: &str = ".hearth-version";

/// Extensions that start switched on for a new install.
const ON_BY_DEFAULT: &[&str] = &["redis"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhpState {
    pub default: Option<String>,
    pub extensions: BTreeMap<String, BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledPhp {
    pub minor: String,
    pub version: String,
    pub path: PathBuf,
}

pub fn install_dir(paths: &HearthPaths, minor: &str) -> PathBuf {
    paths.data.join("php").join("versions").join(minor)
}

fn file(paths: &HearthPaths) -> PathBuf {
    paths.config.join("php.json")
}

/// Never fails: a file that does not parse is kept aside as `php.json.corrupt-<unix-ts>`.
pub fn load(paths: &HearthPaths) -> PhpState {
    let path = file(paths);
    let Ok(bytes) = std::fs::read(&path) else { return PhpState::default() };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = std::fs::rename(&path, paths.config.join(format!("php.json.corrupt-{ts}")));
        PhpState::default()
    })
}

fn save(paths: &HearthPaths, state: &PhpState) -> Result<(), CoreError> {
    let mut text = serde_json::to_string_pretty(state).map_err(|e| CoreError::Php(e.to_string()))?;
    text.push('\n');
    atomic::write(&file(paths), text.as_bytes())
}

/// Installed versions, newest first.
pub fn installed(paths: &HearthPaths) -> Vec<InstalledPhp> {
    let Ok(entries) = std::fs::read_dir(paths.data.join("php").join("versions")) else { return Vec::new() };
    let mut list: Vec<InstalledPhp> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let minor = e.file_name().to_string_lossy().into_owned();
            let version = std::fs::read_to_string(e.path().join(VERSION_FILE)).ok()?;
            Some(InstalledPhp { minor, version: version.trim().to_string(), path: e.path() })
        })
        .collect();
    list.sort_by(|a, b| compare_versions(&b.version, &a.version));
    list
}

fn is_installed(paths: &HearthPaths, minor: &str) -> bool {
    install_dir(paths, minor).join(VERSION_FILE).is_file()
}

/// Records a finished install: the first one becomes the default; extensions the build offers get
/// their starting state, and existing switches are kept.
pub fn register(paths: &HearthPaths, minor: &str, extensions: &[String]) -> Result<(), CoreError> {
    let mut state = load(paths);
    if state.default.as_deref().is_none_or(|d| !is_installed(paths, d)) {
        state.default = Some(minor.to_string());
    }
    let switches = state.extensions.entry(minor.to_string()).or_default();
    for name in extensions {
        switches.entry(name.clone()).or_insert(ON_BY_DEFAULT.contains(&name.as_str()));
    }
    save(paths, &state)
}

pub fn set_default(paths: &HearthPaths, minor: &str) -> Result<(), CoreError> {
    if !is_installed(paths, minor) {
        return Err(CoreError::Php(format!("PHP {minor} is not installed")));
    }
    let mut state = load(paths);
    state.default = Some(minor.to_string());
    save(paths, &state)
}

/// Forgets a removed version; when it was the default, the newest remaining version takes over.
/// Returns the default afterwards.
pub fn forget(paths: &HearthPaths, minor: &str) -> Result<Option<String>, CoreError> {
    let mut state = load(paths);
    state.extensions.remove(minor);
    if state.default.as_deref() == Some(minor) {
        state.default = installed(paths).into_iter().map(|i| i.minor).find(|m| m != minor);
    }
    save(paths, &state)?;
    Ok(state.default)
}

pub fn set_extension(paths: &HearthPaths, minor: &str, name: &str, on: bool) -> Result<(), CoreError> {
    if !is_installed(paths, minor) {
        return Err(CoreError::Php(format!("PHP {minor} is not installed")));
    }
    let mut state = load(paths);
    let Some(switch) = state.extensions.get_mut(minor).and_then(|s| s.get_mut(name)) else {
        return Err(CoreError::Php(format!("PHP {minor} has no {name} extension")));
    };
    *switch = on;
    save(paths, &state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, HearthPaths) {
        let d = tempfile::tempdir().unwrap();
        let p = HearthPaths::at(d.path());
        p.ensure().unwrap();
        (d, p)
    }

    /// What the installer does after a successful unpack.
    fn install(p: &HearthPaths, minor: &str, version: &str) {
        let dir = install_dir(p, minor);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(VERSION_FILE), version).unwrap();
        register(p, minor, &["redis".to_string(), "xdebug".to_string()]).unwrap();
    }

    #[test]
    fn the_first_install_becomes_the_default_with_redis_on_and_xdebug_off() {
        let (_d, p) = home();
        install(&p, "8.4", "8.4.27");
        let state = load(&p);
        assert_eq!(state.default.as_deref(), Some("8.4"));
        assert!(state.extensions["8.4"]["redis"]);
        assert!(!state.extensions["8.4"]["xdebug"]);
        assert_eq!(
            installed(&p),
            vec![InstalledPhp { minor: "8.4".into(), version: "8.4.27".into(), path: install_dir(&p, "8.4") }]
        );
    }

    #[test]
    fn a_later_install_keeps_the_default_and_existing_toggles() {
        let (_d, p) = home();
        install(&p, "8.4", "8.4.27");
        set_extension(&p, "8.4", "xdebug", true).unwrap();
        install(&p, "8.3", "8.3.36");
        install(&p, "8.4", "8.4.28");
        let state = load(&p);
        assert_eq!(state.default.as_deref(), Some("8.4"));
        assert!(state.extensions["8.4"]["xdebug"]);
        let minors: Vec<String> = installed(&p).into_iter().map(|i| i.minor).collect();
        assert_eq!(minors, ["8.4", "8.3"]);
    }

    #[test]
    fn removing_the_default_picks_the_newest_remaining() {
        let (_d, p) = home();
        install(&p, "8.2", "8.2.35");
        install(&p, "8.4", "8.4.27");
        install(&p, "8.3", "8.3.36");
        set_default(&p, "8.4").unwrap();
        std::fs::remove_dir_all(install_dir(&p, "8.4")).unwrap();
        assert_eq!(forget(&p, "8.4").unwrap().as_deref(), Some("8.3"));
        assert!(!load(&p).extensions.contains_key("8.4"));
    }

    #[test]
    fn removing_the_last_version_leaves_no_default() {
        let (_d, p) = home();
        install(&p, "8.4", "8.4.27");
        std::fs::remove_dir_all(install_dir(&p, "8.4")).unwrap();
        assert_eq!(forget(&p, "8.4").unwrap(), None);
        assert_eq!(load(&p).default, None);
    }

    #[test]
    fn unknown_versions_and_extensions_are_refused() {
        let (_d, p) = home();
        install(&p, "8.4", "8.4.27");
        assert_eq!(set_default(&p, "9.9").unwrap_err().to_string(), "PHP 9.9 is not installed");
        assert_eq!(
            set_extension(&p, "8.4", "mongodb", true).unwrap_err().to_string(),
            "PHP 8.4 has no mongodb extension"
        );
    }

    #[test]
    fn a_folder_without_a_version_file_is_not_an_install() {
        let (_d, p) = home();
        std::fs::create_dir_all(install_dir(&p, "8.4")).unwrap();
        std::fs::create_dir_all(p.data.join("php").join("versions").join("8.4.new")).unwrap();
        assert!(installed(&p).is_empty());
    }
}
