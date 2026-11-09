//! The record of the move, in `<home>/config/moveover-record.json`: every step is written here before
//! it is carried out, so Undo knows what to put back and a second "Move over" skips what is done.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::herd::import::ImportPlan;
use crate::{CoreError, StokePaths};

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
        herd_entry: Value,
        stoke: String,
    },
    /// Stoke's CA before Herd's took its place (certificate and key), if it had one.
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
    /// Stoke's Redis that holds Herd's data; `created` is false for one that was there before
    /// (Undo leaves that one).
    Redis {
        id: String,
        #[serde(default = "yes")]
        created: bool,
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
    /// Kinds of the steps carried out to the end; a recorded step missing here runs again.
    #[serde(default)]
    pub finished: Vec<String>,
}

fn yes() -> bool {
    true
}

fn file(paths: &StokePaths) -> PathBuf {
    paths.config.join("moveover-record.json")
}

impl Record {
    pub fn load(paths: &StokePaths) -> Record {
        std::fs::read(file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Adds a step and writes the record, before the step itself is carried out.
    pub fn push(&mut self, paths: &StokePaths, step: Done) -> Result<(), CoreError> {
        self.steps.push(step);
        self.save(paths)
    }

    /// Private: the `ca` step keeps the CA key from before the move, for Undo.
    pub fn save(&self, paths: &StokePaths) -> Result<(), CoreError> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| CoreError::Php(e.to_string()))?;
        crate::atomic::write_private(&file(paths), &bytes)
    }

    /// Whether a step of this kind ("ports", "redis", ...) is recorded.
    pub fn has(&self, kind: &str) -> bool {
        self.steps.iter().any(|s| s.kind() == kind)
    }

    /// The recorded step of this kind.
    pub fn get(&self, kind: &str) -> Option<&Done> {
        self.steps.iter().find(|s| s.kind() == kind)
    }

    /// Whether the step of this kind was carried out to the end.
    pub fn finished(&self, kind: &str) -> bool {
        self.finished.iter().any(|k| k == kind)
    }

    /// Marks the step of this kind as carried out to the end.
    pub fn finish(&mut self, paths: &StokePaths, kind: &str) -> Result<(), CoreError> {
        if !self.finished(kind) {
            self.finished.push(kind.to_string());
        }
        self.save(paths)
    }

    /// Takes the newest step out (Undo put it back) and writes the record.
    pub fn pop(&mut self, paths: &StokePaths) -> Result<(), CoreError> {
        if let Some(step) = self.steps.pop() {
            self.finished.retain(|k| k != step.kind());
        }
        self.save(paths)
    }

    /// Forgets the record (after an Undo, or once Herd is gone).
    pub fn clear(paths: &StokePaths) -> Result<(), CoreError> {
        match std::fs::remove_file(file(paths)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(CoreError::io(file(paths), e)),
            _ => Ok(()),
        }
    }
}
