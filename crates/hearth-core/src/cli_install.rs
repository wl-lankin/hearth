//! Puts the `hearth` CLI into `<home>/bin`, next to the shims, from where the app ships it. On
//! Windows it is also copied as `php.exe`, `composer.exe`, `laravel.exe` and `herd.exe`: the CLI
//! answers to the name it was started under, and an `.exe` (unlike a `.bat`) reaches PHP with every
//! argument exactly as typed. While a Node version is installed it is also `node.exe`, `npm.exe`,
//! `npx.exe`, `corepack.exe` and `nvm.exe`.

use std::path::{Path, PathBuf};

use crate::{CoreError, HearthPaths};

/// The CLI as the app bundle ships it (`hearth-cli`, since the app itself is `Hearth`), or, in a
/// development build, `hearth` from the same target folder.
fn source(app_dir: &Path) -> Option<PathBuf> {
    let ext = std::env::consts::EXE_SUFFIX;
    let shipped = app_dir.join(format!("hearth-cli{ext}"));
    if shipped.is_file() {
        return Some(shipped);
    }
    let dev = app_dir.join(format!("hearth{ext}"));
    (cfg!(debug_assertions) && dev.is_file()).then_some(dev)
}

/// The names the CLI is installed under.
fn names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["hearth.exe", "php.exe", "composer.exe", "laravel.exe", "herd.exe"]
    } else {
        &["hearth"]
    }
}

/// Copies the CLI from `app_dir` into `<home>/bin` under every name; `Ok(false)` when nothing
/// changed (no CLI shipped, or all copies already current).
pub fn install_cli(paths: &HearthPaths, app_dir: &Path) -> Result<bool, CoreError> {
    let Some(source) = source(app_dir) else { return Ok(false) };
    let bytes = std::fs::read(&source).map_err(|e| CoreError::io(&source, e))?;
    let mut changed = false;
    let mut failed = Vec::new();
    for name in names() {
        let target = paths.bin.join(name);
        let aside = paths.bin.join(format!("{name}.old"));
        // A copy moved aside by an earlier update goes once nothing runs it any more.
        let _ = std::fs::remove_file(&aside);
        if std::fs::read(&target).is_ok_and(|current| current == bytes) {
            continue;
        }
        // One busy name must not keep the others on an old version.
        match place(&bytes, &target, &aside) {
            Ok(()) => changed = true,
            Err(e) => failed.push(e.to_string()),
        }
    }
    if !failed.is_empty() {
        return Err(CoreError::Php(failed.join("; ")));
    }
    sync_node_names(paths, !crate::node::store::installed(paths).is_empty())?;
    Ok(changed)
}

/// The Node commands the CLI answers on Windows (macOS uses the shims).
const NODE_NAMES: [&str; 5] = ["node.exe", "npm.exe", "npx.exe", "corepack.exe", "nvm.exe"];

/// Copies `<home>/bin/hearth.exe` as the Node commands while a Node version is installed (`on`), and
/// removes them otherwise, so Herd's nvm keeps answering until then. Nothing to do on macOS.
pub fn sync_node_names(paths: &HearthPaths, on: bool) -> Result<(), CoreError> {
    if !cfg!(windows) {
        return Ok(());
    }
    let cli = std::fs::read(paths.bin.join("hearth.exe")).ok();
    for name in NODE_NAMES {
        let (target, aside) = (paths.bin.join(name), paths.bin.join(format!("{name}.old")));
        let _ = std::fs::remove_file(&aside);
        match &cli {
            Some(bytes) if on => {
                if !std::fs::read(&target).is_ok_and(|current| current == *bytes) {
                    place(bytes, &target, &aside)?;
                }
            }
            // A copy still running (a long `npm run dev`) goes on the next sync.
            _ => {
                let _ = std::fs::remove_file(&target);
            }
        }
    }
    Ok(())
}

/// Writes `bytes` as `target`: next to it first, then renamed over it. A running program cannot be
/// overwritten on Windows but can be renamed, so a busy target is moved to `aside` first.
fn place(bytes: &[u8], target: &Path, aside: &Path) -> Result<(), CoreError> {
    let fresh = target.with_extension("new");
    std::fs::write(&fresh, bytes).map_err(|e| CoreError::io(&fresh, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| CoreError::io(&fresh, e))?;
    }
    if std::fs::rename(&fresh, target).is_ok() {
        return Ok(());
    }
    let moved = std::fs::rename(target, aside).and_then(|()| std::fs::rename(&fresh, target));
    if let Err(e) = moved {
        let _ = std::fs::remove_file(&fresh);
        return Err(CoreError::io(target, e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(cli: &[u8]) -> (tempfile::TempDir, HearthPaths, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(d.path().join("home"));
        paths.ensure().unwrap();
        let app = d.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join(format!("hearth-cli{}", std::env::consts::EXE_SUFFIX)), cli).unwrap();
        (d, paths, app)
    }

    #[test]
    fn the_cli_lands_in_bin_under_every_name_once() {
        let (_d, paths, app) = setup(b"cli v1");
        assert!(install_cli(&paths, &app).unwrap());
        for name in names() {
            assert_eq!(std::fs::read(paths.bin.join(name)).unwrap(), b"cli v1", "{name}");
        }
        assert!(!install_cli(&paths, &app).unwrap(), "identical copies are left alone");
    }

    #[test]
    fn without_a_shipped_cli_nothing_happens() {
        let d = tempfile::tempdir().unwrap();
        let paths = HearthPaths::at(d.path().join("home"));
        paths.ensure().unwrap();
        assert!(!install_cli(&paths, d.path()).unwrap());
        assert_eq!(std::fs::read_dir(&paths.bin).unwrap().count(), 0);
    }

    #[cfg(windows)]
    #[test]
    fn node_names_exist_only_while_a_node_is_installed() {
        let (_d, paths, app) = setup(b"cli v1");
        install_cli(&paths, &app).unwrap();
        let node_names = ["node.exe", "npm.exe", "npx.exe", "corepack.exe", "nvm.exe"];
        assert!(node_names.iter().all(|n| !paths.bin.join(n).exists()), "Herd's node keeps answering");
        sync_node_names(&paths, true).unwrap();
        for name in node_names {
            assert_eq!(std::fs::read(paths.bin.join(name)).unwrap(), b"cli v1", "{name}");
        }
        // An app update refreshes them while a Node is installed.
        let dir = crate::node::store::version_dir(&paths, "22.21.1");
        std::fs::create_dir_all(&dir).unwrap();
        let build = r#"{"major":22,"version":"22.21.1","lts":"Jod","os":"windows","arch":"x64","url":"u","sha256":"s","format":"zip","root":""}"#;
        std::fs::write(dir.join(crate::php::service::BUILD_FILE), build).unwrap();
        std::fs::write(app.join("hearth-cli.exe"), b"cli v2").unwrap();
        install_cli(&paths, &app).unwrap();
        assert_eq!(std::fs::read(paths.bin.join("npm.exe")).unwrap(), b"cli v2");
        sync_node_names(&paths, false).unwrap();
        assert!(node_names.iter().all(|n| !paths.bin.join(n).exists()));
    }

    #[cfg(windows)]
    #[test]
    fn one_name_that_cannot_be_replaced_does_not_hold_back_the_others() {
        let (_d, paths, app) = setup(b"cli v1");
        install_cli(&paths, &app).unwrap();
        std::fs::write(app.join("hearth-cli.exe"), b"cli v2").unwrap();
        // php.exe cannot be replaced and its earlier copy cannot be moved out of the way.
        std::fs::remove_file(paths.bin.join("php.exe")).unwrap();
        std::fs::create_dir_all(paths.bin.join("php.exe").join("busy")).unwrap();
        std::fs::create_dir_all(paths.bin.join("php.exe.old").join("busy")).unwrap();
        let err = install_cli(&paths, &app).unwrap_err().to_string();
        assert!(err.contains("php.exe"), "{err}");
        for name in ["hearth.exe", "composer.exe", "laravel.exe", "herd.exe"] {
            assert_eq!(std::fs::read(paths.bin.join(name)).unwrap(), b"cli v2", "{name}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_running_copy_is_moved_aside_and_cleaned_up_later() {
        let (_d, paths, app) = setup(b"cli v1");
        install_cli(&paths, &app).unwrap();
        // A program really running from bin\php.exe (the supervisor's test helper), as during a
        // long `php artisan serve` while the app updates.
        let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        std::fs::copy(deps.parent().unwrap().join("examples").join("listen.exe"), paths.bin.join("php.exe")).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut running = std::process::Command::new(paths.bin.join("php.exe")).arg(port.to_string()).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::fs::write(app.join("hearth-cli.exe"), b"cli v2").unwrap();
        assert!(install_cli(&paths, &app).unwrap());
        assert_eq!(std::fs::read(paths.bin.join("php.exe")).unwrap(), b"cli v2");
        assert!(paths.bin.join("php.exe.old").exists());
        running.kill().unwrap();
        running.wait().unwrap();
        std::fs::write(app.join("hearth-cli.exe"), b"cli v3").unwrap();
        install_cli(&paths, &app).unwrap();
        assert!(!paths.bin.join("php.exe.old").exists(), "the old copy is gone once it is free");
    }
}
