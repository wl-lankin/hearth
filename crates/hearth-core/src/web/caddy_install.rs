//! Caddy from the signed catalog, installed to `<data>/caddy/<version>/`.

use std::path::{Path, PathBuf};

use crate::ipc::CoreState;
use crate::php::download::download;
use crate::php::manifest::{fetch, Arch, Format, Manifest, Os, Tool};
use crate::php::unpack::unpack;
use crate::CoreError;

/// The Caddy build for this computer.
pub fn pick_caddy(manifest: &Manifest) -> Result<&Tool, CoreError> {
    manifest
        .tools
        .caddy
        .iter()
        .find(|t| t.os == Os::current() && t.arch == Arch::current())
        .ok_or_else(|| CoreError::Php("This catalog has no web server for this computer".into()))
}

fn binary_in(dir: &Path, tool: &Tool) -> PathBuf {
    dir.join(&tool.binary)
}

/// Installs Caddy unless this version is already there; returns the program's path.
pub async fn ensure_caddy(core: &CoreState) -> Result<PathBuf, CoreError> {
    let manifest = fetch(&core.paths, &core.catalog.url, &core.catalog.public_key).await?;
    let tool = pick_caddy(&manifest)?.clone();
    let dir = core.paths.data.join("caddy").join(&tool.version);
    let program = binary_in(&dir, &tool);
    if program.is_file() {
        return Ok(program);
    }
    let _tools = core.tools_lock.lock().await;
    if program.is_file() {
        return Ok(program);
    }
    let downloads = core.paths.data.join("caddy").join("downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| CoreError::io(&downloads, e))?;
    let ext = match tool.format {
        Format::Zip => "zip",
        Format::TarGz => "tar.gz",
    };
    let archive = downloads.join(format!("caddy-{}.{ext}", tool.version));
    let label = format!("Caddy {}", tool.version);
    let result = async {
        download(&tool.url, &archive, &tool.sha256, &label, |_, _| {}).await?;
        let (archive, dir, format) = (archive.clone(), dir.clone(), tool.format);
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(dir.parent().unwrap()).map_err(|e| CoreError::io(&dir, e))?;
            unpack(&archive, format, "", &dir)
        })
        .await
        .map_err(|e| CoreError::Php(e.to_string()))?
    }
    .await;
    let _ = std::fs::remove_file(&archive);
    result?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| CoreError::io(&program, e))?;
    }
    if !program.is_file() {
        return Err(CoreError::Php(format!("{label} has no {} in its archive", tool.binary)));
    }
    Ok(program)
}
