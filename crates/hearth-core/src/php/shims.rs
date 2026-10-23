//! The `php`, `php84`, `composer` and `laravel` commands in `<home>/bin`: tiny scripts that run the
//! right PHP with its php.ini. They use paths relative to themselves, so the Hearth home can contain
//! spaces or non-ASCII characters.

use crate::php::manifest::Os;
use crate::{atomic, CoreError, HearthPaths};

/// Identifies a script as Hearth's, so only those are ever deleted.
const MARKER: &str = "hearth-shim";

// Every shim also sets PHPRC to the version's config folder: tools that start PHP again themselves
// (`artisan serve`, `queue:listen`, parallel test runners) pass no `-c`, and without PHPRC that child
// PHP would load no php.ini at all (on Windows: no openssl, mbstring or database drivers).

/// Renders one shim: PHP version, optional script to run.
type ShimFn = fn(&str, Option<&str>) -> String;

fn windows_shim(minor: &str, script: Option<&str>) -> String {
    let script = script.map(|s| format!(" \"{s}\"")).unwrap_or_default();
    format!(
        "@echo off\r\nrem {MARKER}\r\nsetlocal\r\nset \"PHPRC=%~dp0..\\config\\php\\{minor}\"\r\n\"%~dp0..\\data\\php\\versions\\{minor}\\php.exe\" -c \"%~dp0..\\config\\php\\{minor}\\php.ini\"{script} %*\r\n"
    )
}

fn sh_shim(minor: &str, script: Option<&str>) -> String {
    let script = script.map(|s| format!(" \"{s}\"")).unwrap_or_default();
    format!(
        "#!/bin/sh\n# {MARKER}\nHERE=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\nexport PHPRC=\"$HERE/../config/php/{minor}\"\nexec \"$HERE/../data/php/versions/{minor}/bin/php\" -c \"$HERE/../config/php/{minor}/php.ini\"{script} \"$@\"\n"
    )
}

/// The shims as (file name, contents): `php`, `composer`, `laravel` for the default version (none
/// without one), and `php<major><minor>` for every installed version.
pub fn render_shims(os: Os, default: Option<&str>, installed: &[String]) -> Vec<(String, String)> {
    let (ext, shim): (&str, ShimFn) = match os {
        Os::Windows => (".bat", windows_shim),
        Os::Macos => ("", sh_shim),
    };
    let (composer, laravel) = match os {
        Os::Windows => ("%~dp0composer.phar", "%~dp0..\\data\\composer\\vendor\\laravel\\installer\\bin\\laravel"),
        Os::Macos => ("$HERE/composer.phar", "$HERE/../data/composer/vendor/laravel/installer/bin/laravel"),
    };
    let mut shims = Vec::new();
    if let Some(minor) = default {
        shims.push((format!("php{ext}"), shim(minor, None)));
        shims.push((format!("composer{ext}"), shim(minor, Some(composer))));
        shims.push((format!("laravel{ext}"), shim(minor, Some(laravel))));
    }
    for minor in installed {
        shims.push((format!("php{}{ext}", minor.replace('.', "")), shim(minor, None)));
    }
    shims
}

/// Writes the shims into `<home>/bin` and removes Hearth's own shims that are no longer needed.
pub fn write_shims(paths: &HearthPaths, default: Option<&str>, installed: &[String]) -> Result<(), CoreError> {
    let shims = render_shims(Os::current(), default, installed);
    for (name, contents) in &shims {
        let path = paths.bin.join(name);
        atomic::write(&path, contents.as_bytes())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| CoreError::io(&path, e))?;
        }
    }
    let entries = std::fs::read_dir(&paths.bin).map_err(|e| CoreError::io(&paths.bin, e))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if shims.iter().any(|(n, _)| *n == name) || !entry.path().is_file() {
            continue;
        }
        let ours =
            std::fs::read_to_string(entry.path()).is_ok_and(|text| text.lines().take(3).any(|l| l.contains(MARKER)));
        if ours {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(shims: &[(String, String)]) -> Vec<&str> {
        shims.iter().map(|(n, _)| n.as_str()).collect()
    }

    #[test]
    fn windows_shims_for_two_versions() {
        let shims = render_shims(Os::Windows, Some("8.4"), &["8.4".into(), "8.3".into()]);
        assert_eq!(names(&shims), ["php.bat", "composer.bat", "laravel.bat", "php84.bat", "php83.bat"]);
        assert_eq!(
            shims[0].1,
            "@echo off\r\nrem hearth-shim\r\nsetlocal\r\nset \"PHPRC=%~dp0..\\config\\php\\8.4\"\r\n\"%~dp0..\\data\\php\\versions\\8.4\\php.exe\" -c \"%~dp0..\\config\\php\\8.4\\php.ini\" %*\r\n"
        );
        assert!(shims[1].1.contains("\"%~dp0composer.phar\" %*"), "{}", shims[1].1);
        assert!(shims[4].1.contains("versions\\8.3\\php.exe"), "{}", shims[4].1);
    }

    #[test]
    fn macos_shims_are_sh_scripts() {
        let shims = render_shims(Os::Macos, Some("8.4"), &["8.4".into()]);
        assert_eq!(names(&shims), ["php", "composer", "laravel", "php84"]);
        assert_eq!(
            shims[0].1,
            "#!/bin/sh\n# hearth-shim\nHERE=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\nexport PHPRC=\"$HERE/../config/php/8.4\"\nexec \"$HERE/../data/php/versions/8.4/bin/php\" -c \"$HERE/../config/php/8.4/php.ini\" \"$@\"\n"
        );
    }

    #[test]
    fn without_a_default_only_versioned_shims_exist() {
        assert_eq!(names(&render_shims(Os::Windows, None, &[])), Vec::<&str>::new());
    }

    #[test]
    fn writing_removes_stale_shims_but_never_foreign_files() {
        let d = tempfile::tempdir().unwrap();
        let p = HearthPaths::at(d.path());
        p.ensure().unwrap();
        let ext = if cfg!(windows) { ".bat" } else { "" };
        std::fs::write(p.bin.join(format!("php82{ext}")), "@echo off\r\nrem hearth-shim\r\n").unwrap();
        std::fs::write(p.bin.join(format!("php81{ext}")), "someone else's script\n").unwrap();
        write_shims(&p, Some("8.4"), &["8.4".into()]).unwrap();
        assert!(p.bin.join(format!("php84{ext}")).is_file());
        assert!(!p.bin.join(format!("php82{ext}")).exists());
        assert!(p.bin.join(format!("php81{ext}")).exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(p.bin.join("php")).unwrap().permissions().mode() & 0o777, 0o755);
        }
    }
}
