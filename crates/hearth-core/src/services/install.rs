//! Service builds from the signed catalog, installed to `<data>/services/bin/<kind>/<version>/`.
//! An installed build is used without asking the catalog; a download that does not match its hash
//! leaves nothing behind.

use std::path::{Path, PathBuf};

use crate::ipc::CoreState;
use crate::php::download::download;
use crate::php::manifest::{compare_versions, fetch, Arch, Format, Manifest, Os, ServiceBuild, ServiceFormat};
use crate::php::unpack::unpack;
use crate::CoreError;

/// How a kind is called in messages.
pub fn display_name(kind: &str) -> &str {
    match kind {
        "mysql" => "MySQL",
        "mariadb" => "MariaDB",
        "postgresql" => "PostgreSQL",
        "redis" => "Redis",
        "meilisearch" => "Meilisearch",
        "typesense" => "Typesense",
        "silo" => "Silo (S3)",
        other => other,
    }
}

/// The builds this computer can run.
pub fn available(manifest: &Manifest) -> Vec<&ServiceBuild> {
    manifest.services.iter().filter(|b| b.os == Os::current() && b.arch == Arch::current()).collect()
}

fn kind_dir(core: &CoreState, kind: &str) -> PathBuf {
    core.paths.data.join("services").join("bin").join(kind)
}

/// `version` belongs to `line` ("8.4.11" to "8.4", "18.6.1" to "18"; Silo's single line is "current").
fn in_line(version: &str, line: &str) -> bool {
    line == "current" || version == line || version.starts_with(&format!("{line}."))
}

/// The newest installed build of `kind` in `line`.
pub fn installed(core: &CoreState, kind: &str, line: &str) -> Option<PathBuf> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(kind_dir(core, kind))
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        // `<version>.new` / `.old` are an install or update in progress.
        .filter(|(name, _)| !name.ends_with(".new") && !name.ends_with(".old") && in_line(name, line))
        .collect();
    found.sort_by(|a, b| compare_versions(&a.0, &b.0));
    found.pop().map(|(_, dir)| dir)
}

/// The catalog's build of `kind` at `line` for this computer, or why there is none.
pub fn pick<'m>(manifest: &'m Manifest, kind: &str, line: &str) -> Result<&'m ServiceBuild, CoreError> {
    if let Some(build) = available(manifest).into_iter().find(|b| b.kind == kind && b.line == line) {
        return Ok(build);
    }
    if kind == "typesense" && Os::current() == Os::Windows {
        return Err(CoreError::Php("Typesense is not available on Windows (it needs WSL)".into()));
    }
    Err(CoreError::Php(format!("The catalog has no {} {line} for this computer", display_name(kind))))
}

/// The installed folder of `kind` at `line`, installing it from the catalog first if needed.
pub async fn ensure_installed(core: &CoreState, kind: &str, line: &str) -> Result<PathBuf, CoreError> {
    if let Some(dir) = installed(core, kind, line) {
        return Ok(dir);
    }
    let manifest = fetch(&core.paths, &core.catalog.url, &core.catalog.public_key).await?;
    let build = pick(&manifest, kind, line)?.clone();
    let _tools = core.tools_lock.lock().await;
    let dir = kind_dir(core, kind).join(&build.version);
    if dir.is_dir() {
        return Ok(dir);
    }
    let downloads = core.paths.data.join("services").join("downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| CoreError::io(&downloads, e))?;
    let archive = downloads.join(format!("{kind}-{}", build.version));
    let label = format!("{} {}", display_name(kind), build.version);
    let result = async {
        download(&build.url, &archive, &build.sha256, &label, |_, _| {}).await?;
        let (archive, dir, build, kind) = (archive.clone(), dir.clone(), build.clone(), kind.to_string());
        tokio::task::spawn_blocking(move || place(&archive, &build, &kind, &dir))
            .await
            .map_err(|e| CoreError::Php(e.to_string()))?
    }
    .await;
    let _ = std::fs::remove_file(&archive);
    result?;
    Ok(dir)
}

/// Unpacks (or, for a single program, copies) the verified download into `dir`, all or nothing.
fn place(archive: &Path, build: &ServiceBuild, kind: &str, dir: &Path) -> Result<(), CoreError> {
    let parent = dir.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
    match build.format {
        ServiceFormat::Zip => unpack(archive, Format::Zip, &build.root, dir)?,
        ServiceFormat::TarGz => unpack(archive, Format::TarGz, &build.root, dir)?,
        ServiceFormat::Binary => {
            let staging = dir.with_extension("new");
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).map_err(|e| CoreError::io(&staging, e))?;
            let program = staging.join(format!("{kind}{}", std::env::consts::EXE_SUFFIX));
            std::fs::copy(archive, &program).map_err(|e| CoreError::io(&program, e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| CoreError::io(&program, e))?;
            }
            crate::atomic::rename_dir(&staging, dir).map_err(|e| CoreError::io(dir, e))?;
        }
    }
    // Downloads made by Hearth are normally not quarantined; if one is, Gatekeeper would block
    // every program in it.
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(dir).output();
    Ok(())
}
