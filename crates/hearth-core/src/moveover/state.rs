//! Where the move stands, in `<home>/config/moveover.json`: Herd's answers before the move, whether
//! it was moved and uninstalled, and the check results the owner accepted.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::check::SiteCheck;
use super::probe::Answer;
use crate::{CoreError, HearthPaths};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MoveState {
    pub baseline: Vec<Answer>,
    pub moved: bool,
    pub uninstalled: bool,
    /// `<scheme>://<host>` of check results the owner accepted.
    pub accepted: Vec<String>,
    /// The last check after the move (the uninstall waits for it).
    pub last_check: Vec<SiteCheck>,
}

fn file(paths: &HearthPaths) -> PathBuf {
    paths.config.join("moveover.json")
}

pub fn load(paths: &HearthPaths) -> MoveState {
    std::fs::read(file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(paths: &HearthPaths, state: &MoveState) -> Result<(), CoreError> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| CoreError::Php(e.to_string()))?;
    crate::atomic::write(&file(paths), &bytes)
}
