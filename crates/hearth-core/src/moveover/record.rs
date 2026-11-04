//! The record of the move, in `<home>/config/moveover-record.json`: every step is written here before
//! it is carried out, so Undo knows what to put back and a second "Move over" skips what is done.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::herd::import::ImportPlan;
use crate::{CoreError, HearthPaths};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "camelCase")]
pub enum Done {
    /// Herd's app and services were stopped (Undo starts Herd again).
    HerdStopped {
        app: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    UserPath {
        removed: Vec<String>,
        added: String,
        before: String,
    },
    #[serde(rename_all = "camelCase")]
    MachineEnv {
        path_removed: Vec<String>,
        vars_removed: Vec<(String, String)>,
    },
    Autostart {
        name: String,
        value: String,
    },
    #[serde(rename_all = "camelCase")]
    Mcp {
        herd_entry: serde_json::Value,
        hearth: String,
    },
    /// Hearth's CA before Herd's took its place (certificate and key), if it had one.
    Ca {
        previous: Option<(Vec<u8>, Vec<u8>)>,
    },
    Ports {
        http: u16,
        https: u16,
    },
    Smtp {
        previous: u16,
    },
    Imported {
        plan: ImportPlan,
    },
    #[serde(rename_all = "camelCase")]
    Php {
        installed: Vec<String>,
        default_before: Option<String>,
        ini_before: BTreeMap<String, String>,
    },
    Redis {
        id: String,
    },
    #[serde(rename_all = "camelCase")]
    Node {
        installed: Vec<String>,
        default_before: Option<String>,
    },
    Forwarders {
        names: Vec<String>,
    },
}

impl Done {
    /// The step's name, as written in the record.
    pub fn kind(&self) -> &'static str {
        match self {
            Done::HerdStopped { .. } => "herdStopped",
            Done::UserPath { .. } => "userPath",
            Done::MachineEnv { .. } => "machineEnv",
            Done::Autostart { .. } => "autostart",
            Done::Mcp { .. } => "mcp",
            Done::Ca { .. } => "ca",
            Done::Ports { .. } => "ports",
            Done::Smtp { .. } => "smtp",
            Done::Imported { .. } => "imported",
            Done::Php { .. } => "php",
            Done::Redis { .. } => "redis",
            Done::Node { .. } => "node",
            Done::Forwarders { .. } => "forwarders",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub steps: Vec<Done>,
}

fn file(paths: &HearthPaths) -> std::path::PathBuf {
    paths.config.join("moveover-record.json")
}

impl Record {
    pub fn load(paths: &HearthPaths) -> Record {
        std::fs::read(file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Adds a step and writes the record, before the step itself is carried out.
    pub fn push(&mut self, paths: &HearthPaths, step: Done) -> Result<(), CoreError> {
        self.steps.push(step);
        self.save(paths)
    }

    pub fn save(&self, paths: &HearthPaths) -> Result<(), CoreError> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| CoreError::Php(e.to_string()))?;
        crate::atomic::write(&file(paths), &bytes)
    }

    /// Whether a step of this kind ("ports", "redis", ...) is recorded.
    pub fn has(&self, kind: &str) -> bool {
        self.steps.iter().any(|s| s.kind() == kind)
    }

    /// Forgets the record (after an Undo, or once Herd is gone).
    pub fn clear(paths: &HearthPaths) -> Result<(), CoreError> {
        match std::fs::remove_file(file(paths)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(CoreError::io(file(paths), e)),
            _ => Ok(()),
        }
    }
}
