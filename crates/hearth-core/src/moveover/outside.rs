//! The move's changes outside Hearth's home, and their undo: the user PATH, Herd's autostart, Claude's
//! `herd` MCP entry, and Herd's Node manager in the machine environment (through the helper).

use super::record::Done;
use crate::dns::protocol::Request;
use crate::CoreError;

/// Where those changes land; the real one is this computer, tests pass their own.
pub trait MachineWrite {
    fn user_path(&self) -> String;
    fn set_user_path(&mut self, value: &str) -> Result<(), CoreError>;
    /// Sets (or with `None` removes) a value of the current user's `Run` key.
    fn set_run(&mut self, name: &str, command: Option<&str>) -> Result<(), CoreError>;
    /// Runs `claude` with these arguments.
    fn claude(&mut self, args: &[String]) -> Result<(), CoreError>;
    /// Sends a request to Hearth's helper service.
    fn helper(&mut self, request: &Request) -> Result<(), CoreError>;
}

fn entries(path: &str) -> Vec<String> {
    path.split(';').filter(|e| !e.trim().is_empty()).map(str::to_string).collect()
}

fn has(list: &[String], entry: &str) -> bool {
    list.iter().any(|e| e.eq_ignore_ascii_case(entry))
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| a.to_string()).collect()
}

pub fn apply_outside(m: &mut dyn MachineWrite, step: &Done) -> Result<(), CoreError> {
    match step {
        Done::UserPath { removed, added, .. } => {
            let mut next: Vec<String> = entries(&m.user_path()).into_iter().filter(|e| !has(removed, e)).collect();
            if !has(&next, added) {
                next.insert(0, added.clone());
            }
            m.set_user_path(&next.join(";"))
        }
        Done::MachineEnv { path_removed, vars_removed } => m.helper(&Request::MachineEnv {
            remove_path: path_removed.clone(),
            remove_vars: vars_removed.iter().map(|(name, _)| name.clone()).collect(),
        }),
        Done::Autostart { name, .. } => m.set_run(name, None),
        Done::Mcp { hearth, .. } => {
            m.claude(&args(&["mcp", "remove", "--scope", "user", "herd"]))?;
            m.claude(&args(&["mcp", "add", "--scope", "user", "herd", "--", hearth, "mcp"]))
        }
        _ => Ok(()),
    }
}

pub fn undo_outside(m: &mut dyn MachineWrite, step: &Done) -> Result<(), CoreError> {
    match step {
        Done::UserPath { removed, added, before } => {
            let current = m.user_path();
            let mut moved: Vec<String> = entries(before).into_iter().filter(|e| !has(removed, e)).collect();
            if !has(&moved, added) {
                moved.insert(0, added.clone());
            }
            // Untouched since the move: the exact old value. Else only Hearth's entry goes and Herd's come back.
            if current == moved.join(";") {
                return m.set_user_path(before);
            }
            let mut next: Vec<String> =
                entries(&current).into_iter().filter(|e| !e.eq_ignore_ascii_case(added)).collect();
            for entry in removed {
                if !has(&next, entry) {
                    next.push(entry.clone());
                }
            }
            m.set_user_path(&next.join(";"))
        }
        // The helper puts back exactly what it recorded taking out; nothing is named here.
        Done::MachineEnv { .. } => m.helper(&Request::MachineEnvRestore),
        Done::Autostart { name, value } => m.set_run(name, Some(value)),
        Done::Mcp { herd_entry, .. } => {
            m.claude(&args(&["mcp", "remove", "--scope", "user", "herd"]))?;
            let json = herd_entry.to_string();
            m.claude(&args(&["mcp", "add-json", "--scope", "user", "herd", &json]))
        }
        _ => Ok(()),
    }
}
