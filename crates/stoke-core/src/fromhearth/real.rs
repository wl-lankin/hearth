//! This computer, for the move from Hearth.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Host, Outside, LEGACY_LOGIN_ENTRY, LEGACY_PROFILE_END, LEGACY_PROFILE_START};
use crate::moveover::machine::RealWrite;
use crate::moveover::outside::MachineWrite;
use crate::{CoreError, StokePaths};

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Hearth's app folder: `%LOCALAPPDATA%\Hearth` on Windows, `/Applications/Hearth.app` on macOS.
fn hearth_app_dir() -> PathBuf {
    if cfg!(windows) {
        dirs::data_local_dir().unwrap_or_default().join("Hearth")
    } else {
        PathBuf::from("/Applications/Hearth.app")
    }
}

/// Hearth's helper program, while it is installed.
fn hearth_helper_program() -> PathBuf {
    let programs =
        std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    programs.join("Hearth").join("hearth-helper.exe")
}

pub struct RealHost;

impl Host for RealHost {
    fn stop_programs(&mut self, folders: &[PathBuf]) {
        if cfg!(windows) {
            let folders: Vec<&Path> = folders.iter().map(PathBuf::as_path).collect();
            let own = std::process::id();
            for (pid, program) in crate::moveover::stop::processes() {
                if pid != own && crate::moveover::stop::inside(&program, &folders) {
                    crate::moveover::stop::stop_pid(pid);
                }
            }
        } else {
            for folder in folders {
                let _ = std::process::Command::new("pkill").arg("-f").arg(folder).output();
            }
        }
        // Stopped programs let go of their files a moment later.
        std::thread::sleep(Duration::from_millis(500));
    }

    fn link(&mut self, link: &Path, target: &Path) -> Result<(), CoreError> {
        #[cfg(windows)]
        {
            // A junction needs no admin rights (a directory symlink does). cmd reads a "/" in a
            // path as an option, so the paths go with backslashes.
            let backslashed = |p: &Path| p.to_string_lossy().replace('/', "\\");
            let out = crate::workspaces::command("cmd")
                .arg("/c")
                .arg("mklink")
                .arg("/J")
                .arg(backslashed(link))
                .arg(backslashed(target))
                .output()
                .map_err(|e| CoreError::io(link, e))?;
            if !out.status.success() {
                return Err(CoreError::Php(format!(
                    "Could not link {} to {}: {}",
                    link.display(),
                    target.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
            Ok(())
        }
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, link).map_err(|e| CoreError::io(link, e));
    }

    fn hearth_app(&self) -> Option<PathBuf> {
        Some(hearth_app_dir()).filter(|d| d.exists())
    }
}

pub struct RealOutside;

impl Outside for RealOutside {
    fn user_path(&self) -> Option<String> {
        if cfg!(windows) {
            RealWrite.user_path().ok()
        } else {
            None
        }
    }

    fn set_user_path(&mut self, value: &str) -> Result<(), CoreError> {
        RealWrite.set_user_path(value)
    }

    fn move_profiles(&mut self, paths: &StokePaths) -> Result<(), CoreError> {
        if cfg!(windows) {
            return Ok(());
        }
        let mut had_block = false;
        for name in [".zprofile", ".bash_profile", ".profile"] {
            let file = home().join(name);
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let without = without_legacy_block(&text);
            if without != text {
                had_block = true;
                std::fs::write(&file, without).map_err(|e| CoreError::io(&file, e))?;
            }
        }
        if had_block {
            crate::path_env::set_terminal(paths, true)?;
        }
        Ok(())
    }

    fn remove_login_entry(&mut self) -> Result<(), CoreError> {
        if cfg!(windows) {
            return RealWrite.set_run(LEGACY_LOGIN_ENTRY, None);
        }
        let agent = home().join("Library").join("LaunchAgents").join(format!("{LEGACY_LOGIN_ENTRY}.plist"));
        match std::fs::remove_file(&agent) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(CoreError::io(&agent, e)),
            _ => Ok(()),
        }
    }

    fn claude_servers(&self) -> Option<Value> {
        let bytes = std::fs::read(home().join(".claude.json")).ok()?;
        serde_json::from_slice::<Value>(&bytes).ok()?.get("mcpServers").cloned()
    }

    fn claude(&mut self, args: &[String]) -> Result<(), CoreError> {
        RealWrite.claude(args)
    }

    fn mcp_files(&self) -> Vec<PathBuf> {
        vec![home().join(".cursor").join("mcp.json")]
    }

    fn hearth_helper(&self) -> bool {
        cfg!(windows) && hearth_helper_program().is_file()
    }

    fn setup_helper(&mut self) -> Result<(), CoreError> {
        crate::dns::setup()?;
        if hearth_helper_program().is_file() {
            return Err(CoreError::Php("Hearth's .test helper is still installed".into()));
        }
        Ok(())
    }

    fn uninstall_hearth(&mut self) -> Result<(), CoreError> {
        let dir = hearth_app_dir();
        if !dir.exists() {
            return Ok(());
        }
        if cfg!(windows) {
            let program = dir.join("hearth-app.exe");
            let uninstaller = dir.join("uninstall.exe");
            if uninstaller.is_file() {
                std::process::Command::new(&uninstaller)
                    .arg("/S")
                    .status()
                    .map_err(|e| CoreError::Php(format!("Could not run Hearth's uninstaller: {e}")))?;
            }
            // The uninstaller hands over to a copy of itself and returns at once: wait for the result.
            let deadline = Instant::now() + Duration::from_secs(60);
            while program.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(500));
            }
            if program.exists() {
                return Err(CoreError::Php(format!("Hearth is still installed in {}", dir.display())));
            }
            // What the uninstaller leaves in the folder (notes of the move from Herd) stays.
            return Ok(());
        }
        let script = format!("tell application \"Finder\" to delete POSIX file \"{}\"", dir.display());
        let out = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| CoreError::Php(format!("Could not move Hearth to the Trash: {e}")))?;
        if !out.status.success() || dir.exists() {
            return Err(CoreError::Php(format!(
                "Could not move Hearth to the Trash: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }
}

/// `profile` without Hearth's block and the blank line before it.
pub fn without_legacy_block(profile: &str) -> String {
    let Some(start) = profile.find(LEGACY_PROFILE_START) else { return profile.to_string() };
    let end = profile[start..]
        .find(LEGACY_PROFILE_END)
        .map(|i| start + i + LEGACY_PROFILE_END.len())
        .unwrap_or(profile.len());
    let mut before = profile[..start].to_string();
    if before.ends_with("\n\n") {
        before.pop();
    }
    let after = profile[end..].strip_prefix('\n').unwrap_or(&profile[end..]);
    format!("{before}{after}")
}
