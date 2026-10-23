//! "Use Hearth's PHP in terminals": puts `<home>/bin` first on the user's PATH (Windows: the user
//! environment in the registry; macOS: a marked block in the shell profiles) and takes it out again.

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

/// Adds (`on`) or removes Hearth's block in each profile file; files are written only when they change.
pub fn apply_profiles(files: &[PathBuf], line: &str, on: bool) -> Result<(), CoreError> {
    for file in files {
        let current = std::fs::read_to_string(file).unwrap_or_default();
        let next = if on { with_block(&current, line) } else { without_block(&current) };
        if next != current {
            atomic::write(file, next.as_bytes())?;
        }
    }
    Ok(())
}

/// True when `dir` holds a `php` command.
pub fn has_php(dir: &str) -> bool {
    let dir = Path::new(dir);
    ["php", "php.bat", "php.exe", "php.cmd"].iter().any(|name| dir.join(name).is_file())
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
