//! Registering Hearth's MCP server with agents: their MCP config files, or Claude Code's own
//! command for its user-wide list. Other entries (Herd's `herd` too) are kept.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Cursor,
    Vscode,
}

impl Agent {
    pub fn parse(name: &str) -> Result<Agent, String> {
        match name.to_lowercase().as_str() {
            "claude" | "claude-code" | "claude_code" => Ok(Agent::Claude),
            "cursor" => Ok(Agent::Cursor),
            "vscode" | "vs-code" | "code" => Ok(Agent::Vscode),
            other => Err(format!("Hearth cannot register with `{other}`: use claude, cursor or vscode")),
        }
    }

    /// The project's config file and the key holding the servers.
    fn project_file(self, dir: &Path) -> (PathBuf, &'static str) {
        match self {
            Agent::Claude => (dir.join(".mcp.json"), "mcpServers"),
            Agent::Cursor => (dir.join(".cursor").join("mcp.json"), "mcpServers"),
            Agent::Vscode => (dir.join(".vscode").join("mcp.json"), "servers"),
        }
    }

    /// The user-wide config file (Claude Code's goes through `claude mcp add` instead).
    pub fn global_file(self) -> Option<(PathBuf, &'static str)> {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)?;
        match self {
            Agent::Claude => None,
            Agent::Cursor => Some((home.join(".cursor").join("mcp.json"), "mcpServers")),
            Agent::Vscode => {
                let base = if cfg!(windows) {
                    std::env::var_os("APPDATA").map(PathBuf::from)?
                } else {
                    home.join("Library").join("Application Support")
                };
                Some((base.join("Code").join("User").join("mcp.json"), "servers"))
            }
        }
    }

    fn entry(self, exe: &Path) -> Value {
        match self {
            Agent::Vscode => json!({ "type": "stdio", "command": exe, "args": ["mcp"] }),
            _ => json!({ "command": exe, "args": ["mcp"] }),
        }
    }
}

/// Adds the `hearth` entry to `file` under `key`, keeping everything else.
pub fn write_entry(file: &Path, key: &str, entry: Value, force: bool) -> Result<(), String> {
    let mut config: Value = match std::fs::read_to_string(file) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid JSON ({e}): fix it first, Hearth leaves it alone", file.display()))?,
        _ => json!({}),
    };
    let root = config.as_object_mut().ok_or_else(|| format!("{} holds no JSON object", file.display()))?;
    let servers = root.entry(key).or_insert_with(|| Value::Object(Map::new()));
    let servers = servers.as_object_mut().ok_or_else(|| format!("`{key}` in {} is no object", file.display()))?;
    if let Some(existing) = servers.get("hearth") {
        if *existing != entry && !force {
            return Err(format!(
                "{} already has a different `hearth` entry: pass --force to replace it",
                file.display()
            ));
        }
    }
    servers.insert("hearth".into(), entry);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(file, text + "\n").map_err(|e| format!("{}: {e}", file.display()))
}

/// Registers Hearth for `agent` in the project folder `dir`; the file written.
pub fn write_project(agent: Agent, dir: &Path, exe: &Path, force: bool) -> Result<PathBuf, String> {
    let (file, key) = agent.project_file(dir);
    write_entry(&file, key, agent.entry(exe), force)?;
    Ok(file)
}

/// Registers Hearth in `agent`'s user-wide config file; the file written.
pub fn write_global(agent: Agent, exe: &Path, force: bool) -> Result<PathBuf, String> {
    let (file, key) = agent.global_file().ok_or("This agent has no user-wide config file Hearth knows")?;
    write_entry(&file, key, agent.entry(exe), force)?;
    Ok(file)
}

/// Claude Code's own command that adds Hearth to its user-wide list.
pub fn global_claude_command(exe: &Path) -> Vec<String> {
    ["claude", "mcp", "add", "--scope", "user", "hearth", "--"]
        .iter()
        .map(|s| s.to_string())
        .chain([exe.to_string_lossy().into_owned(), "mcp".into()])
        .collect()
}

/// The program agents start: the `hearth` in Hearth's bin folder when it is there, else this one.
pub fn hearth_exe(paths: &hearth_core::HearthPaths) -> PathBuf {
    let installed = paths.bin.join(format!("hearth{}", std::env::consts::EXE_SUFFIX));
    if installed.is_file() {
        installed
    } else {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hearth"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Value};

    use super::{global_claude_command, write_project, Agent};

    fn read(p: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn claude_code_gets_an_entry_in_mcp_json_next_to_herds() {
        let d = tempfile::tempdir().unwrap();
        let exe = Path::new("C:/Users/x/.config/hearth/bin/hearth.exe");
        std::fs::write(
            d.path().join(".mcp.json"),
            r#"{"mcpServers":{"herd":{"command":"cmd","args":["/c","herd","mcp"]}}}"#,
        )
        .unwrap();
        let file = write_project(Agent::Claude, d.path(), exe, false).unwrap();
        assert_eq!(file, d.path().join(".mcp.json"));
        let config = read(&file);
        assert_eq!(config["mcpServers"]["hearth"], json!({ "command": exe, "args": ["mcp"] }));
        assert_eq!(config["mcpServers"]["herd"]["command"], "cmd", "Herd's entry stays");
        // The same entry again is fine; a different one needs --force.
        write_project(Agent::Claude, d.path(), exe, false).unwrap();
        let other = Path::new("D:/other/hearth.exe");
        let err = write_project(Agent::Claude, d.path(), other, false).unwrap_err();
        assert!(err.contains("already has a different `hearth` entry"), "{err}");
        write_project(Agent::Claude, d.path(), other, true).unwrap();
        assert_eq!(read(&file)["mcpServers"]["hearth"]["command"], json!(other));
    }

    #[test]
    fn cursor_and_vscode_use_their_own_files_and_shapes() {
        let d = tempfile::tempdir().unwrap();
        let exe = Path::new("/Users/x/.config/hearth/bin/hearth");
        let cursor = write_project(Agent::Cursor, d.path(), exe, false).unwrap();
        assert_eq!(cursor, d.path().join(".cursor").join("mcp.json"));
        assert_eq!(read(&cursor)["mcpServers"]["hearth"]["args"], json!(["mcp"]));
        let vscode = write_project(Agent::Vscode, d.path(), exe, false).unwrap();
        assert_eq!(vscode, d.path().join(".vscode").join("mcp.json"));
        assert_eq!(read(&vscode)["servers"]["hearth"], json!({ "type": "stdio", "command": exe, "args": ["mcp"] }));
    }

    #[test]
    fn a_broken_config_file_is_not_overwritten() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".mcp.json"), "{ not json").unwrap();
        let err = write_project(Agent::Claude, d.path(), Path::new("hearth"), false).unwrap_err();
        assert!(err.contains("is not valid JSON"), "{err}");
        assert_eq!(std::fs::read_to_string(d.path().join(".mcp.json")).unwrap(), "{ not json");
    }

    #[test]
    fn claude_codes_user_wide_list_is_changed_through_its_own_command() {
        let exe = Path::new(r"C:\Users\Jörg Müller\.config\hearth\bin\hearth.exe");
        assert_eq!(
            global_claude_command(exe),
            [
                "claude",
                "mcp",
                "add",
                "--scope",
                "user",
                "hearth",
                "--",
                r"C:\Users\Jörg Müller\.config\hearth\bin\hearth.exe",
                "mcp"
            ]
        );
    }
}
