//! `config/processes.json`: site name -> its processes.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{atomic, CoreError, StokePaths};

/// One long-running command of a site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proc {
    /// Unique per site: 1 to 32 lowercase letters, digits or dashes.
    pub name: String,
    /// Run by the platform shell in the site's folder, as typed.
    pub command: String,
    /// Starts with Stoke.
    #[serde(default)]
    pub autostart: bool,
}

pub type Processes = BTreeMap<String, Vec<Proc>>;

fn file(paths: &StokePaths) -> PathBuf {
    paths.config.join("processes.json")
}

/// Every site's processes; a missing or unreadable file has none.
pub fn load(paths: &StokePaths) -> Processes {
    std::fs::read_to_string(file(paths)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(paths: &StokePaths, all: &Processes) -> Result<(), CoreError> {
    let json = serde_json::to_string_pretty(all).map_err(|e| CoreError::Php(e.to_string()))?;
    atomic::write(&file(paths), json.as_bytes())
}

/// Whether `name` can name a process (it is part of the supervisor key and the log path).
pub fn valid_name(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Adds a process to `site`; refuses a bad or taken name and an empty command.
pub fn add(paths: &StokePaths, site: &str, proc: Proc) -> Result<(), CoreError> {
    if !valid_name(&proc.name) {
        return Err(CoreError::Php("Use 1 to 32 lowercase letters, digits or dashes for the name".into()));
    }
    if proc.command.trim().is_empty() {
        return Err(CoreError::Php("The command is empty".into()));
    }
    // cmd runs only the first line of several; tabs are fine.
    if proc.command.chars().any(|c| c.is_control() && c != '\t') {
        return Err(CoreError::Php("The command must be one line".into()));
    }
    let mut all = load(paths);
    let list = all.entry(site.to_string()).or_default();
    if list.iter().any(|p| p.name == proc.name) {
        return Err(CoreError::Php(format!("{site} already has a process named {}", proc.name)));
    }
    list.push(Proc { command: proc.command.trim().to_string(), ..proc });
    save(paths, &all)
}

/// The process `name` of `site`.
pub fn get(paths: &StokePaths, site: &str, name: &str) -> Result<Proc, CoreError> {
    let all = load(paths);
    let found = all.get(site).and_then(|list| list.iter().find(|p| p.name == name)).cloned();
    found.ok_or_else(|| CoreError::Php(format!("{site} has no process named {name}")))
}

pub fn remove(paths: &StokePaths, site: &str, name: &str) -> Result<(), CoreError> {
    get(paths, site, name)?;
    let mut all = load(paths);
    if let Some(list) = all.get_mut(site) {
        list.retain(|p| p.name != name);
        if list.is_empty() {
            all.remove(site);
        }
    }
    save(paths, &all)
}

/// Forgets every process of `site` (the site is gone).
pub fn forget_site(paths: &StokePaths, site: &str) -> Result<(), CoreError> {
    let mut all = load(paths);
    if all.remove(site).is_some() {
        save(paths, &all)?;
    }
    Ok(())
}
