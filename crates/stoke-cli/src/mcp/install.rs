//! Registering Stoke's MCP server with agents: their MCP config files, or Claude Code's own
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
            other => Err(format!("Stoke cannot register with `{other}`: use claude, cursor or vscode")),
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

    fn entry(self, command: &str, env: Option<&Value>) -> Value {
        let mut entry = match self {
            Agent::Vscode => json!({ "type": "stdio", "command": command, "args": ["mcp"] }),
            _ => json!({ "command": command, "args": ["mcp"] }),
        };
        if let Some(env) = env {
            entry["env"] = env.clone();
        }
        entry
    }
}

/// Adds the `stoke` entry to `file` under `key`, keeping everything else.
pub fn write_entry(file: &Path, key: &str, entry: Value, force: bool) -> Result<(), String> {
    let mut config: Value = match std::fs::read_to_string(file) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid JSON ({e}): fix it first, Stoke leaves it alone", file.display()))?,
        _ => json!({}),
    };
    let root = config.as_object_mut().ok_or_else(|| format!("{} holds no JSON object", file.display()))?;
    let servers = root.entry(key).or_insert_with(|| Value::Object(Map::new()));
    let servers = servers.as_object_mut().ok_or_else(|| format!("`{key}` in {} is no object", file.display()))?;
    if let Some(existing) = servers.get("stoke") {
        if *existing != entry && !force {
            return Err(format!(
                "{} already has a different `stoke` entry: pass --force to replace it",
                file.display()
            ));
        }
    }
    servers.insert("stoke".into(), entry);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    stoke_core::atomic::write(file, (text + "\n").as_bytes()).map_err(|e| format!("{}: {e}", file.display()))
}

/// Registers Stoke for `agent` in the project folder `dir`; the file written.
#[cfg(test)]
pub fn write_project(agent: Agent, dir: &Path, exe: &Path, force: bool) -> Result<PathBuf, String> {
    write_project_with(agent, dir, &exe.to_string_lossy(), None, force)
}

/// As `write_project`, with the command and environment `launch` worked out.
pub fn write_project_with(
    agent: Agent,
    dir: &Path,
    command: &str,
    env: Option<&Value>,
    force: bool,
) -> Result<PathBuf, String> {
    let (file, key) = agent.project_file(dir);
    write_entry(&file, key, agent.entry(command, env), force)?;
    Ok(file)
}

/// Registers Stoke in `agent`'s user-wide config file; the file written.
pub fn write_global(agent: Agent, command: &str, env: Option<&Value>, force: bool) -> Result<PathBuf, String> {
    let (file, key) = agent.global_file().ok_or("This agent has no user-wide config file Stoke knows")?;
    write_entry(&file, key, agent.entry(command, env), force)?;
    Ok(file)
}

/// Claude Code's own command that adds Stoke to its user-wide list.
pub fn global_claude_command(command: &str, home: Option<&Path>) -> Vec<String> {
    let mut out: Vec<String> = ["claude", "mcp", "add", "--scope", "user"].iter().map(|s| s.to_string()).collect();
    if let Some(home) = home {
        out.extend(["-e".to_string(), format!("STOKE_HOME={}", home.display())]);
    }
    out.extend(["stoke".to_string(), "--".to_string(), command.to_string(), "mcp".to_string()]);
    out
}

/// How agents start the server: plain `stoke` when Stoke's `bin` is on `path_var` (config files
/// are often committed: no machine path, no user name), else the program's full path; with
/// `STOKE_HOME` when `home` is set, or the server would talk to the default home's core.
pub fn launch(exe: &Path, bin: &Path, path_var: Option<&str>, home: Option<&Path>) -> (String, Option<Value>) {
    let on_path = path_var.is_some_and(|p| std::env::split_paths(p).any(|d| same_dir(&d, bin)));
    let command = if on_path && exe.parent().is_some_and(|d| same_dir(d, bin)) {
        "stoke".to_string()
    } else {
        exe.to_string_lossy().into_owned()
    };
    (command, home.map(|h| json!({ "STOKE_HOME": h })))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    norm(a) == norm(b)
}

/// The program agents start: the `stoke` in Stoke's bin folder when it is there, else this one.
pub fn stoke_exe(paths: &stoke_core::StokePaths) -> PathBuf {
    let installed = paths.bin.join(format!("stoke{}", std::env::consts::EXE_SUFFIX));
    if installed.is_file() {
        installed
    } else {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("stoke"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::{json, Value};

    use super::{global_claude_command, launch, write_project, Agent};

    fn read(p: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn claude_code_gets_an_entry_in_mcp_json_next_to_herds() {
        let d = tempfile::tempdir().unwrap();
        let exe = Path::new("C:/Users/x/.config/stoke/bin/stoke.exe");
        std::fs::write(
            d.path().join(".mcp.json"),
            r#"{"mcpServers":{"herd":{"command":"cmd","args":["/c","herd","mcp"]}}}"#,
        )
        .unwrap();
        let file = write_project(Agent::Claude, d.path(), exe, false).unwrap();
        assert_eq!(file, d.path().join(".mcp.json"));
        let config = read(&file);
        assert_eq!(config["mcpServers"]["stoke"], json!({ "command": exe, "args": ["mcp"] }));
        assert_eq!(config["mcpServers"]["herd"]["command"], "cmd", "Herd's entry stays");
        // The same entry again is fine; a different one needs --force.
        write_project(Agent::Claude, d.path(), exe, false).unwrap();
        let other = Path::new("D:/other/stoke.exe");
        let err = write_project(Agent::Claude, d.path(), other, false).unwrap_err();
        assert!(err.contains("already has a different `stoke` entry"), "{err}");
        write_project(Agent::Claude, d.path(), other, true).unwrap();
        assert_eq!(read(&file)["mcpServers"]["stoke"]["command"], json!(other));
    }

    #[test]
    fn cursor_and_vscode_use_their_own_files_and_shapes() {
        let d = tempfile::tempdir().unwrap();
        let exe = Path::new("/Users/x/.config/stoke/bin/stoke");
        let cursor = write_project(Agent::Cursor, d.path(), exe, false).unwrap();
        assert_eq!(cursor, d.path().join(".cursor").join("mcp.json"));
        assert_eq!(read(&cursor)["mcpServers"]["stoke"]["args"], json!(["mcp"]));
        let vscode = write_project(Agent::Vscode, d.path(), exe, false).unwrap();
        assert_eq!(vscode, d.path().join(".vscode").join("mcp.json"));
        assert_eq!(read(&vscode)["servers"]["stoke"], json!({ "type": "stdio", "command": exe, "args": ["mcp"] }));
    }

    #[test]
    fn a_broken_config_file_is_not_overwritten() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".mcp.json"), "{ not json").unwrap();
        let err = write_project(Agent::Claude, d.path(), Path::new("stoke"), false).unwrap_err();
        assert!(err.contains("is not valid JSON"), "{err}");
        assert_eq!(std::fs::read_to_string(d.path().join(".mcp.json")).unwrap(), "{ not json");
    }

    #[test]
    fn claude_codes_user_wide_list_is_changed_through_its_own_command() {
        let exe = Path::new(r"C:\Users\Jörg Müller\.config\stoke\bin\stoke.exe");
        assert_eq!(
            global_claude_command(&exe.to_string_lossy(), None),
            [
                "claude",
                "mcp",
                "add",
                "--scope",
                "user",
                "stoke",
                "--",
                r"C:\Users\Jörg Müller\.config\stoke\bin\stoke.exe",
                "mcp"
            ]
        );
    }

    /// `.mcp.json` is shared and committed: a plain `stoke` when Stoke's bin folder is on PATH,
    /// so no machine path (and no user name) lands in the repository.
    #[test]
    fn the_command_is_plain_stoke_when_its_folder_is_on_path() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("stoke").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(format!("stoke{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&exe, b"").unwrap();
        let sep = if cfg!(windows) { ";" } else { ":" };
        let on_path = format!("C:\\other{sep}{}", bin.display());
        let (command, env) = launch(&exe, &bin, Some(&on_path), None);
        assert_eq!(command, "stoke");
        assert!(env.is_none());
        let (command, _) = launch(&exe, &bin, Some("C:\\other"), None);
        assert_eq!(command, exe.to_string_lossy());
        // A test home travels along, or the server would talk to the default home's core.
        let (_, env) = launch(&exe, &bin, Some(&on_path), Some(d.path()));
        assert_eq!(env, Some(json!({ "STOKE_HOME": d.path() })));
    }
}
