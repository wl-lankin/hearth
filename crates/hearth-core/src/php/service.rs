//! Installing, updating and removing PHP versions; keeping php.ini and shims in step.

use std::path::{Path, PathBuf};

use crate::ipc::CoreState;
use crate::jobs::JobId;
use crate::php::download::download;
use crate::php::ini::{ini_dir, render_ini, write_ini, IniContext};
use crate::php::manifest::{builds_for, fetch, Arch, Artifact, ExtensionSource, Os, PhpBuild};
use crate::php::shims::write_shims;
use crate::php::store::{self, install_dir, VERSION_FILE};
use crate::php::unpack::unpack_with;
use crate::{CoreError, HearthPaths};

/// The catalog entry an install came from, kept in its folder (php.ini needs its extension sources).
pub const BUILD_FILE: &str = ".hearth-build.json";

fn php_data(paths: &HearthPaths) -> PathBuf {
    paths.data.join("php")
}

fn cacert_path(paths: &HearthPaths) -> PathBuf {
    php_data(paths).join("cacert.pem")
}

/// Starts installing (or updating) PHP `minor` as a background job; a running install of the same
/// version is joined instead of started twice. Needs a tokio runtime.
pub fn install(core: &CoreState, minor: &str) -> JobId {
    let (id, new) = core.jobs.start("php.install", &format!("php.install:{minor}"), &format!("Installing PHP {minor}"));
    if new {
        let core = core.clone();
        let minor = minor.to_string();
        tokio::spawn(async move {
            let result = run_install(&core, &minor, id).await;
            core.jobs.finish(id, result.map_err(|e| e.to_string()));
        });
    }
    id
}

/// Downloaded files of one install, deleted however the install ends.
struct TempFiles(Vec<PathBuf>);

impl Drop for TempFiles {
    fn drop(&mut self) {
        for file in &self.0 {
            let _ = std::fs::remove_file(file);
        }
    }
}

async fn run_install(core: &CoreState, minor: &str, id: JobId) -> Result<(), CoreError> {
    let paths = &core.paths;
    core.jobs.progress(id, 0.0, "Reading the PHP catalog");
    let manifest = fetch(paths, &core.catalog.url, &core.catalog.public_key).await?;
    let build = builds_for(&manifest, Os::current(), Arch::current())
        .into_iter()
        .find(|b| b.minor == minor)
        .cloned()
        .ok_or_else(|| CoreError::Php(format!("PHP {minor} is not available for this computer")))?;

    let downloads = php_data(paths).join("downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| CoreError::io(&downloads, e))?;
    let mut temp = TempFiles(Vec::new());
    let archive = downloads.join(format!("php-{}.{}", build.version, extension_of(&build)));
    temp.0.push(archive.clone());
    let label = format!("PHP {}", build.version);
    let jobs = core.jobs.clone();
    let message = format!("Downloading PHP {}", build.version);
    download(&build.url, &archive, &build.sha256, &label, |got, total| {
        if let Some(total) = total {
            jobs.progress(id, 0.05 + 0.7 * got as f64 / total as f64, &message);
        }
    })
    .await?;

    // Windows extensions come as separate zips; they go into the new folder before it is swapped in.
    let mut extension_dlls = Vec::new();
    for (name, source) in &build.extensions {
        if let ExtensionSource::Dll { url, sha256, file, .. } | ExtensionSource::ZendDll { url, sha256, file, .. } =
            source
        {
            core.jobs.progress(id, 0.78, &format!("Downloading the {name} extension"));
            let zip = downloads.join(format!("{name}-{}.zip", build.version));
            temp.0.push(zip.clone());
            download(url, &zip, sha256, &format!("the {name} extension"), |_, _| {}).await?;
            extension_dlls.push((name.clone(), zip, file.clone()));
        }
    }

    // Everything that needs the network happens before the swap: a failure here leaves the old
    // install (or none) exactly as it was.
    core.jobs.progress(id, 0.82, "Setting up Composer");
    {
        let _tools = core.tools_lock.lock().await;
        ensure_artifact(&manifest.cacert, &cacert_path(paths), "the CA bundle").await?;
        ensure_artifact(&manifest.composer, &paths.bin.join("composer.phar"), "Composer").await?;
    }

    core.jobs.progress(id, 0.88, "Unpacking");
    let target = install_dir(paths, minor);
    let build_json = serde_json::to_vec_pretty(&build).map_err(|e| CoreError::Php(e.to_string()))?;
    let version = build.version.clone();
    let (format, root) = (build.format, build.root.clone());
    let names: Vec<String> = build.extensions.keys().cloned().collect();
    let (php_lock, paths, minor) = (core.php_lock.clone(), paths.clone(), minor.to_string());
    tokio::task::spawn_blocking(move || {
        // One writer at a time for installs, php.json, php.ini and the shims.
        let _guard = php_lock.lock().unwrap_or_else(|e| e.into_inner());
        std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| CoreError::io(&target, e))?;
        unpack_with(&archive, format, &root, &target, |new| {
            for (name, zip, file) in &extension_dlls {
                extract_file(zip, file, &new.join("ext").join(file), name)?;
            }
            std::fs::write(new.join(BUILD_FILE), &build_json).map_err(|e| CoreError::io(new, e))?;
            // Last: only a folder with this file counts as installed.
            std::fs::write(new.join(VERSION_FILE), &version).map_err(|e| CoreError::io(new, e))
        })?;
        // Only local steps after the swap.
        store::register(&paths, &minor, &names)?;
        apply_config(&paths, &minor)?;
        refresh_shims(&paths)
    })
    .await
    .map_err(|e| CoreError::Php(e.to_string()))?
}

fn extension_of(build: &PhpBuild) -> &'static str {
    match build.format {
        crate::php::manifest::Format::Zip => "zip",
        crate::php::manifest::Format::TarGz => "tar.gz",
    }
}

/// Copies `file` (matched by name, any folder, any case) out of an extension zip.
fn extract_file(zip_path: &Path, file: &str, dest: &Path, name: &str) -> Result<(), CoreError> {
    let damaged = || CoreError::Php(format!("The download of the {name} extension is damaged; try again"));
    let f = std::fs::File::open(zip_path).map_err(|e| CoreError::io(zip_path, e))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|_| damaged())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|_| damaged())?;
        let base = entry.name().rsplit(['/', '\\']).next().unwrap_or_default();
        if base.eq_ignore_ascii_case(file) {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
            }
            let mut out = std::fs::File::create(dest).map_err(|e| CoreError::io(dest, e))?;
            std::io::copy(&mut entry, &mut out).map_err(|_| damaged())?;
            return Ok(());
        }
    }
    Err(damaged())
}

fn sha256_of(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads `artifact` to `dest` unless an identical copy is already there.
async fn ensure_artifact(artifact: &Artifact, dest: &Path, label: &str) -> Result<(), CoreError> {
    if sha256_of(dest).is_some_and(|h| h.eq_ignore_ascii_case(&artifact.sha256)) {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
    }
    download(&artifact.url, dest, &artifact.sha256, label, |_, _| {}).await
}

fn installed_build(paths: &HearthPaths, minor: &str) -> Result<PhpBuild, CoreError> {
    let path = install_dir(paths, minor).join(BUILD_FILE);
    let bytes = std::fs::read(&path).map_err(|_| CoreError::Php(format!("PHP {minor} is not installed")))?;
    serde_json::from_slice(&bytes).map_err(|e| CoreError::Php(format!("PHP {minor} needs a reinstall: {e}")))
}

/// The generated php.ini part for an installed version, from its build and the extension switches.
fn generated_ini(paths: &HearthPaths, minor: &str) -> Result<String, CoreError> {
    let build = installed_build(paths, minor)?;
    let state = store::load(paths);
    let switches = state.extensions.get(minor);
    let extensions_on = build
        .extensions
        .iter()
        .filter(|(name, _)| switches.and_then(|s| s.get(*name)).copied().unwrap_or(false))
        .map(|(name, source)| (name.clone(), source.clone()))
        .collect();
    Ok(render_ini(&IniContext {
        os: Os::current(),
        minor: minor.into(),
        install_dir: install_dir(paths, minor),
        cacert: cacert_path(paths),
        extensions_on,
    }))
}

/// Rewrites php.ini for `minor` (generated part + user.ini).
pub fn apply_config(paths: &HearthPaths, minor: &str) -> Result<(), CoreError> {
    write_ini(paths, minor, &generated_ini(paths, minor)?)
}

/// Rewrites the shims for the current default and installed versions.
pub fn refresh_shims(paths: &HearthPaths) -> Result<(), CoreError> {
    let state = store::load(paths);
    let minors: Vec<String> = store::installed(paths).into_iter().map(|i| i.minor).collect();
    write_shims(paths, state.default.as_deref(), &minors)
}

/// Holds the one-writer lock for installs, php.json, php.ini and the shims.
fn locked(core: &CoreState) -> std::sync::MutexGuard<'_, ()> {
    core.php_lock.lock().unwrap_or_else(|e| e.into_inner())
}

/// Removes PHP `minor`. Fails without changing anything while one of its programs runs or while it
/// is being installed. Returns the default version afterwards. The user's user.ini is kept for a
/// later reinstall.
pub fn uninstall(core: &CoreState, minor: &str) -> Result<Option<String>, CoreError> {
    let installing = format!("php.install:{minor}");
    if core.jobs.list().iter().any(|j| j.key == installing && j.state == crate::jobs::JobState::Running) {
        return Err(CoreError::Php(format!("PHP {minor} is being installed; try again when it is done")));
    }
    let _guard = locked(core);
    let paths = &core.paths;
    let target = install_dir(paths, minor);
    if !target.join(VERSION_FILE).is_file() {
        return Err(CoreError::Php(format!("PHP {minor} is not installed")));
    }
    let old = target.with_file_name(format!("{minor}.old"));
    let _ = std::fs::remove_dir_all(&old);
    std::fs::rename(&target, &old)
        .map_err(|_| CoreError::Php(format!("Close the programs using PHP {minor} and try again")))?;
    let _ = std::fs::remove_dir_all(&old);
    let generated = ini_dir(paths, minor).join("php.ini");
    let _ = std::fs::remove_file(generated);
    let default = store::forget(paths, minor)?;
    refresh_shims(paths)?;
    Ok(default)
}

/// Makes `minor` the version behind `php`, `composer` and `laravel`.
pub fn use_version(core: &CoreState, minor: &str) -> Result<(), CoreError> {
    let _guard = locked(core);
    store::set_default(&core.paths, minor)?;
    refresh_shims(&core.paths)
}

pub fn set_extension(core: &CoreState, minor: &str, name: &str, on: bool) -> Result<(), CoreError> {
    let _guard = locked(core);
    store::set_extension(&core.paths, minor, name, on)?;
    apply_config(&core.paths, minor)
}

/// The generated part of php.ini and the user's own part.
pub fn ini_parts(paths: &HearthPaths, minor: &str) -> Result<(String, String), CoreError> {
    let generated = generated_ini(paths, minor)?;
    let user = std::fs::read_to_string(ini_dir(paths, minor).join("user.ini")).unwrap_or_default();
    Ok((generated, user))
}

/// Saves the user's part of php.ini and rewrites php.ini.
pub fn set_ini_user(core: &CoreState, minor: &str, user: &str) -> Result<(), CoreError> {
    let _guard = locked(core);
    let paths = &core.paths;
    installed_build(paths, minor)?;
    let dir = ini_dir(paths, minor);
    std::fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    crate::atomic::write(&dir.join("user.ini"), user.as_bytes())?;
    apply_config(paths, minor)
}

/// Installs the Laravel installer (`laravel new`) with Composer into `<data>/composer`, as a job.
/// Uses the default PHP; the `laravel` shim runs it.
pub fn install_laravel_installer(core: &CoreState) -> JobId {
    let (id, new) = core.jobs.start("tools.laravel", "tools.laravel", "Installing the Laravel installer");
    if new {
        let core = core.clone();
        tokio::spawn(async move {
            let result = run_laravel_installer(&core.paths).await;
            core.jobs.finish(id, result.map_err(|e| e.to_string()));
        });
    }
    id
}

async fn run_laravel_installer(paths: &HearthPaths) -> Result<(), CoreError> {
    let Some(minor) = store::load(paths).default else {
        return Err(CoreError::Php("Install a PHP version first".into()));
    };
    let php = match Os::current() {
        Os::Windows => install_dir(paths, &minor).join("php.exe"),
        Os::Macos => install_dir(paths, &minor).join("bin").join("php"),
    };
    let composer_home = paths.data.join("composer");
    let output = tokio::process::Command::new(php)
        .arg("-c")
        .arg(ini_dir(paths, &minor).join("php.ini"))
        .arg(paths.bin.join("composer.phar"))
        .args(["global", "require", "laravel/installer", "--no-interaction", "--no-progress"])
        .env("COMPOSER_HOME", &composer_home)
        // The installer starts PHP again for its own steps; those find php.ini through PHPRC.
        .env("PHPRC", ini_dir(paths, &minor))
        .output()
        .await
        .map_err(|e| CoreError::Php(format!("Could not run Composer: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
    Err(CoreError::Php(format!("Composer could not install the Laravel installer: {}", last.trim())))
}
