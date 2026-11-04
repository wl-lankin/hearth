//! Turning a saved process into what the supervisor runs, and starting, stopping and pruning them.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::ipc::CoreState;
use crate::processes::store::{self, Proc};
use crate::supervisor::ProcessSpec;
use crate::{CoreError, HearthPaths};

/// The supervisor's key for a site's process.
pub fn key(site: &str, name: &str) -> String {
    format!("proc:{site}:{name}")
}

pub fn log_path(paths: &HearthPaths, site: &str, name: &str) -> PathBuf {
    paths.logs.join("processes").join(site).join(format!("{name}.log"))
}

/// The platform shell running `proc.command` as typed in `dir`, with Hearth's commands first on
/// PATH: `php`, `composer`, `node` and `npm` pick the site's own versions there.
pub fn spec_for(paths: &HearthPaths, site: &str, dir: &Path, proc: &Proc) -> ProcessSpec {
    let windows = cfg!(windows);
    let (program, args, raw_args): (PathBuf, Vec<OsString>, bool) = if windows {
        // `/s /c "..."`: cmd drops the outer quotes and runs the rest exactly as written.
        let comspec = std::env::var_os("ComSpec").map(PathBuf::from).unwrap_or_else(|| "cmd.exe".into());
        (comspec, vec!["/d".into(), "/s".into(), "/c".into(), format!("\"{}\"", proc.command).into()], true)
    } else {
        ("/bin/sh".into(), vec!["-c".into(), proc.command.clone().into()], false)
    };
    let entries: Vec<String> =
        crate::path_env::terminal_entries(paths).iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let inherited = std::env::var("PATH").unwrap_or_default();
    let sep = if windows { ';' } else { ':' };
    let path = crate::path_env::with_entries(&inherited, &entries, sep, windows);
    ProcessSpec {
        key: key(site, &proc.name),
        program,
        args,
        env: vec![
            ("PATH".into(), path.into()),
            // Most tools leave out color codes then; the rest is stripped when the log is read.
            ("NO_COLOR".into(), "1".into()),
            ("FORCE_COLOR".into(), "0".into()),
        ],
        cwd: Some(dir.to_path_buf()),
        log: log_path(paths, site, &proc.name),
        ready_port: None,
        ready_socket: None,
        guard_port: None,
        raw_args,
    }
}

/// The folder of site `site`, if it exists.
pub fn site_dir(paths: &HearthPaths, site: &str) -> Result<PathBuf, CoreError> {
    let found = crate::sites::registry::list(paths).into_iter().find(|s| s.name == site);
    let found = found.ok_or_else(|| CoreError::Php(format!("No site named {site}.test")))?;
    found.path.ok_or_else(|| CoreError::Php(format!("{site}.test is a proxy: it has no folder to run processes in")))
}

/// Starts a process with an empty log (blocking: a stop before it waits for the old one).
pub fn start(core: &CoreState, site: &str, name: &str) -> Result<(), CoreError> {
    let proc = store::get(&core.paths, site, name)?;
    let dir = site_dir(&core.paths, site)?;
    let spec = spec_for(&core.paths, site, &dir, &proc);
    core.supervisor.stop(&spec.key);
    if let Some(parent) = spec.log.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
    }
    std::fs::write(&spec.log, b"").map_err(|e| CoreError::io(&spec.log, e))?;
    core.supervisor.ensure(spec)
}

pub fn stop(core: &CoreState, site: &str, name: &str) -> Result<(), CoreError> {
    store::get(&core.paths, site, name)?;
    core.supervisor.stop(&key(site, name));
    Ok(())
}

/// Starts every process marked autostart; one that fails is skipped (its row shows why it is off).
pub fn start_autostart(core: &CoreState) {
    for (site, list) in store::load(&core.paths) {
        for proc in list.iter().filter(|p| p.autostart) {
            let _ = start(core, &site, &proc.name);
        }
    }
}

/// Stops and forgets the processes of sites that no longer exist (unlinked, unparked).
pub fn prune(core: &CoreState) -> Result<(), CoreError> {
    let sites: Vec<String> = crate::sites::registry::list(&core.paths).into_iter().map(|s| s.name).collect();
    for (site, list) in store::load(&core.paths) {
        if !sites.contains(&site) {
            for proc in &list {
                core.supervisor.stop(&key(&site, &proc.name));
            }
            store::forget_site(&core.paths, &site)?;
        }
    }
    Ok(())
}
