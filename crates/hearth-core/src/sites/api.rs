//! The sites.* and web.* IPC methods. Changes are saved at once; the web server follows in the
//! background (`web.status` shows the result).

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS};
use crate::sites::registry;
use crate::tls;
use crate::web::service::{current_status, schedule_sync};
use crate::CoreError;

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(INVALID_PARAMS, message)
}

fn failed(e: CoreError) -> RpcError {
    RpcError::new(INTERNAL_ERROR, e.to_string())
}

fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params.get(key).and_then(Value::as_str).ok_or_else(|| invalid(format!("{key} must be a string")))
}

fn dir(params: &Value) -> Result<PathBuf, RpcError> {
    text(params, "dir").map(PathBuf::from)
}

/// Runs a registry change under the sites lock, then lets the web server follow.
fn change(core: &CoreState, f: impl FnOnce() -> Result<(), CoreError>) -> Result<Value, RpcError> {
    {
        let _guard = core.sites_lock.lock().unwrap_or_else(|e| e.into_inner());
        f().map_err(failed)?;
    }
    // A site that is gone (unlinked, unparked) takes its processes with it.
    crate::processes::run::prune(core).map_err(failed)?;
    schedule_sync(core);
    Ok(Value::Null)
}

/// Xdebug's port, where an IDE listens for debug sessions.
const XDEBUG_PORT: u16 = 9003;

/// Starts (or stops) an Xdebug session for every request of a site. Switching on also switches the
/// Xdebug extension on for the site's PHP; switching off leaves the extension as it is.
fn debug(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let paths = &core.paths;
    // The prepend script starts the session: php.ini loads it while any site is debugged.
    let with_prepend = |name: &str, on: bool| {
        let _php = core.php_lock.lock().unwrap_or_else(|e| e.into_inner());
        registry::set_debug(paths, name, on)?;
        crate::insights::prepend::apply_all(paths)
    };
    php_switch(core, params, "xdebug", "debug", &with_prepend)
}

/// Lets SPX profile (or stop profiling) every request of a site, like `debug` for Xdebug.
fn profile(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    php_switch(core, params, "spx", "profile", &|name, on| registry::set_profile(&core.paths, name, on))
}

/// A per-site switch that needs extension `ext` in the PHP the site runs on: switching on first
/// switches `ext` on there, then `set` records the site.
fn php_switch(
    core: &CoreState,
    params: &Value,
    ext: &str,
    what: &str,
    set: &dyn Fn(&str, bool) -> Result<(), CoreError>,
) -> Result<Value, RpcError> {
    let name = text(params, "name")?;
    let on = params.get("on").and_then(Value::as_bool).ok_or_else(|| invalid("on must be true or false"))?;
    let paths = &core.paths;
    if !on {
        return change(core, || set(name, false));
    }
    let site = registry::list(paths).into_iter().find(|s| s.name == name);
    let site = site.ok_or_else(|| failed(CoreError::Php(format!("No site named {name}.test"))))?;
    if site.kind == registry::SiteKind::Proxy {
        return Err(failed(CoreError::Php(format!("{name}.test is a proxy: it runs no PHP to {what}"))));
    }
    // The version the site really runs on, as the web sync picks it (a removed isolation falls back).
    let installed: Vec<String> = crate::php::store::installed(paths).into_iter().map(|i| i.minor).collect();
    let state = crate::php::store::load(paths);
    let (minor, note) = crate::web::pools::effective_php(site.php.as_deref(), &installed, state.default.as_deref());
    let minor = minor.ok_or_else(|| failed(CoreError::Php(note.unwrap_or_default())))?;
    // SPX has no Windows build: say so plainly instead of naming a missing extension.
    if ext == "spx" && !state.extensions.get(&minor).is_some_and(|e| e.contains_key("spx")) {
        return Err(failed(CoreError::Php(format!("SPX is not available for PHP {minor}"))));
    }
    crate::php::service::set_extension(core, &minor, ext, true).map_err(failed)?;
    change(core, || set(name, true))
}

/// Whether a program listens on Xdebug's port (or `port`, for tests), and which one.
fn debug_status(params: &Value) -> Value {
    let port = params.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).unwrap_or(XDEBUG_PORT);
    let found = crate::debug::ide::listener(port);
    json!({
        "port": port,
        "listening": found.is_some(),
        "program": found.and_then(|l| l.program),
    })
}

/// The sites, parked folders and HTTP port.
pub fn list(core: &CoreState) -> Value {
    let config = registry::load(&core.paths);
    json!({
        "sites": registry::list(&core.paths),
        "parked": config.parked,
        "httpPort": config.http_port,
        "httpsPort": config.https_port,
    })
}

/// Runs a blocking call that may show an OS prompt (UAC, a trust dialog), then lets the web
/// server pick up the result.
async fn prompting(
    core: &CoreState,
    f: impl FnOnce() -> Result<(), CoreError> + Send + 'static,
) -> Result<Value, RpcError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| RpcError::new(INTERNAL_ERROR, e.to_string()))?.map_err(failed)?;
    schedule_sync(core);
    Ok(Value::Null)
}

/// Reads Herd's sites (from `from`, or Herd's usual folder) and returns what importing them
/// would change; with `apply`, also does it.
fn herd_import(core: &CoreState, params: &Value) -> Result<Value, RpcError> {
    let valet = match params.get("from").and_then(Value::as_str) {
        Some(dir) => PathBuf::from(dir),
        None => crate::herd::default_valet_dir().ok_or_else(|| invalid("Herd's folder could not be found"))?,
    };
    let apply = params.get("apply").and_then(Value::as_bool).unwrap_or(false);
    let setup = crate::herd::read(&valet).map_err(|e| invalid(e.to_string()))?;
    let (plan, failed) = {
        let _guard = core.sites_lock.lock().unwrap_or_else(|e| e.into_inner());
        let plan = crate::herd::import::plan(&core.paths, &setup);
        let failed = if apply { crate::herd::import::apply(&core.paths, &plan) } else { vec![] };
        (plan, failed)
    };
    if apply {
        schedule_sync(core);
    }
    Ok(json!({ "from": valet, "plan": plan, "failed": failed }))
}

/// Asks the OS to trust Hearth's CA (creating it first if needed).
fn trust_ca(paths: crate::HearthPaths) -> Result<(), CoreError> {
    let ca = tls::ensure_ca(&paths)?;
    tls::trust::trust(&ca, &tls::ca_cert_path(&paths))
}

/// Answers a sites.* / web.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    let paths = &core.paths;
    let result = match method {
        "sites.list" => Ok(list(core)),
        "web.status" => Ok(serde_json::to_value(current_status(core)).unwrap_or(Value::Null)),
        "sites.park" => dir(params).and_then(|d| change(core, || registry::park(paths, &d))),
        "sites.unpark" => dir(params).and_then(|d| change(core, || registry::unpark(paths, &d))),
        "sites.link" => match (text(params, "name"), dir(params)) {
            (Ok(name), Ok(d)) => change(core, || registry::link(paths, name, &d)),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "sites.unlink" => text(params, "name").and_then(|n| change(core, || registry::unlink(paths, n))),
        "sites.proxy" => match (text(params, "name"), text(params, "upstream")) {
            (Ok(name), Ok(upstream)) => change(core, || registry::proxy(paths, name, upstream)),
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "sites.unproxy" => text(params, "name").and_then(|n| change(core, || registry::unproxy(paths, n))),
        "sites.isolate" => match (text(params, "name"), params.get("minor")) {
            (Ok(name), Some(Value::Null) | None) => change(core, || registry::isolate(paths, name, None)),
            (Ok(name), Some(Value::String(minor))) => change(core, || registry::isolate(paths, name, Some(minor))),
            (Ok(_), Some(_)) => Err(invalid("minor must be a string like \"8.4\" or null")),
            (Err(e), _) => Err(e),
        },
        "sites.debug" => debug(core, params),
        "sites.profile" => profile(core, params),
        // Off the async threads: on macOS the check runs lsof.
        "debug.status" => {
            let params = params.clone();
            tokio::task::spawn_blocking(move || debug_status(&params))
                .await
                .map_err(|e| failed(CoreError::Php(format!("The IDE check failed: {e}"))))
        }
        "herd.import" => herd_import(core, params),
        "web.restart" | "web.start" => {
            core.web_paused.store(false, std::sync::atomic::Ordering::Relaxed);
            if method == "web.restart" {
                let supervisor = core.supervisor.clone();
                let _ = tokio::task::spawn_blocking(move || supervisor.stop_all()).await;
            }
            schedule_sync(core);
            Ok(Value::Null)
        }
        "web.stop" => {
            core.web_paused.store(true, std::sync::atomic::Ordering::Relaxed);
            let supervisor = core.supervisor.clone();
            let _ = tokio::task::spawn_blocking(move || supervisor.stop_all()).await;
            Ok(Value::Null)
        }
        "sites.secure" => text(params, "name").and_then(|n| change(core, || registry::secure(paths, n))),
        "sites.unsecure" => text(params, "name").and_then(|n| change(core, || registry::unsecure(paths, n))),
        "tls.trust" => {
            let paths = paths.clone();
            prompting(core, move || trust_ca(paths)).await
        }
        "dns.setup" => match core.dns_pipe {
            Some(_) => prompting(core, crate::dns::setup).await,
            None => Err(RpcError::new(INVALID_PARAMS, "This Hearth home does not manage .test names")),
        },
        "sites.port" => match params.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()) {
            Some(port) => change(core, || registry::set_http_port(paths, port)),
            None => Err(invalid("port must be a number between 1 and 65535")),
        },
        _ => return None,
    };
    Some(result)
}
