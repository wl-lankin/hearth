//! The sites Stoke serves, kept in `<config>/sites.json`: parked folders (every subfolder is a
//! site), linked folders, proxies, PHP versions per site and the HTTP port.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::php::store::is_minor;
use crate::sites::detect::docroot;
use crate::{atomic, CoreError, StokePaths};

/// Stoke stays off ports 80/443 while Herd may still use them.
pub const DEFAULT_HTTP_PORT: u16 = 8080;
pub const DEFAULT_HTTPS_PORT: u16 = 8443;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SitesConfig {
    pub parked: Vec<PathBuf>,
    pub links: BTreeMap<String, PathBuf>,
    pub proxies: BTreeMap<String, String>,
    /// Site name -> PHP minor.
    pub isolated: BTreeMap<String, String>,
    /// Sites served over HTTPS.
    pub secured: BTreeSet<String>,
    /// Sites whose requests start an Xdebug session.
    pub debug: BTreeSet<String>,
    /// Sites whose requests SPX profiles.
    pub profile: BTreeSet<String>,
    pub http_port: u16,
    pub https_port: u16,
}

impl Default for SitesConfig {
    fn default() -> Self {
        SitesConfig {
            parked: Vec::new(),
            links: BTreeMap::new(),
            proxies: BTreeMap::new(),
            isolated: BTreeMap::new(),
            secured: BTreeSet::new(),
            debug: BTreeSet::new(),
            profile: BTreeSet::new(),
            http_port: DEFAULT_HTTP_PORT,
            https_port: DEFAULT_HTTPS_PORT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SiteKind {
    Parked,
    Linked,
    Proxy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    /// The name before `.test`.
    pub name: String,
    pub kind: SiteKind,
    pub path: Option<PathBuf>,
    pub upstream: Option<String>,
    pub docroot: Option<PathBuf>,
    /// The PHP minor this site is isolated to, if any.
    pub php: Option<String>,
    pub secured: bool,
    /// Xdebug starts a session for every request of this site.
    pub debug: bool,
    /// SPX profiles every request of this site.
    pub profile: bool,
    pub problem: Option<String>,
}

fn file(paths: &StokePaths) -> PathBuf {
    paths.config.join("sites.json")
}

/// Never fails: a file that does not parse is kept aside as `sites.json.corrupt-<unix-ts>`.
pub fn load(paths: &StokePaths) -> SitesConfig {
    let path = file(paths);
    let Ok(bytes) = std::fs::read(&path) else { return SitesConfig::default() };
    serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = std::fs::rename(&path, paths.config.join(format!("sites.json.corrupt-{ts}")));
        SitesConfig::default()
    })
}

fn save(paths: &StokePaths, config: &SitesConfig) -> Result<(), CoreError> {
    let mut text = serde_json::to_string_pretty(config).map_err(|e| CoreError::Php(e.to_string()))?;
    text.push('\n');
    atomic::write(&file(paths), text.as_bytes())
}

fn problem(message: &str) -> CoreError {
    CoreError::Php(message.into())
}

/// `api.shop`-style names: dot-separated labels of lowercase letters, digits and inner dashes.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn check_name(name: &str) -> Result<(), CoreError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(problem("Use lowercase letters, digits and dashes for the site name"))
    }
}

/// A folder name as a site name: lowercase, dots kept like Herd does ("api.shop" ->
/// "api.shop.test"), anything else becomes a dash ("blog site" -> "blog-site").
pub fn name_from_folder(folder: &str) -> Option<String> {
    let mapped: String = folder
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' { c } else { '-' })
        .collect();
    let label = |part: &str| part.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let name = mapped.split('.').map(label).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(".");
    valid_name(&name).then_some(name)
}

fn site_for_folder(name: String, kind: SiteKind, path: PathBuf, config: &SitesConfig) -> Site {
    let (docroot, problem) = if !path.is_dir() {
        (None, Some("Folder missing".to_string()))
    } else {
        match docroot(&path) {
            Ok(root) => (Some(root), None),
            Err(why) => (None, Some(why)),
        }
    };
    let php = config.isolated.get(&name).cloned();
    let secured = config.secured.contains(&name);
    let debug = config.debug.contains(&name);
    let profile = config.profile.contains(&name);
    Site { name, kind, path: Some(path), upstream: None, docroot, php, secured, debug, profile, problem }
}

/// Every site, sorted by name. A link wins over a parked folder of the same name.
pub fn list(paths: &StokePaths) -> Vec<Site> {
    let config = load(paths);
    let mut sites: BTreeMap<String, Site> = BTreeMap::new();
    for parked in &config.parked {
        let Ok(entries) = std::fs::read_dir(parked) else { continue };
        for entry in entries.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()) {
            let folder = entry.file_name().to_string_lossy().into_owned();
            if folder.starts_with('.') {
                continue;
            }
            if let Some(name) = name_from_folder(&folder) {
                sites.insert(name.clone(), site_for_folder(name, SiteKind::Parked, entry.path(), &config));
            }
        }
    }
    for (name, path) in &config.links {
        sites.insert(name.clone(), site_for_folder(name.clone(), SiteKind::Linked, path.clone(), &config));
    }
    for (name, upstream) in &config.proxies {
        sites.insert(
            name.clone(),
            Site {
                name: name.clone(),
                kind: SiteKind::Proxy,
                path: None,
                upstream: Some(upstream.clone()),
                docroot: None,
                php: None,
                secured: config.secured.contains(name),
                debug: false,
                profile: false,
                problem: None,
            },
        );
    }
    sites.into_values().collect()
}

fn absolute(dir: &Path) -> Result<PathBuf, CoreError> {
    let abs = std::path::absolute(dir).map_err(|e| CoreError::io(dir, e))?;
    if !abs.is_dir() {
        return Err(CoreError::Php(format!("{} is not a folder", abs.display())));
    }
    Ok(abs)
}

pub fn park(paths: &StokePaths, dir: &Path) -> Result<(), CoreError> {
    let dir = absolute(dir)?;
    let mut config = load(paths);
    if !config.parked.contains(&dir) {
        config.parked.push(dir);
    }
    save(paths, &config)
}

pub fn unpark(paths: &StokePaths, dir: &Path) -> Result<(), CoreError> {
    let dir = std::path::absolute(dir).map_err(|e| CoreError::io(dir, e))?;
    let mut config = load(paths);
    let before = config.parked.len();
    config.parked.retain(|p| p != &dir);
    if config.parked.len() == before {
        return Err(CoreError::Php(format!("{} is not parked", dir.display())));
    }
    save(paths, &config)
}

pub fn link(paths: &StokePaths, name: &str, dir: &Path) -> Result<(), CoreError> {
    check_name(name)?;
    let dir = absolute(dir)?;
    let mut config = load(paths);
    if config.proxies.contains_key(name) {
        return Err(CoreError::Php(format!("{name} is already a proxy")));
    }
    config.links.insert(name.into(), dir);
    save(paths, &config)
}

pub fn unlink(paths: &StokePaths, name: &str) -> Result<(), CoreError> {
    let mut config = load(paths);
    if config.links.remove(name).is_none() {
        return Err(CoreError::Php(format!("{name} is not a linked site")));
    }
    config.isolated.remove(name);
    config.secured.remove(name);
    config.debug.remove(name);
    config.profile.remove(name);
    save(paths, &config)
}

/// `http(s)://host[:port]` and nothing else (a trailing `/` is dropped): Caddy's proxy upstreams take
/// no path, and only host/port characters can reach the Caddyfile.
pub fn valid_upstream(upstream: &str) -> bool {
    let Some(rest) = upstream.strip_prefix("http://").or_else(|| upstream.strip_prefix("https://")) else {
        return false;
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        match v6.split_once(']') {
            Some((addr, after)) if !addr.is_empty() && addr.chars().all(|c| c.is_ascii_hexdigit() || c == ':') => {
                (addr, after.strip_prefix(':'))
            }
            _ => return false,
        }
    } else {
        match rest.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (rest, None),
        }
    };
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':') {
        return false;
    }
    match port {
        None => !rest.ends_with(':'),
        Some(p) => p.parse::<u16>().is_ok_and(|n| n > 0),
    }
}

pub fn proxy(paths: &StokePaths, name: &str, upstream: &str) -> Result<(), CoreError> {
    check_name(name)?;
    if !valid_upstream(upstream) {
        return Err(problem("Use an address like http://localhost:5173"));
    }
    let mut config = load(paths);
    if config.links.contains_key(name) {
        return Err(CoreError::Php(format!("{name} is already a linked site")));
    }
    config.proxies.insert(name.into(), upstream.trim_end_matches('/').into());
    save(paths, &config)
}

pub fn unproxy(paths: &StokePaths, name: &str) -> Result<(), CoreError> {
    let mut config = load(paths);
    if config.proxies.remove(name).is_none() {
        return Err(CoreError::Php(format!("{name} is not a proxy")));
    }
    config.secured.remove(name);
    save(paths, &config)
}

/// Runs site `name` on PHP `minor`, or on the default version with `None`.
pub fn isolate(paths: &StokePaths, name: &str, minor: Option<&str>) -> Result<(), CoreError> {
    if !list(paths).iter().any(|s| s.name == name && s.kind != SiteKind::Proxy) {
        return Err(CoreError::Php(format!("There is no site {name}")));
    }
    let mut config = load(paths);
    match minor {
        Some(m) if is_minor(m) => {
            config.isolated.insert(name.into(), m.into());
        }
        Some(m) => return Err(CoreError::Php(format!("{m} is not a PHP version like 8.4"))),
        None => {
            config.isolated.remove(name);
        }
    }
    save(paths, &config)
}

pub fn set_http_port(paths: &StokePaths, port: u16) -> Result<(), CoreError> {
    if port == 0 {
        return Err(problem("Use a port between 1 and 65535"));
    }
    let mut config = load(paths);
    config.http_port = port;
    save(paths, &config)
}

/// Serves site `name` over HTTPS.
pub fn secure(paths: &StokePaths, name: &str) -> Result<(), CoreError> {
    if !list(paths).iter().any(|s| s.name == name) {
        return Err(CoreError::Php(format!("No site named {name}.test")));
    }
    let mut config = load(paths);
    config.secured.insert(name.into());
    save(paths, &config)
}

/// Adds `name` to (or removes it from) one of the per-site switch sets, for PHP sites only; `what`
/// names the action in the error ("debug", "profile").
fn set_php_switch(
    paths: &StokePaths,
    name: &str,
    on: bool,
    what: &str,
    set: fn(&mut SitesConfig) -> &mut BTreeSet<String>,
) -> Result<(), CoreError> {
    let site = list(paths).into_iter().find(|s| s.name == name);
    match site {
        None => return Err(CoreError::Php(format!("No site named {name}.test"))),
        Some(s) if s.kind == SiteKind::Proxy => {
            return Err(CoreError::Php(format!("{name}.test is a proxy: it runs no PHP to {what}")))
        }
        Some(_) => {}
    }
    let mut config = load(paths);
    if on {
        set(&mut config).insert(name.into());
    } else {
        set(&mut config).remove(name);
    }
    save(paths, &config)
}

/// Starts (or stops) an Xdebug session for every request of PHP site `name`.
pub fn set_debug(paths: &StokePaths, name: &str, on: bool) -> Result<(), CoreError> {
    set_php_switch(paths, name, on, "debug", |c| &mut c.debug)
}

/// Lets SPX profile (or stop profiling) every request of PHP site `name`.
pub fn set_profile(paths: &StokePaths, name: &str, on: bool) -> Result<(), CoreError> {
    set_php_switch(paths, name, on, "profile", |c| &mut c.profile)
}

pub fn unsecure(paths: &StokePaths, name: &str) -> Result<(), CoreError> {
    let mut config = load(paths);
    config.secured.remove(name);
    save(paths, &config)
}

pub fn set_https_port(paths: &StokePaths, port: u16) -> Result<(), CoreError> {
    if port == 0 {
        return Err(problem("Use a port between 1 and 65535"));
    }
    let mut config = load(paths);
    config.https_port = port;
    save(paths, &config)
}
