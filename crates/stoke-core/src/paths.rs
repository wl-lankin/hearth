use std::path::PathBuf;

use crate::CoreError;

/// The environment variable that moves the Stoke home, e.g. for tests or a dev build.
pub const HOME_VAR: &str = "STOKE_HOME";

/// Every directory Stoke owns, below one root (`~/.config/stoke` unless `STOKE_HOME` is set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StokePaths {
    pub root: PathBuf,
    pub bin: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub logs: PathBuf,
    pub run: PathBuf,
}

impl StokePaths {
    pub fn at(root: impl Into<PathBuf>) -> StokePaths {
        let root = root.into();
        StokePaths {
            bin: root.join("bin"),
            config: root.join("config"),
            data: root.join("data"),
            logs: root.join("logs"),
            run: root.join("run"),
            root,
        }
    }

    pub fn from_env() -> Result<StokePaths, CoreError> {
        if let Some(home) = std::env::var_os(HOME_VAR).filter(|v| !v.is_empty()) {
            return Ok(StokePaths::at(home));
        }
        let home = dirs::home_dir().ok_or(CoreError::NoHomeDir)?;
        Ok(StokePaths::at(home.join(".config").join("stoke")))
    }

    pub fn ensure(&self) -> Result<(), CoreError> {
        for dir in [&self.bin, &self.config, &self.data, &self.logs, &self.run] {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;

    /// Tests that change STOKE_HOME run one at a time.
    static ENV: Mutex<()> = Mutex::new(());

    #[test]
    fn at_builds_the_five_subdirs() {
        let p = StokePaths::at("/x");
        assert_eq!(p.root, Path::new("/x"));
        assert_eq!(p.bin, Path::new("/x/bin"));
        assert_eq!(p.config, Path::new("/x/config"));
        assert_eq!(p.data, Path::new("/x/data"));
        assert_eq!(p.logs, Path::new("/x/logs"));
        assert_eq!(p.run, Path::new("/x/run"));
    }

    #[test]
    fn from_env_prefers_stoke_home() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(HOME_VAR, dir.path());
        let p = StokePaths::from_env().unwrap();
        std::env::remove_var(HOME_VAR);
        assert_eq!(p.root, dir.path());
    }

    #[test]
    fn from_env_ignores_an_empty_stoke_home() {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(HOME_VAR, "");
        let p = StokePaths::from_env().unwrap();
        std::env::remove_var(HOME_VAR);
        assert!(p.root.ends_with(Path::new(".config").join("stoke")));
    }

    #[test]
    fn ensure_creates_dirs_under_unicode_path_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = StokePaths::at(dir.path().join("Jörg Müller").join("stoke"));
        p.ensure().unwrap();
        for d in [&p.bin, &p.config, &p.data, &p.logs, &p.run] {
            assert!(d.is_dir(), "{} missing", d.display());
        }
    }
}
