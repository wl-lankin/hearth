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

/// What is on disk: nothing, valid settings, or a file Hearth cannot read as settings.
enum OnDisk {
    Missing,
    Valid(Settings),
    Unparsed,
}

fn read(paths: &HearthPaths) -> OnDisk {
    let bytes = match std::fs::read(file(paths)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return OnDisk::Missing,
        Err(_) => return OnDisk::Unparsed,
    };
    // Some Windows editors and PowerShell's `Set-Content -Encoding UTF8` write a byte order mark.
    let json = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    match serde_json::from_slice(json) {
        Ok(settings) => OnDisk::Valid(settings),
        Err(_) => OnDisk::Unparsed,
    }
}

/// Moves an unparsed settings.json to `settings.json.corrupt-<unix-ts>` so a hand edit is not lost.
fn keep_aside(paths: &HearthPaths) -> Result<(), CoreError> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let from = file(paths);
    std::fs::rename(&from, paths.config.join(format!("settings.json.corrupt-{ts}"))).map_err(|e| CoreError::io(from, e))
}

/// Never fails: a missing file gives the defaults, and a file that does not parse (also one in
/// another encoding) is kept aside as `settings.json.corrupt-<unix-ts>` before the defaults apply.
pub fn load(paths: &HearthPaths) -> Settings {
    match read(paths) {
        OnDisk::Valid(settings) => settings,
        OnDisk::Missing => Settings::default(),
        OnDisk::Unparsed => {
            let _ = keep_aside(paths);
            Settings::default()
        }
    }
}

/// Saves the settings. A file that does not parse is kept aside first; if that fails, nothing is
/// written, so a hand edit is never overwritten.
pub fn save(paths: &HearthPaths, settings: &Settings) -> Result<(), CoreError> {
    if let OnDisk::Unparsed = read(paths) {
        keep_aside(paths)?;
    }
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

    fn aside_files(paths: &HearthPaths) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(&paths.config)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("settings.json.corrupt-"))
            .collect()
    }

    #[test]
    fn utf16_file_is_kept_aside() {
        // What Windows PowerShell 5.1 `>` writes.
        let (_d, paths) = home();
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(r#"{"theme":"dark"}"#.encode_utf16().flat_map(|u| u.to_le_bytes()));
        std::fs::write(file(&paths), &bytes).unwrap();
        assert_eq!(load(&paths), Settings::default());
        let aside = aside_files(&paths);
        assert_eq!(aside.len(), 1);
        assert_eq!(std::fs::read(&aside[0]).unwrap(), bytes);
    }

    #[test]
    fn utf8_bom_is_accepted() {
        let (_d, paths) = home();
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"theme":"dark"}"#);
        std::fs::write(file(&paths), &bytes).unwrap();
        assert_eq!(load(&paths).theme, Theme::Dark);
        assert!(aside_files(&paths).is_empty());
    }

    #[test]
    fn save_never_overwrites_an_unparsed_file() {
        // A file that appeared, or could not be set aside, after the last load.
        let (_d, paths) = home();
        std::fs::write(file(&paths), "{hand edit").unwrap();
        save(&paths, &Settings::default()).unwrap();
        let aside = aside_files(&paths);
        assert_eq!(aside.len(), 1);
        assert_eq!(std::fs::read_to_string(&aside[0]).unwrap(), "{hand edit");
        assert_eq!(load(&paths), Settings::default());
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
