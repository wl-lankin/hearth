use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{atomic, CoreError, HearthPaths};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// The app settings in `<config>/settings.json`. Missing keys take their defaults, unknown keys
/// (from a newer Hearth) are ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: Theme,
    pub launch_at_login: bool,
    pub start_hidden: bool,
}

fn file(paths: &HearthPaths) -> PathBuf {
    paths.config.join("settings.json")
}

/// Never fails: a missing or unreadable file gives the defaults, and a file that does not parse is
/// kept aside as `settings.json.corrupt-<unix-ts>` so a hand edit is not lost.
pub fn load(paths: &HearthPaths) -> Settings {
    let path = file(paths);
    let Ok(text) = std::fs::read_to_string(&path) else { return Settings::default() };
    match serde_json::from_str(&text) {
        Ok(settings) => settings,
        Err(_) => {
            let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = std::fs::rename(&path, paths.config.join(format!("settings.json.corrupt-{ts}")));
            Settings::default()
        }
    }
}

pub fn save(paths: &HearthPaths, settings: &Settings) -> Result<(), CoreError> {
    let mut text = serde_json::to_string_pretty(settings).map_err(|e| CoreError::Protocol(e.to_string()))?;
    text.push('\n');
    atomic::write(&file(paths), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, HearthPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(dir.path());
        paths.ensure().unwrap();
        (dir, paths)
    }

    fn file(paths: &HearthPaths) -> std::path::PathBuf {
        paths.config.join("settings.json")
    }

    #[test]
    fn missing_file_gives_defaults() {
        let (_d, paths) = home();
        assert_eq!(load(&paths), Settings { theme: Theme::System, launch_at_login: false, start_hidden: false });
    }

    #[test]
    fn save_then_load_roundtrips() {
        let (_d, paths) = home();
        let s = Settings { theme: Theme::Dark, launch_at_login: true, start_hidden: false };
        save(&paths, &s).unwrap();
        assert_eq!(load(&paths), s);
    }

    #[test]
    fn json_uses_camel_case_keys() {
        let (_d, paths) = home();
        save(&paths, &Settings { theme: Theme::Dark, launch_at_login: true, start_hidden: true }).unwrap();
        let text = std::fs::read_to_string(file(&paths)).unwrap();
        assert!(text.contains("\"launchAtLogin\""), "{text}");
        assert!(text.contains("\"startHidden\""), "{text}");
        assert!(text.contains("\"theme\": \"dark\""), "{text}");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn unknown_and_missing_keys_are_tolerated() {
        let (_d, paths) = home();
        std::fs::write(file(&paths), r#"{"theme":"light","future":1}"#).unwrap();
        assert_eq!(load(&paths), Settings { theme: Theme::Light, ..Settings::default() });
    }

    #[test]
    fn corrupt_file_is_kept_aside() {
        let (_d, paths) = home();
        std::fs::write(file(&paths), "{not json").unwrap();
        assert_eq!(load(&paths), Settings::default());
        assert!(!file(&paths).exists());
        let aside: Vec<_> = std::fs::read_dir(&paths.config)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("settings.json.corrupt-"))
            .collect();
        assert_eq!(aside.len(), 1);
        assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), "{not json");
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let (_d, paths) = home();
        save(&paths, &Settings::default()).unwrap();
        let names: Vec<String> = std::fs::read_dir(&paths.config)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["settings.json".to_string()]);
    }
}
