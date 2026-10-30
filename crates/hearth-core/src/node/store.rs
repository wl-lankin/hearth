//! What is installed: `<data>/node/v<version>/` per version (with the catalog entry it came from),
//! the default in `<config>/node.json`, and `<data>/node/current`, a link to the default version
//! (a junction on Windows, which needs no extra rights; a symlink on macOS).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::php::manifest::{compare_versions, NodeBuild};
use crate::php::service::BUILD_FILE;
use crate::{CoreError, HearthPaths};

pub fn node_dir(paths: &HearthPaths) -> PathBuf {
    paths.data.join("node")
}

pub fn version_dir(paths: &HearthPaths, version: &str) -> PathBuf {
    node_dir(paths).join(format!("v{version}"))
}

pub fn current_link(paths: &HearthPaths) -> PathBuf {
    node_dir(paths).join("current")
}

/// The folder holding `node` (and the programs `npm i -g` adds) inside an install.
pub fn bin_dir(install: &Path) -> PathBuf {
    if cfg!(windows) {
        install.to_path_buf()
    } else {
        install.join("bin")
    }
}

pub fn node_program(install: &Path) -> PathBuf {
    bin_dir(install).join(format!("node{}", std::env::consts::EXE_SUFFIX))
}

/// An installed version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub lts: Option<String>,
    pub dir: PathBuf,
}

/// The installed versions, newest first. Only a folder with its catalog entry counts (it is written
/// last); `.new` / `.old` folders are an install in progress.
pub fn installed(paths: &HearthPaths) -> Vec<Installed> {
    let Ok(entries) = std::fs::read_dir(node_dir(paths)) else { return vec![] };
    let mut list: Vec<Installed> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let version = name.strip_prefix('v')?;
            if version.ends_with(".new") || version.ends_with(".old") {
                return None;
            }
            let build: NodeBuild = serde_json::from_slice(&std::fs::read(e.path().join(BUILD_FILE)).ok()?).ok()?;
            Some(Installed { version: version.to_string(), lts: build.lts, dir: e.path() })
        })
        .collect();
    list.sort_by(|a, b| compare_versions(&b.version, &a.version));
    list
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    default: Option<String>,
}

fn state_path(paths: &HearthPaths) -> PathBuf {
    paths.config.join("node.json")
}

/// The default version, if it is still installed.
pub fn default_version(paths: &HearthPaths) -> Option<String> {
    let state: State = serde_json::from_slice(&std::fs::read(state_path(paths)).ok()?).ok()?;
    let version = state.default?;
    installed(paths).iter().any(|i| i.version == version).then_some(version)
}

/// Makes `version` the default (`None`: no default) and points `current` at it.
pub fn set_default(paths: &HearthPaths, version: Option<&str>) -> Result<(), CoreError> {
    if let Some(v) = version {
        if !installed(paths).iter().any(|i| i.version == v) {
            return Err(CoreError::Php(format!("Node {v} is not installed")));
        }
    }
    let json = serde_json::to_vec_pretty(&State { default: version.map(String::from) })
        .map_err(|e| CoreError::Php(e.to_string()))?;
    crate::atomic::write(&state_path(paths), &json)?;
    link_current(paths, version.map(|v| version_dir(paths, v)).as_deref())
}

/// Points `current` at `target`, or removes it.
fn link_current(paths: &HearthPaths, target: Option<&Path>) -> Result<(), CoreError> {
    let link = current_link(paths);
    if std::fs::symlink_metadata(&link).is_ok() {
        // A junction or a directory symlink on Windows is removed like a folder (only the link
        // goes); a symlink on macOS like a file.
        let removed = if cfg!(windows) { std::fs::remove_dir(&link) } else { std::fs::remove_file(&link) };
        removed.map_err(|e| CoreError::io(&link, e))?;
    }
    let Some(target) = target else { return Ok(()) };
    #[cfg(windows)]
    junction::create(target, &link).map_err(|e| CoreError::io(&link, e))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &link).map_err(|e| CoreError::io(&link, e))?;
    Ok(())
}
