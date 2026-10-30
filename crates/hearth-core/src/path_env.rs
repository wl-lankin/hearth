//! "Use Hearth's PHP in terminals": puts `<home>/bin` first on the user's PATH (Windows: the user
//! environment in the registry; macOS: a marked block in the shell profiles) and takes it out again.
//! The global tools of the default Node (`npm i -g`, through the `current` link) come right after.

use std::path::{Path, PathBuf};

use crate::{atomic, CoreError};

pub const START: &str = "# >>> hearth >>>";
pub const END: &str = "# <<< hearth <<<";

fn same(a: &str, b: &str, windows: bool) -> bool {
    if windows {
        // Windows paths are case-insensitive beyond ASCII too (Jörg / JÖRG).
        a.trim_end_matches('\\').to_lowercase() == b.trim_end_matches('\\').to_lowercase()
    } else {
        a.trim_end_matches('/') == b.trim_end_matches('/')
    }
}

/// `path_value` with `entry` first, exactly once.
pub fn with_entry(path_value: &str, entry: &str, sep: char, windows: bool) -> String {
    let rest = without_entry(path_value, entry, sep, windows);
    if rest.is_empty() {
        entry.to_string()
    } else {
        format!("{entry}{sep}{rest}")
    }
}

/// `path_value` with `entries` first, in this order, each exactly once.
pub fn with_entries(path_value: &str, entries: &[String], sep: char, windows: bool) -> String {
    entries.iter().rev().fold(path_value.to_string(), |value, entry| with_entry(&value, entry, sep, windows))
}

/// The folders Hearth puts on PATH: `<home>/bin`, then the default Node's global tools.
pub fn terminal_entries(paths: &crate::HearthPaths) -> Vec<PathBuf> {
    vec![paths.bin.clone(), crate::node::store::bin_dir(&crate::node::store::current_link(paths))]
}

/// `path_value` without `entry`; every other entry (also empty ones and %VARIABLES%) is kept as is.
pub fn without_entry(path_value: &str, entry: &str, sep: char, windows: bool) -> String {
    path_value.split(sep).filter(|e| !same(e, entry, windows)).collect::<Vec<_>>().join(&sep.to_string())
}

/// Other PATH folders that also hold a `php` command (e.g. Herd's), so the UI can mention them.
pub fn others_with_php(
    path_value: &str,
    ours: &str,
    sep: char,
    windows: bool,
    has_php: impl Fn(&str) -> bool,
) -> Vec<String> {
    path_value
        .split(sep)
        .filter(|e| !e.is_empty() && !same(e, ours, windows) && has_php(e))
        .map(str::to_string)
        .collect()
}

/// `profile` with Hearth's block at the end (replacing an existing one), after a blank line.
pub fn with_block(profile: &str, line: &str) -> String {
    let mut base = without_block(profile);
    if !base.is_empty() {
        if !base.ends_with('\n') {
            base.push('\n');
        }
        base.push('\n');
    }
    format!("{base}{START}\n{line}\n{END}\n")
}

/// `profile` without Hearth's block and the blank line Hearth put before it.
pub fn without_block(profile: &str) -> String {
    let Some(start) = profile.find(START) else { return profile.to_string() };
    let end = profile[start..].find(END).map(|i| start + i + END.len()).unwrap_or(profile.len());
    let mut before = profile[..start].to_string();
    if before.ends_with("\n\n") {
        before.pop();
    }
    let after = profile[end..].strip_prefix('\n').unwrap_or(&profile[end..]);
    format!("{before}{after}")
}

pub fn profile_has_block(files: &[PathBuf]) -> bool {
    files.iter().any(|f| std::fs::read_to_string(f).is_ok_and(|t| t.contains(START)))
}

/// The shell profiles to edit: existing ones only (a new `.bash_profile` would hide a bash user's
/// `.profile`). zsh reads `.zprofile`; bash reads `.bash_profile`, else `.profile`. With none of
/// them, `.zprofile` is created (zsh is the macOS default shell).
pub fn profile_targets(home: &Path, exists: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let (zsh, bash, profile) = (home.join(".zprofile"), home.join(".bash_profile"), home.join(".profile"));
    let mut targets = Vec::new();
    if exists(&zsh) {
        targets.push(zsh.clone());
    }
    if exists(&bash) {
        targets.push(bash);
    } else if exists(&profile) {
        targets.push(profile);
    }
    if targets.is_empty() {
        targets.push(zsh);
    }
    targets
}

/// Adds (`on`) or removes Hearth's block in each profile file; files are written only when they
/// change, and a symlinked profile (dotfile managers) is written through, keeping the link.
pub fn apply_profiles(files: &[PathBuf], line: &str, on: bool) -> Result<(), CoreError> {
    for file in files {
        let current = std::fs::read_to_string(file).unwrap_or_default();
        let next = if on { with_block(&current, line) } else { without_block(&current) };
        if next != current {
            let target = std::fs::canonicalize(file).unwrap_or_else(|_| file.clone());
            atomic::write(&target, next.as_bytes())?;
        }
    }
    Ok(())
}

/// True when `dir` holds a `php` command.
pub fn has_php(dir: &str) -> bool {
    let dir = Path::new(dir);
    ["php", "php.bat", "php.exe", "php.cmd"].iter().any(|name| dir.join(name).is_file())
}

/// Whether terminals use Hearth's PHP, and which other folders on PATH also have a `php`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStatus {
    pub enabled: bool,
    pub others: Vec<String>,
}

fn others(ours: &str) -> Vec<String> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let current = std::env::var("PATH").unwrap_or_default();
    let mut list = others_with_php(&current, ours, sep, cfg!(windows), has_php);
    list.dedup();
    list
}

#[cfg(windows)]
pub fn terminal_status(paths: &crate::HearthPaths) -> Result<TerminalStatus, CoreError> {
    let ours = paths.bin.display().to_string();
    let (value, _) = registry::user_path()?;
    let enabled = value.split(';').any(|e| same(e, &ours, true));
    Ok(TerminalStatus { enabled, others: others(&ours) })
}

#[cfg(windows)]
pub fn set_terminal(paths: &crate::HearthPaths, on: bool) -> Result<TerminalStatus, CoreError> {
    let entries: Vec<String> = terminal_entries(paths).iter().map(|e| e.display().to_string()).collect();
    let (value, kind) = registry::user_path()?;
    let next = if on {
        with_entries(&value, &entries, ';', true)
    } else {
        entries.iter().fold(value.clone(), |v, e| without_entry(&v, e, ';', true))
    };
    if next != value {
        registry::set_user_path(&next, kind)?;
    }
    terminal_status(paths)
}

#[cfg(unix)]
fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Every profile Hearth may have edited, for status and removal (missing files are skipped).
#[cfg(unix)]
fn all_profiles() -> Vec<PathBuf> {
    [".zprofile", ".bash_profile", ".profile"].iter().map(|n| home().join(n)).collect()
}

/// Inside double quotes only these four are special to the shell.
#[cfg(unix)]
fn quoted(p: &Path) -> String {
    p.display().to_string().replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`")
}

#[cfg(unix)]
fn profile_line(paths: &crate::HearthPaths) -> String {
    let entries: Vec<String> = terminal_entries(paths).iter().map(|e| quoted(e)).collect();
    format!("export PATH=\"{}:$PATH\"", entries.join(":"))
}

/// Writes the PATH entries again when terminals already use this Hearth home, so a change in the
/// entries (the Node tools folder, added in M6) reaches them. Another home's entries are left alone.
#[cfg(unix)]
pub fn refresh_terminal(paths: &crate::HearthPaths) -> Result<(), CoreError> {
    let ours = format!("export PATH=\"{}:", quoted(&paths.bin));
    if all_profiles().iter().any(|f| std::fs::read_to_string(f).is_ok_and(|t| t.contains(START) && t.contains(&ours))) {
        set_terminal(paths, true)?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn refresh_terminal(paths: &crate::HearthPaths) -> Result<(), CoreError> {
    if terminal_status(paths)?.enabled {
        set_terminal(paths, true)?;
    }
    Ok(())
}

#[cfg(unix)]
pub fn terminal_status(paths: &crate::HearthPaths) -> Result<TerminalStatus, CoreError> {
    Ok(TerminalStatus { enabled: profile_has_block(&all_profiles()), others: others(&paths.bin.display().to_string()) })
}

#[cfg(unix)]
pub fn set_terminal(paths: &crate::HearthPaths, on: bool) -> Result<TerminalStatus, CoreError> {
    if on {
        apply_profiles(&profile_targets(&home(), |p| p.exists()), &profile_line(paths), true)?;
    } else {
        let existing: Vec<PathBuf> = all_profiles().into_iter().filter(|p| p.exists()).collect();
        apply_profiles(&existing, &profile_line(paths), false)?;
    }
    terminal_status(paths)
}

/// The user's PATH in `HKCU\Environment`, written back with its original value type.
#[cfg(windows)]
mod registry {
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
        KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    use crate::CoreError;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn failed(what: &str, code: u32) -> CoreError {
        CoreError::Php(format!("Could not {what} the user PATH (Windows error {code})"))
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key was opened by `open` and is closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open(access: u32) -> Result<Key, CoreError> {
        let mut key: HKEY = null_mut();
        // SAFETY: valid NUL-terminated name and out pointer.
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide("Environment").as_ptr(), 0, access, &mut key) };
        if status != ERROR_SUCCESS {
            return Err(failed("open", status));
        }
        Ok(Key(key))
    }

    /// The value (empty when unset) and its type (REG_EXPAND_SZ for a new value).
    pub fn user_path() -> Result<(String, REG_VALUE_TYPE), CoreError> {
        let key = open(KEY_QUERY_VALUE)?;
        let name = wide("Path");
        let (mut kind, mut size) = (0u32, 0u32);
        // SAFETY: size query with a null buffer.
        let status = unsafe { RegQueryValueExW(key.0, name.as_ptr(), null_mut(), &mut kind, null_mut(), &mut size) };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok((String::new(), REG_EXPAND_SZ));
        }
        if status != ERROR_SUCCESS {
            return Err(failed("read", status));
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        // SAFETY: the buffer holds `size` bytes plus one spare u16.
        let status = unsafe {
            RegQueryValueExW(key.0, name.as_ptr(), null_mut(), &mut kind, buf.as_mut_ptr().cast(), &mut size)
        };
        if status != ERROR_SUCCESS {
            return Err(failed("read", status));
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let kind = if kind == REG_SZ { REG_SZ } else { REG_EXPAND_SZ };
        Ok((String::from_utf16_lossy(&buf[..len]), kind))
    }

    /// Writes the value, then tells running programs (Explorer, new terminals) that it changed.
    pub fn set_user_path(value: &str, kind: REG_VALUE_TYPE) -> Result<(), CoreError> {
        let key = open(KEY_SET_VALUE)?;
        let data = wide(value);
        // SAFETY: `data` is NUL-terminated UTF-16 and its byte length is passed.
        let status = unsafe {
            RegSetValueExW(key.0, wide("Path").as_ptr(), 0, kind, data.as_ptr().cast(), (data.len() * 2) as u32)
        };
        if status != ERROR_SUCCESS {
            return Err(failed("change", status));
        }
        let env = wide("Environment");
        let mut result = 0usize;
        // SAFETY: broadcast with a 2 s timeout so a hung window cannot block Hearth.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                env.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                2000,
                &mut result,
            )
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: &str = r"C:\Users\Jörg Müller\.config\hearth\bin";

    #[test]
    fn our_entry_is_prepended_once() {
        let path = r"C:\Windows;%USERPROFILE%\.config\herd\bin";
        let once = with_entry(path, OURS, ';', true);
        assert_eq!(once, format!(r"{OURS};C:\Windows;%USERPROFILE%\.config\herd\bin"));
        assert_eq!(with_entry(&once, OURS, ';', true), once);
    }

    #[test]
    fn several_entries_go_first_in_order() {
        let entries = ["C:\\h\\bin".to_string(), "C:\\h\\data\\node\\current".to_string()];
        assert_eq!(
            with_entries("C:\\a;c:\\h\\data\\node\\current;C:\\h\\bin", &entries, ';', true),
            "C:\\h\\bin;C:\\h\\data\\node\\current;C:\\a"
        );
    }

    #[test]
    fn terminals_also_get_the_global_node_tools() {
        let paths = crate::HearthPaths::at("/h");
        assert_eq!(
            terminal_entries(&paths),
            [paths.bin.clone(), crate::node::store::bin_dir(&paths.data.join("node").join("current"))]
        );
    }

    #[test]
    fn a_later_copy_moves_to_the_front() {
        let path = format!(r"C:\Windows;{OURS}");
        assert_eq!(with_entry(&path, OURS, ';', true), format!(r"{OURS};C:\Windows"));
    }

    #[test]
    fn windows_comparison_ignores_case_and_a_trailing_backslash() {
        let path = format!(r"C:\Windows;{}\", OURS.to_uppercase());
        assert_eq!(without_entry(&path, OURS, ';', true), r"C:\Windows");
    }

    #[test]
    fn removing_keeps_every_other_entry_untouched() {
        let path = format!(r"%SystemRoot%\system32;;{OURS};C:\Tools");
        assert_eq!(without_entry(&path, OURS, ';', true), r"%SystemRoot%\system32;;C:\Tools");
        assert_eq!(without_entry(r"C:\Tools", OURS, ';', true), r"C:\Tools");
    }

    #[test]
    fn unix_comparison_is_case_sensitive() {
        assert_eq!(
            without_entry("/Users/j/.config/HEARTH/bin:/usr/bin", "/Users/j/.config/hearth/bin", ':', false),
            "/Users/j/.config/HEARTH/bin:/usr/bin"
        );
    }

    #[test]
    fn other_php_folders_are_reported() {
        let path = format!(r"C:\Herd\bin;{OURS};C:\Windows");
        let others = others_with_php(&path, OURS, ';', true, |dir| dir.contains("Herd"));
        assert_eq!(others, [r"C:\Herd\bin"]);
    }

    const BLOCK: &str = "export PATH=\"$HOME/.config/hearth/bin:$PATH\"";

    #[test]
    fn the_profile_block_is_added_replaced_and_removed_without_touching_the_rest() {
        let profile = "export EDITOR=vim\n# my stuff\n";
        let with = with_block(profile, BLOCK);
        assert_eq!(with, format!("export EDITOR=vim\n# my stuff\n\n{START}\n{BLOCK}\n{END}\n"));
        assert_eq!(with_block(&with, BLOCK), with, "adding twice changes nothing");
        assert_eq!(without_block(&with), profile);
        assert_eq!(without_block(profile), profile);
    }

    #[test]
    fn a_profile_without_a_final_newline_only_gains_that_newline() {
        let profile = "alias ll='ls -l'";
        assert_eq!(without_block(&with_block(profile, BLOCK)), "alias ll='ls -l'\n");
    }

    #[test]
    fn profiles_are_written_only_when_they_change() {
        let d = tempfile::tempdir().unwrap();
        let zprofile = d.path().join(".zprofile");
        std::fs::write(&zprofile, "export EDITOR=vim\n").unwrap();
        let bash = d.path().join(".bash_profile");
        apply_profiles(&[zprofile.clone(), bash.clone()], BLOCK, true).unwrap();
        assert!(std::fs::read_to_string(&zprofile).unwrap().contains(START));
        assert!(std::fs::read_to_string(&bash).unwrap().contains(BLOCK));
        apply_profiles(&[zprofile.clone(), bash.clone()], BLOCK, false).unwrap();
        assert_eq!(std::fs::read_to_string(&zprofile).unwrap(), "export EDITOR=vim\n");
        assert_eq!(std::fs::read_to_string(&bash).unwrap(), "");
        assert!(!profile_has_block(&[zprofile]));
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    fn existing(names: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| names.iter().any(|n| p.ends_with(n))
    }

    #[test]
    fn only_existing_profiles_are_edited_and_bash_falls_back_to_profile() {
        let home = Path::new("/Users/j");
        assert_eq!(
            profile_targets(home, existing(&[".zprofile", ".profile"])),
            [home.join(".zprofile"), home.join(".profile")]
        );
        assert_eq!(profile_targets(home, existing(&[".bash_profile", ".profile"])), [home.join(".bash_profile")]);
    }

    #[test]
    fn without_any_profile_only_zprofile_is_created() {
        let home = Path::new("/Users/j");
        assert_eq!(profile_targets(home, existing(&[])), [home.join(".zprofile")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_profile_is_written_through() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("dotfiles-zprofile");
        std::fs::write(&real, "export EDITOR=vim\n").unwrap();
        let link = d.path().join(".zprofile");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        apply_profiles(std::slice::from_ref(&link), "export PATH=x", true).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains(START));
    }
}
