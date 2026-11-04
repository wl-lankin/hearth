//! `config/workspaces.json` (every workspace) and the `.hearth-workspace.json` marker in each one.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workspaces::database::Mode;
use crate::{atomic, CoreError, HearthPaths};

pub const MARKER: &str = ".hearth-workspace.json";

/// One workspace, with Herd's fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// `worktree`: a git worktree (made by Hearth or adopted).
    pub kind: String,
    /// The branch as typed (`feature/X`); it also finds the workspace.
    pub name: String,
    /// The host without `.test` (`feature-x.shop`): the linked site's name.
    pub site: String,
    pub url: String,
    /// The checkout's root.
    pub path: PathBuf,
    /// The served folder inside the checkout ("" for the root; a monorepo's app folder otherwise).
    #[serde(default)]
    pub app_dir: String,
    pub parent_site: String,
    /// The parent's checkout root.
    pub parent_path: PathBuf,
    pub branch: String,
    pub base_branch: String,
    pub base_commit: String,
    pub secured: bool,
    pub php_version: Option<String>,
    pub database_mode: Mode,
    pub database_name: Option<String>,
    pub database_driver: Option<String>,
    /// `cli` or `app`.
    pub created_by: String,
    /// Unix seconds.
    pub created_at: u64,
    /// Made by Hearth in its own folder (deleting may remove the folder), not adopted.
    #[serde(default)]
    pub owned: bool,
}

impl Record {
    pub fn app_path(&self) -> PathBuf {
        if self.app_dir.is_empty() {
            self.path.clone()
        } else {
            self.path.join(&self.app_dir)
        }
    }

    pub fn parent_app_path(&self) -> PathBuf {
        if self.app_dir.is_empty() {
            self.parent_path.clone()
        } else {
            self.parent_path.join(&self.app_dir)
        }
    }

    /// Whether `name` names it: the branch, the host, or the host label.
    pub fn named(&self, name: &str) -> bool {
        let name = name.trim().to_lowercase();
        let name = name.strip_suffix(".test").unwrap_or(&name);
        self.name.to_lowercase() == name
            || self.site == name
            || self.site.strip_suffix(&format!(".{}", self.parent_site)) == Some(name)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    workspaces: Vec<Record>,
}

fn file(paths: &HearthPaths) -> PathBuf {
    paths.config.join("workspaces.json")
}

/// One writer at a time (the app and a CLI call may change workspaces at once).
static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn load(paths: &HearthPaths) -> Vec<Record> {
    std::fs::read_to_string(file(paths))
        .ok()
        .and_then(|s| serde_json::from_str::<File>(&s).ok())
        .map(|f| f.workspaces)
        .unwrap_or_default()
}

fn save(paths: &HearthPaths, workspaces: Vec<Record>) -> Result<(), CoreError> {
    let json = serde_json::to_string_pretty(&File { workspaces }).map_err(|e| CoreError::Php(e.to_string()))?;
    atomic::write(&file(paths), json.as_bytes())
}

/// Adds (or replaces, same site) a record.
pub fn put(paths: &HearthPaths, record: Record) -> Result<(), CoreError> {
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = load(paths);
    all.retain(|r| r.site != record.site);
    all.push(record);
    save(paths, all)
}

pub fn remove(paths: &HearthPaths, site: &str) -> Result<(), CoreError> {
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = load(paths);
    all.retain(|r| r.site != site);
    save(paths, all)
}

/// The one workspace `name` names, or why there is none (or several).
pub fn find(paths: &HearthPaths, name: &str) -> Result<Record, CoreError> {
    let matches: Vec<Record> = load(paths).into_iter().filter(|r| r.named(name)).collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap_or_else(|| unreachable!())),
        0 => Err(CoreError::Php(format!("There is no workspace named {name}"))),
        _ => Err(CoreError::Php(format!(
            "Several workspaces are named {name}: use the host ({})",
            matches.iter().map(|r| r.site.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

/// Writes the marker and keeps it out of git (`info/exclude` of the repository).
pub fn mark(record: &Record) -> Result<(), CoreError> {
    let json = serde_json::to_string_pretty(record).map_err(|e| CoreError::Php(e.to_string()))?;
    let marker = record.path.join(MARKER);
    std::fs::write(&marker, json).map_err(|e| CoreError::io(&marker, e))?;
    let exclude = record.parent_path.join(".git").join("info").join("exclude");
    let text = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !text.lines().any(|l| l.trim() == MARKER) {
        if let Some(dir) = exclude.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&exclude, format!("{text}{sep}{MARKER}\n")).map_err(|e| CoreError::io(&exclude, e))?;
    }
    Ok(())
}

/// Whether `inner` is `outer` or inside it (case-insensitive with `/` on Windows).
pub fn within(inner: &Path, outer: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    let (inner, outer) = (norm(inner), norm(outer));
    inner == outer || inner.starts_with(&format!("{outer}/"))
}
