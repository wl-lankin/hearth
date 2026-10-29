//! Installing and removing Node versions. A download that does not match its hash leaves nothing
//! behind; the first version installed becomes the default, and removing the default moves it to
//! the newest version left.

use std::path::PathBuf;

use crate::ipc::CoreState;
use crate::php::download::download;
use crate::php::manifest::{compare_versions, Arch, Manifest, NodeBuild, Os};
use crate::php::service::BUILD_FILE;
use crate::php::unpack::unpack_with;
use crate::CoreError;

use super::store::{default_version, installed, node_dir, set_default, version_dir};

/// The catalog's builds for this computer, newest first.
pub fn available(manifest: &Manifest) -> Vec<&NodeBuild> {
    let mut list: Vec<&NodeBuild> =
        manifest.node.iter().filter(|b| b.os == Os::current() && b.arch == Arch::current()).collect();
    list.sort_by(|a, b| compare_versions(&b.version, &a.version));
    list
}

/// Downloads, checks and unpacks `build`; returns its folder.
pub async fn install(core: &CoreState, build: &NodeBuild) -> Result<PathBuf, CoreError> {
    let paths = &core.paths;
    let _tools = core.tools_lock.lock().await;
    let dir = version_dir(paths, &build.version);
    if !installed(paths).iter().any(|i| i.version == build.version) {
        let downloads = node_dir(paths).join("downloads");
        std::fs::create_dir_all(&downloads).map_err(|e| CoreError::io(&downloads, e))?;
        let archive = downloads.join(format!("node-{}", build.version));
        let label = format!("Node {}", build.version);
        let result = async {
            download(&build.url, &archive, &build.sha256, &label, |_, _| {}).await?;
            let json = serde_json::to_vec_pretty(build).map_err(|e| CoreError::Php(e.to_string()))?;
            let (archive, dir, format, root) = (archive.clone(), dir.clone(), build.format, build.root.clone());
            tokio::task::spawn_blocking(move || {
                unpack_with(&archive, format, &root, &dir, |new| {
                    // Last: only a folder with this file counts as installed.
                    std::fs::write(new.join(BUILD_FILE), &json).map_err(|e| CoreError::io(new, e))
                })
            })
            .await
            .map_err(|e| CoreError::Php(e.to_string()))?
        }
        .await;
        let _ = std::fs::remove_file(&archive);
        result?;
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&dir).output();
    }
    if default_version(paths).is_none() {
        set_default(paths, Some(&build.version))?;
    }
    refresh_commands(core)?;
    Ok(dir)
}

/// Removes `version`. When it was the default, the newest remaining version takes over (or none).
pub async fn uninstall(core: &CoreState, version: &str) -> Result<(), CoreError> {
    let paths = &core.paths;
    let _tools = core.tools_lock.lock().await;
    if !installed(paths).iter().any(|i| i.version == version) {
        return Err(CoreError::Php(format!("Node {version} is not installed")));
    }
    let was_default = default_version(paths).as_deref() == Some(version);
    let dir = version_dir(paths, version);
    // Moved aside first: Windows refuses while a program of it runs, and then nothing is half gone.
    let old = node_dir(paths).join(format!("v{version}.old"));
    let _ = std::fs::remove_dir_all(&old);
    crate::atomic::rename_dir(&dir, &old)
        .map_err(|_| CoreError::Php(format!("Close the programs using Node {version} and try again")))?;
    let _ = std::fs::remove_dir_all(&old);
    if was_default {
        let next = installed(paths).into_iter().next().map(|i| i.version);
        set_default(paths, next.as_deref())?;
    }
    refresh_commands(core)
}

/// The Node commands in `<home>/bin` exist while a version is installed (Herd's answer until then),
/// and terminals that use Hearth also get the global tools folder (added in M6).
fn refresh_commands(core: &CoreState) -> Result<(), CoreError> {
    let paths = &core.paths;
    let _php = core.php_lock.lock().unwrap_or_else(|e| e.into_inner());
    crate::php::service::refresh_shims(paths)?;
    crate::cli_install::sync_node_names(paths, !installed(paths).is_empty())?;
    crate::path_env::refresh_terminal(paths)
}
