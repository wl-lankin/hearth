//! Moving from Hearth, Stoke's old name. The first start of Stoke finds Hearth's home
//! (`~/.config/hearth`), stops Hearth, moves the home to `~/.config/stoke` and renames what still
//! carries the old name, before anything else touches the new home. A link keeps the old path
//! working for anything outside that still points there (an IDE's PHP, a script). Once the core
//! runs, the changes outside the home follow: the user PATH, Hearth's login entry, Claude's and
//! Cursor's MCP entries, the .test helper (one UAC prompt) and Hearth's app, which is uninstalled.
//! Every step is recorded when it is done, so a later start carries out only what is left.

pub mod api;
pub mod real;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{atomic, CoreError, StokePaths};

/// Moves a test or dev run's Hearth home somewhere else (tests and hands-on checks only).
pub const LEGACY_VAR: &str = "STOKE_LEGACY_HOME";
/// The marker line of Hearth's shims, so they are recognized as Stoke's own and replaced.
pub const LEGACY_SHIM_MARKER: &str = "hearth-shim";
/// Hearth's hosts block markers; the helper replaces that block with its own.
pub const LEGACY_HOSTS_START: &str = "# hearth-start";
pub const LEGACY_HOSTS_END: &str = "# hearth-end";
/// Hearth's block in the macOS shell profiles.
pub const LEGACY_PROFILE_START: &str = "# >>> hearth >>>";
pub const LEGACY_PROFILE_END: &str = "# <<< hearth <<<";
/// Hearth's Windows service; the helper's install removes it.
pub const LEGACY_SERVICE: &str = "HearthHelper";
/// Hearth's "Start at login" entry (the current user's `Run` key).
pub const LEGACY_LOGIN_ENTRY: &str = "Hearth";

const LEGACY_DOTFILE: &str = ".hearth-";
const LEGACY_WORKSPACE_MARKER: &str = ".hearth-workspace.json";
const RECORD: &str = "fromhearth.json";

/// The steps after the core runs, in the order they are carried out.
pub const STEPS: [&str; 5] = ["path", "login", "mcp", "helper", "app"];

/// Hearth's home: `~/.config/hearth`, or `STOKE_LEGACY_HOME`. A test or dev home (`STOKE_HOME`)
/// never moves the real Hearth home, so then only `STOKE_LEGACY_HOME` counts.
pub fn legacy_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os(LEGACY_VAR).filter(|v| !v.is_empty()) {
        return Some(home.into());
    }
    if std::env::var_os(crate::paths::HOME_VAR).is_some_and(|v| !v.is_empty()) {
        return None;
    }
    dirs::home_dir().map(|h| h.join(".config").join("hearth"))
}

/// Whether Hearth's home is there to move: a real folder (not the link a move leaves behind) while
/// Stoke has no home yet.
pub fn due(legacy: &Path, root: &Path) -> bool {
    std::fs::symlink_metadata(legacy).is_ok_and(|m| m.is_dir()) && !root.exists()
}

/// What the move did and what is left; kept in `<home>/config/fromhearth.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Hearth's home as it was (the old root), to map paths that still point there.
    pub legacy: PathBuf,
    /// True once the home was moved and everything in it renamed.
    pub home: bool,
    /// Steps (of `STEPS`) carried out to the end.
    pub finished: Vec<String>,
    /// The last error per step that failed; it runs again on the next start or "Try again".
    pub errors: BTreeMap<String, String>,
    /// True once the user closed the note about the move.
    #[serde(default)]
    pub seen: bool,
}

impl Record {
    fn file(paths: &StokePaths) -> PathBuf {
        paths.config.join(RECORD)
    }

    /// The record, if Stoke was moved from Hearth.
    pub fn load(paths: &StokePaths) -> Option<Record> {
        std::fs::read(Self::file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok())
    }

    pub fn save(&self, paths: &StokePaths) -> Result<(), CoreError> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| CoreError::Php(e.to_string()))?;
        atomic::write(&Self::file(paths), &bytes)
    }

    pub fn finished(&self, step: &str) -> bool {
        self.finished.iter().any(|s| s == step)
    }

    /// Whether anything is left to do.
    pub fn pending(&self) -> bool {
        !self.home || STEPS.iter().any(|s| !self.finished(s))
    }
}

/// What the move needs from the computer before the core runs; tests pass their own.
pub trait Host {
    /// Stops every process whose program lies in one of `folders` (Hearth's app and home).
    fn stop_programs(&mut self, folders: &[PathBuf]);
    /// Leaves `link` pointing at `target` (a junction on Windows, a symlink on macOS).
    fn link(&mut self, link: &Path, target: &Path) -> Result<(), CoreError>;
    /// Where Hearth's app is installed, if it is.
    fn hearth_app(&self) -> Option<PathBuf>;
}

/// Moves Hearth's home to `paths.root` and renames what carries Hearth's name. Runs before the home
/// is prepared; when the home was moved by an earlier start that did not get to the end, only the
/// renaming runs (it is safe to repeat).
pub fn move_home(legacy: &Path, paths: &StokePaths, host: &mut dyn Host) -> Result<(), CoreError> {
    if due(legacy, &paths.root) {
        let mut folders = vec![legacy.to_path_buf()];
        folders.extend(host.hearth_app());
        host.stop_programs(&folders);
        rename_with_retries(legacy, &paths.root)?;
        let record = Record { legacy: legacy.to_path_buf(), ..Record::default() };
        record.save(paths)?;
        // Only a convenience for what still points at the old path: the move goes on without it.
        if let Err(e) = host.link(legacy, &paths.root) {
            eprintln!("Stoke could not leave a link at Hearth's old folder: {e}");
        }
    }
    let Some(mut record) = Record::load(paths) else { return Ok(()) };
    if !record.home {
        fix_home(paths, &record.legacy)?;
        record.home = true;
        record.save(paths)?;
    }
    Ok(())
}

/// A folder another program still holds is let go a moment after that program stopped.
fn rename_with_retries(from: &Path, to: &Path) -> Result<(), CoreError> {
    let mut tries = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(_) if tries < 20 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(e) => {
                return Err(CoreError::Php(format!(
                "Could not move Hearth's folder {} to {}: {e}. Close every program that uses it and start Stoke again.",
                from.display(),
                to.display()
            )))
            }
        }
    }
}

/// Renames and rewrites everything in the moved home that still names Hearth or its old folder.
pub fn fix_home(paths: &StokePaths, legacy: &Path) -> Result<(), CoreError> {
    // Version files of every PHP and Node install (`.hearth-build.json`, `.hearth-version`).
    for dir in [paths.data.join("php").join("versions"), paths.data.join("node")] {
        for install in subdirs(&dir) {
            rename_dotfiles(&install)?;
        }
    }
    // Settings with absolute paths into the old folder (sites, forwarders, workspaces, ...).
    for file in json_files(&paths.config) {
        rewrite_json_file(&file, legacy, &paths.root)?;
    }
    // Workspace markers in the user's checkouts.
    for folder in workspace_folders(paths) {
        let (old, new) = (folder.join(LEGACY_WORKSPACE_MARKER), folder.join(crate::workspaces::registry::MARKER));
        if old.is_file() && !new.exists() {
            std::fs::rename(&old, &new).map_err(|e| CoreError::io(&old, e))?;
        }
        replace_in_text_file(
            &folder.join(".git").join("info").join("exclude"),
            LEGACY_WORKSPACE_MARKER,
            crate::workspaces::registry::MARKER,
        )?;
    }
    // Hearth's CLI; Stoke puts its own in place on start.
    for name in ["hearth.exe", "hearth.exe.old", "hearth"] {
        let _ = std::fs::remove_file(paths.bin.join(name));
    }
    // Runtime files of the stopped Hearth (lock, sockets).
    if let Ok(entries) = std::fs::read_dir(&paths.run) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        }
    }
    // php.ini and the shims name the home's folder: write them again for the new one.
    // One damaged PHP install must not stop the move; its php.ini is written on its reinstall.
    for install in crate::php::store::installed(paths) {
        if let Err(e) = crate::php::service::apply_config(paths, &install.minor) {
            eprintln!("Stoke could not write php.ini for PHP {}: {e}", install.minor);
        }
    }
    crate::php::service::refresh_shims(paths)
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

fn rename_dotfiles(dir: &Path) -> Result<(), CoreError> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(rest) = name.strip_prefix(LEGACY_DOTFILE) {
            let target = dir.join(format!(".stoke-{rest}"));
            if !target.exists() {
                std::fs::rename(entry.path(), &target).map_err(|e| CoreError::io(entry.path(), e))?;
            }
        }
    }
    Ok(())
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(json_files(&path));
        } else if path.extension().is_some_and(|e| e == "json") {
            found.push(path);
        }
    }
    found
}

fn rewrite_json_file(file: &Path, legacy: &Path, root: &Path) -> Result<(), CoreError> {
    let Ok(bytes) = std::fs::read(file) else { return Ok(()) };
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else { return Ok(()) };
    if !rewrite_paths(&mut value, legacy, root) {
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| CoreError::Php(e.to_string()))?;
    atomic::write(file, &bytes)
}

/// Replaces `legacy` at the start of every string in `value` (keys too) by `root`; true when
/// anything changed.
pub fn rewrite_paths(value: &mut Value, legacy: &Path, root: &Path) -> bool {
    match value {
        Value::String(s) => match moved(s, legacy, root) {
            Some(new) => {
                *s = new;
                true
            }
            None => false,
        },
        Value::Array(items) => items.iter_mut().fold(false, |changed, v| rewrite_paths(v, legacy, root) | changed),
        Value::Object(map) => {
            let mut changed = false;
            let old = std::mem::take(map);
            for (key, mut v) in old {
                changed |= rewrite_paths(&mut v, legacy, root);
                let key = match moved(&key, legacy, root) {
                    Some(new) => {
                        changed = true;
                        new
                    }
                    None => key,
                };
                map.insert(key, v);
            }
            changed
        }
        _ => false,
    }
}

/// `text` with the folder `legacy` at its start replaced by `root`; `None` when it does not start
/// with that folder. Windows compares without case and accepts either slash.
pub fn moved(text: &str, legacy: &Path, root: &Path) -> Option<String> {
    let legacy = legacy.to_string_lossy();
    let legacy = legacy.trim_end_matches(['/', '\\']);
    let head = text.get(..legacy.len())?;
    let same = if cfg!(windows) {
        head.replace('/', "\\").eq_ignore_ascii_case(&legacy.replace('/', "\\"))
    } else {
        head == legacy
    };
    let rest = &text[legacy.len()..];
    (same && (rest.is_empty() || rest.starts_with(['/', '\\']))).then(|| format!("{}{rest}", root.to_string_lossy()))
}

fn workspace_folders(paths: &StokePaths) -> Vec<PathBuf> {
    let file = paths.config.join("workspaces.json");
    let Some(value) = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
        return Vec::new();
    };
    let items = value.as_array().cloned().or_else(|| value.get("workspaces").and_then(Value::as_array).cloned());
    items.unwrap_or_default().iter().filter_map(|w| w.get("path").and_then(Value::as_str).map(PathBuf::from)).collect()
}

fn replace_in_text_file(file: &Path, from: &str, to: &str) -> Result<(), CoreError> {
    let Ok(text) = std::fs::read_to_string(file) else { return Ok(()) };
    if text.contains(from) {
        std::fs::write(file, text.replace(from, to)).map_err(|e| CoreError::io(file, e))?;
    }
    Ok(())
}

/// What the steps after the start need from the computer; tests pass their own.
pub trait Outside {
    /// The user PATH as stored (Windows); `None` elsewhere.
    fn user_path(&self) -> Option<String>;
    fn set_user_path(&mut self, value: &str) -> Result<(), CoreError>;
    /// Moves Stoke's terminal setup to the new home where it is not the registry (macOS profiles).
    fn move_profiles(&mut self, paths: &StokePaths) -> Result<(), CoreError>;
    /// Removes Hearth's "Start at login" entry.
    fn remove_login_entry(&mut self) -> Result<(), CoreError>;
    /// Claude's user-wide MCP entries (`mcpServers` in `~/.claude.json`), if Claude is set up.
    fn claude_servers(&self) -> Option<Value>;
    /// Runs `claude` with these arguments.
    fn claude(&mut self, args: &[String]) -> Result<(), CoreError>;
    /// Other MCP config files with user-wide entries, written directly (Cursor's).
    fn mcp_files(&self) -> Vec<PathBuf>;
    /// Whether Hearth's .test helper is still installed.
    fn hearth_helper(&self) -> bool;
    /// Installs Stoke's helper, which takes Hearth's out (one UAC prompt).
    fn setup_helper(&mut self) -> Result<(), CoreError>;
    /// Uninstalls Hearth's app, if it is installed; an error when it is still there afterwards.
    fn uninstall_hearth(&mut self) -> Result<(), CoreError>;
}

/// Carries out the steps after the start that are not done yet, recording each one.
pub fn finish(paths: &StokePaths, outside: &mut dyn Outside) -> Option<Record> {
    let mut record = Record::load(paths)?;
    if !record.home {
        return Some(record);
    }
    for step in STEPS {
        if record.finished(step) {
            continue;
        }
        let result = match step {
            "path" => move_path(paths, &record.legacy, outside),
            "login" => outside.remove_login_entry(),
            "mcp" => move_mcp_entries(paths, &record.legacy, outside),
            "helper" if outside.hearth_helper() => outside.setup_helper(),
            "helper" => Ok(()),
            _ => outside.uninstall_hearth(),
        };
        match result {
            Ok(()) => {
                record.finished.push(step.to_string());
                record.errors.remove(step);
            }
            Err(e) => {
                record.errors.insert(step.to_string(), e.to_string());
            }
        }
        if let Err(e) = record.save(paths) {
            eprintln!("Stoke could not save the record of the move from Hearth: {e}");
        }
    }
    Some(record)
}

fn move_path(paths: &StokePaths, legacy: &Path, outside: &mut dyn Outside) -> Result<(), CoreError> {
    outside.move_profiles(paths)?;
    let Some(current) = outside.user_path() else { return Ok(()) };
    let next = moved_path(&current, legacy, &paths.root);
    if next != current {
        outside.set_user_path(&next)?;
    }
    Ok(())
}

/// The user PATH with every entry inside Hearth's home moved to Stoke's, in place; an entry that is
/// then there twice is kept once.
pub fn moved_path(value: &str, legacy: &Path, root: &Path) -> String {
    let mut out: Vec<String> = Vec::new();
    for entry in value.split(';').filter(|e| !e.trim().is_empty()) {
        let entry = moved(entry, legacy, root).unwrap_or_else(|| entry.to_string());
        if !out.iter().any(|e| e.trim_end_matches('\\').eq_ignore_ascii_case(entry.trim_end_matches('\\'))) {
            out.push(entry);
        }
    }
    out.join(";")
}

fn move_mcp_entries(paths: &StokePaths, legacy: &Path, outside: &mut dyn Outside) -> Result<(), CoreError> {
    // Claude through its CLI: it owns `~/.claude.json` and writes it while it runs.
    if let Some(servers) = outside.claude_servers() {
        for (old, new, entry) in moved_entries(&servers, legacy, &paths.root) {
            let _ = outside.claude(&args(&["mcp", "remove", "--scope", "user", &old]));
            outside.claude(&args(&["mcp", "add-json", "--scope", "user", &new, &entry.to_string()]))?;
        }
    }
    for file in outside.mcp_files() {
        let Ok(bytes) = std::fs::read(&file) else { continue };
        let Ok(mut config) = serde_json::from_slice::<Value>(&bytes) else { continue };
        let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) else { continue };
        let moves = moved_entries(&Value::Object(servers.clone()), legacy, &paths.root);
        if moves.is_empty() {
            continue;
        }
        for (old, new, entry) in moves {
            servers.remove(&old);
            servers.insert(new, entry);
        }
        let bytes = serde_json::to_vec_pretty(&config).map_err(|e| CoreError::Php(e.to_string()))?;
        atomic::write(&file, &bytes)?;
    }
    Ok(())
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| a.to_string()).collect()
}

/// The MCP entries that run Hearth's CLI, pointed at Stoke's, as (old name, new name, entry): a
/// `hearth` entry becomes `stoke` (unless there is one), any other keeps its name (`herd`). Entries
/// that do not run Hearth's CLI are left out.
pub fn moved_entries(servers: &Value, legacy: &Path, root: &Path) -> Vec<(String, String, Value)> {
    let Some(map) = servers.as_object() else { return Vec::new() };
    let mut moves = Vec::new();
    for (name, entry) in map {
        let Some(command) = entry.get("command").and_then(Value::as_str) else { continue };
        let Some(new) = moved(command, legacy, root) else { continue };
        let new = new
            .strip_suffix("hearth.exe")
            .map(|base| format!("{base}stoke.exe"))
            .or_else(|| new.strip_suffix("hearth").map(|base| format!("{base}stoke")))
            .unwrap_or(new);
        let mut entry = entry.clone();
        entry["command"] = Value::String(new);
        let new_name = if name == "hearth" && !map.contains_key("stoke") { "stoke" } else { name };
        moves.push((name.clone(), new_name.to_string(), entry));
    }
    moves
}
