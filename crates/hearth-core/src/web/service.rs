//! Keeps Caddy and the PHP pools in step with the sites: which pools run, what the Caddyfile says,
//! whether Caddy runs. Called after every change to sites or PHP versions and from a health loop.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::dns::{self, DnsStatus};
use crate::ipc::CoreState;
use crate::php::manifest::Os;
use crate::php::store;
use crate::sites::caddyfile::{render, servable, CaddyContext, Route, SiteRoute};
use crate::sites::registry::{self, SiteKind};
use crate::supervisor::{ProcessSpec, ProcessStatus};
use crate::tls::{self, LeafState};
use crate::web::caddy_install::ensure_caddy;
use crate::web::pools::{effective_php, pool_specs, pool_upstreams, write_pool_config};
use crate::CoreError;

const CADDY: &str = "caddy";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebStatus {
    pub http_port: u16,
    pub https_port: u16,
    /// Browsers trust Hearth's CA (false before the first secured site creates it).
    pub ca_trusted: bool,
    /// Whether `<site>.test` names resolve.
    pub dns: DnsStatus,
    /// Caddy is serving.
    pub running: bool,
    pub processes: Vec<ProcessStatus>,
    /// The latest problem (port taken, invalid config, a pool that would not start).
    pub error: Option<String>,
    /// Site name -> a note, e.g. "Uses the default PHP 8.4 (8.3 was removed)".
    pub notes: BTreeMap<String, String>,
}

fn caddyfile_path(core: &CoreState) -> PathBuf {
    core.paths.config.join("caddy").join("Caddyfile")
}

/// Installs Caddy if needed, then syncs.
pub async fn sync(core: &CoreState) -> Result<WebStatus, CoreError> {
    let caddy = ensure_caddy(core).await?;
    let core = core.clone();
    tokio::task::spawn_blocking(move || sync_with(&core, &caddy)).await.map_err(|e| CoreError::Php(e.to_string()))?
}

/// The sync itself, with a given Caddy program. Blocking (starts processes and waits for them).
pub fn sync_with(core: &CoreState, caddy: &Path) -> Result<WebStatus, CoreError> {
    let guard = core.sites_lock.lock().unwrap_or_else(|e| e.into_inner());
    let paths = &core.paths;
    let config = registry::load(paths);
    let php_state = store::load(paths);
    let installed: Vec<String> = store::installed(paths).into_iter().map(|i| i.minor).collect();
    let mut notes = BTreeMap::new();
    let mut errors = Vec::new();

    let mut routes = Vec::new();
    for site in registry::list(paths) {
        let target = match (site.kind, &site.problem, &site.docroot) {
            (SiteKind::Proxy, _, _) => Route::Proxy { upstream: site.upstream.clone().unwrap_or_default() },
            (_, Some(problem), _) => Route::Skip { reason: problem.clone() },
            (_, None, Some(docroot)) if servable(docroot, Os::current() == Os::Windows).is_err() => {
                Route::Skip { reason: servable(docroot, Os::current() == Os::Windows).unwrap_err() }
            }
            (_, None, Some(docroot)) => {
                let (minor, note) = effective_php(site.php.as_deref(), &installed, php_state.default.as_deref());
                if let Some(note) = note.clone() {
                    notes.insert(site.name.clone(), note);
                }
                match minor {
                    Some(minor) => Route::Php { docroot: docroot.clone(), minor },
                    None => Route::Skip { reason: note.unwrap_or_default() },
                }
            }
            (_, None, None) => Route::Skip { reason: "No web root".into() },
        };
        routes.push(SiteRoute { name: site.name, target, tls: None });
    }

    // Certificates for secured sites; a new one needs a Caddy restart (Caddy reads them at start).
    let mut reissued = false;
    let secured: BTreeSet<String> =
        routes.iter().filter(|r| config.secured.contains(&r.name)).map(|r| r.name.clone()).collect();
    if !secured.is_empty() {
        match tls::ensure_ca(paths) {
            Ok(ca) => {
                let now = time::OffsetDateTime::now_utc();
                for route in routes.iter_mut().filter(|r| secured.contains(&r.name)) {
                    match tls::ensure_leaf(paths, &ca, &route.name, now) {
                        Ok(state) => {
                            reissued |= state == LeafState::Issued;
                            route.tls = Some(tls::leaf_paths(paths, &route.name));
                        }
                        Err(e) => errors.push(format!("{}.test: {e}", route.name)),
                    }
                }
            }
            Err(e) => errors.push(e.to_string()),
        }
    }
    remove_stale_leaves(paths, &secured);
    let ca_trusted = trusted_ca(paths);
    // Every site's name, served or not: a name that resolves shows the problem page, not "not found".
    let names: BTreeSet<String> = routes.iter().map(|r| format!("{}.test", r.name)).collect();

    // PHP pools: the versions some site uses, nothing else.
    let used: BTreeSet<String> = routes
        .iter()
        .filter_map(|r| match &r.target {
            Route::Php { minor, .. } => Some(minor.clone()),
            _ => None,
        })
        .collect();
    let mut wanted_keys: BTreeSet<String> = BTreeSet::new();
    let mut upstreams = BTreeMap::new();
    for minor in &used {
        let specs = pool_specs(paths, Os::current(), minor);
        wanted_keys.extend(specs.iter().map(|s| s.key.clone()));
        let started = write_pool_config(paths, minor)
            .and_then(|()| specs.into_iter().try_for_each(|spec| core.supervisor.ensure(spec)));
        match started {
            Ok(()) => {
                upstreams.insert(minor.clone(), pool_upstreams(paths, Os::current(), minor));
            }
            Err(e) => errors.push(format!("PHP {minor}: {e}")),
        }
    }
    for key in core.supervisor.keys() {
        if key.starts_with("php-") && !wanted_keys.contains(&key) {
            core.supervisor.stop(&key);
        }
    }

    // Caddy: only with something to serve.
    // Caddy only with a site it can serve (comments alone would leave it without a listener).
    let serving = routes.iter().any(|r| !matches!(r.target, Route::Skip { .. }));
    if !serving {
        core.supervisor.stop(CADDY);
    } else {
        let rendered = render(&CaddyContext {
            http_port: config.http_port,
            https_port: config.https_port,
            routes,
            php_upstreams: upstreams,
            log: paths.logs.join("caddy.log"),
            windows_paths: Os::current() == Os::Windows,
        });
        let path = caddyfile_path(core);
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        let mut changed = false;
        if rendered != current {
            match validated(caddy, &path, &rendered) {
                Ok(()) => changed = true,
                Err(e) => errors.push(e),
            }
        }
        if path.is_file() {
            if changed || reissued {
                core.supervisor.stop(CADDY);
            }
            let spec = ProcessSpec {
                key: CADDY.into(),
                program: caddy.to_path_buf(),
                args: vec![
                    "run".into(),
                    "--config".into(),
                    path.clone().into_os_string(),
                    "--adapter".into(),
                    "caddyfile".into(),
                ],
                env: vec![],
                cwd: None,
                log: paths.logs.join("caddy-process.log"),
                ready_port: Some(config.http_port),
                ready_socket: None,
            };
            if let Err(e) = core.supervisor.ensure(spec) {
                errors.push(e.to_string());
            }
        }
    }

    let processes = core.supervisor.status();
    let running = processes.iter().any(|p| p.key == CADDY && p.running);
    // A process that should run but does not (crashed, waiting to restart) is never silent.
    if serving {
        wanted_keys.insert(CADDY.into());
    }
    for p in processes.iter().filter(|p| wanted_keys.contains(&p.key) && !p.running) {
        let why = p.last_error.clone().unwrap_or_else(|| format!("{} is not running", p.key));
        if !errors.contains(&why) {
            errors.push(why);
        }
    }
    let error = (!errors.is_empty()).then(|| errors.join("; "));
    // Outside the sites lock: talking to the helper may take a few seconds, site changes may not.
    drop(guard);
    let dns = match &core.dns_pipe {
        Some(pipe) => dns::sync_names(pipe, &names),
        None => DnsStatus::default(),
    };
    Ok(WebStatus {
        http_port: config.http_port,
        https_port: config.https_port,
        ca_trusted,
        dns,
        running,
        processes,
        error,
        notes,
    })
}

/// Deletes the certificates of sites that are no longer secured.
fn remove_stale_leaves(paths: &crate::HearthPaths, secured: &BTreeSet<String>) {
    let dir = tls::leaf_paths(paths, "x").0.parent().map(Path::to_path_buf).unwrap_or_default();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if !secured.contains(&stem) {
            tls::remove_leaf(paths, &stem);
        }
    }
}

/// Whether the OS trusts Hearth's CA; false while there is none yet (it is created on first use).
fn trusted_ca(paths: &crate::HearthPaths) -> bool {
    tls::load_ca(paths).is_some_and(|ca| tls::trust::is_trusted(&ca, &tls::ca_cert_path(paths)))
}

/// Writes `rendered` as the new Caddyfile only when Caddy accepts it; otherwise the last good one stays.
fn validated(caddy: &Path, path: &Path, rendered: &str) -> Result<(), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let candidate = dir.join("Caddyfile.new");
    std::fs::write(&candidate, rendered).map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(caddy);
    command.args(["validate", "--config"]).arg(&candidate).args(["--adapter", "caddyfile"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Caddy is a console program; without this, every config change flashes a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().map_err(|e| format!("Could not run Caddy: {e}"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&candidate);
        let text = String::from_utf8_lossy(&output.stderr);
        let line = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").trim();
        return Err(format!("The web server config was not applied: {line}"));
    }
    crate::atomic::write(path, rendered.as_bytes()).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&candidate);
    Ok(())
}

/// The latest sync result, or a resting status before the first sync.
pub fn current_status(core: &CoreState) -> WebStatus {
    if let Some(status) = core.web_status.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return status;
    }
    let config = registry::load(&core.paths);
    WebStatus {
        http_port: config.http_port,
        https_port: config.https_port,
        ca_trusted: false,
        dns: DnsStatus::default(),
        running: false,
        processes: core.supervisor.status(),
        error: None,
        notes: BTreeMap::new(),
    }
}

/// Syncs in the background (when the host turned syncing on) and keeps the result for `web.status`.
/// Returns whether a sync was scheduled (not when syncing is off or Hearth is quitting).
pub fn schedule_sync(core: &CoreState) -> bool {
    let paused = core.web_paused.load(std::sync::atomic::Ordering::Relaxed);
    if !core.web_sync || paused || core.quitting.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let core = core.clone();
    tokio::spawn(async move {
        let status = match sync(&core).await {
            Ok(status) => status,
            Err(e) => WebStatus { error: Some(e.to_string()), ..current_status(&core) },
        };
        *core.web_status.lock().unwrap_or_else(|e| e.into_inner()) = Some(status);
    });
    true
}
