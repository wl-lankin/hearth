//! The `php`, `php84`, `composer` and `laravel` commands in `<home>/bin`: tiny scripts that run the
//! right PHP with its php.ini. They use paths relative to themselves, so the Hearth home can contain
//! spaces or non-ASCII characters. `herd` and the Node commands hand over to the `hearth` CLI.

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

/// A shim that hands `command` to the `hearth` CLI next to it (which picks the project's PHP), and
/// runs `fallback` when the CLI is not there.
fn forwarding(os: Os, command: &str, fallback: String) -> String {
    match os {
        Os::Windows => {
            let direct = fallback.split_once("setlocal\r\n").map(|(_, rest)| rest).unwrap_or(&fallback);
            format!(
                "@echo off\r\nrem {MARKER}\r\nsetlocal\r\nif not exist \"%~dp0hearth.exe\" goto direct\r\n\"%~dp0hearth.exe\" {command} %*\r\nexit /b %errorlevel%\r\n:direct\r\n{direct}"
            )
        }
        Os::Macos => fallback.replacen(
            "pwd)\"\n",
            &format!("pwd)\"\nif [ -x \"$HERE/hearth\" ]; then exec \"$HERE/hearth\" {command} \"$@\"; fi\n"),
            1,
        ),
    }
}

/// A shim that runs `hearth <command> ...`: `herd ...` (no command) so scripts written for Herd
/// keep working, and the Node commands.
fn cli_shim(os: Os, command: Option<&str>) -> String {
    let command = command.map(|c| format!(" {c}")).unwrap_or_default();
    match os {
        Os::Windows => format!("@echo off\r\nrem {MARKER}\r\n\"%~dp0hearth.exe\"{command} %*\r\n"),
        Os::Macos => {
            format!("#!/bin/sh\n# {MARKER}\nexec \"$(cd \"$(dirname \"$0\")\" && pwd)/hearth\"{command} \"$@\"\n")
        }
    }
}

/// The shims as (file name, contents): `php`, `composer`, `laravel` for the default version (none
/// without one), `herd`, the Node commands while a Node is installed (until then Herd's nvm keeps
/// answering them), and `php<major><minor>` for every installed version.
pub fn render_shims(os: Os, default: Option<&str>, installed: &[String], node: bool) -> Vec<(String, String)> {
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
        shims.push((format!("php{ext}"), forwarding(os, "php", shim(minor, None))));
        shims.push((format!("composer{ext}"), forwarding(os, "composer", shim(minor, Some(composer)))));
        shims.push((format!("laravel{ext}"), forwarding(os, "laravel", shim(minor, Some(laravel)))));
    }
    shims.push((format!("herd{ext}"), cli_shim(os, None)));
    if node {
        for command in crate::node::launch::COMMANDS.iter().chain(&["nvm"]) {
            shims.push((format!("{command}{ext}"), cli_shim(os, Some(command))));
        }
    }
    for minor in installed {
        shims.push((format!("php{}{ext}", minor.replace('.', "")), shim(minor, None)));
    }
    shims
}

/// Writes the shims into `<home>/bin` and removes Hearth's own shims that are no longer needed.
pub fn write_shims(
    paths: &HearthPaths,
    default: Option<&str>,
    installed: &[String],
    node: bool,
) -> Result<(), CoreError> {
    let shims = render_shims(Os::current(), default, installed, node);
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
        let shims = render_shims(Os::Windows, Some("8.4"), &["8.4".into(), "8.3".into()], false);
        assert_eq!(names(&shims), ["php.bat", "composer.bat", "laravel.bat", "herd.bat", "php84.bat", "php83.bat"]);
        // With the CLI next to it, the project's PHP runs; without it, the default.
        assert_eq!(
            shims[0].1,
            "@echo off\r\nrem hearth-shim\r\nsetlocal\r\nif not exist \"%~dp0hearth.exe\" goto direct\r\n\"%~dp0hearth.exe\" php %*\r\nexit /b %errorlevel%\r\n:direct\r\nset \"PHPRC=%~dp0..\\config\\php\\8.4\"\r\n\"%~dp0..\\data\\php\\versions\\8.4\\php.exe\" -c \"%~dp0..\\config\\php\\8.4\\php.ini\" %*\r\n"
        );
        assert!(shims[1].1.contains("\"%~dp0hearth.exe\" composer %*"), "{}", shims[1].1);
        assert!(shims[1].1.contains("\"%~dp0composer.phar\" %*"), "{}", shims[1].1);
        assert_eq!(shims[3].1, "@echo off\r\nrem hearth-shim\r\n\"%~dp0hearth.exe\" %*\r\n");
        assert_eq!(
            shims[5].1,
            "@echo off\r\nrem hearth-shim\r\nsetlocal\r\nset \"PHPRC=%~dp0..\\config\\php\\8.3\"\r\n\"%~dp0..\\data\\php\\versions\\8.3\\php.exe\" -c \"%~dp0..\\config\\php\\8.3\\php.ini\" %*\r\n",
            "a versioned shim always runs its own version"
        );
    }

    #[test]
    fn macos_shims_are_sh_scripts() {
        let shims = render_shims(Os::Macos, Some("8.4"), &["8.4".into()], false);
        assert_eq!(names(&shims), ["php", "composer", "laravel", "herd", "php84"]);
        assert_eq!(
            shims[0].1,
            "#!/bin/sh\n# hearth-shim\nHERE=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\nif [ -x \"$HERE/hearth\" ]; then exec \"$HERE/hearth\" php \"$@\"; fi\nexport PHPRC=\"$HERE/../config/php/8.4\"\nexec \"$HERE/../data/php/versions/8.4/bin/php\" -c \"$HERE/../config/php/8.4/php.ini\" \"$@\"\n"
        );
        assert_eq!(shims[3].1, "#!/bin/sh\n# hearth-shim\nexec \"$(cd \"$(dirname \"$0\")\" && pwd)/hearth\" \"$@\"\n");
    }

    #[test]
    fn without_a_default_only_herd_and_versioned_shims_exist() {
        assert_eq!(names(&render_shims(Os::Windows, None, &[], false)), ["herd.bat"]);
    }

    #[test]
    fn node_commands_forward_to_the_cli_once_a_node_is_installed() {
        let shims = render_shims(Os::Windows, None, &[], true);
        assert_eq!(names(&shims), ["herd.bat", "node.bat", "npm.bat", "npx.bat", "corepack.bat", "nvm.bat"]);
        assert_eq!(shims[2].1, "@echo off\r\nrem hearth-shim\r\n\"%~dp0hearth.exe\" npm %*\r\n");
        let mac = render_shims(Os::Macos, None, &[], true);
        assert_eq!(
            mac[1].1,
            "#!/bin/sh\n# hearth-shim\nexec \"$(cd \"$(dirname \"$0\")\" && pwd)/hearth\" node \"$@\"\n"
        );
    }

    #[test]
    fn writing_removes_stale_shims_but_never_foreign_files() {
        let d = tempfile::tempdir().unwrap();
        let p = HearthPaths::at(d.path());
        p.ensure().unwrap();
        let ext = if cfg!(windows) { ".bat" } else { "" };
        std::fs::write(p.bin.join(format!("php82{ext}")), "@echo off\r\nrem hearth-shim\r\n").unwrap();
        std::fs::write(p.bin.join(format!("php81{ext}")), "someone else's script\n").unwrap();
        write_shims(&p, Some("8.4"), &["8.4".into()], false).unwrap();
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
