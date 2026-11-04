//! Reading all of Herd into an inventory. Nothing here writes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::herd::read::{self as herd_read, HerdSetup};
use crate::CoreError;

/// Herd's home folder (`~/.config/herd` on Windows).
#[derive(Debug, Clone)]
pub struct HerdHome {
    pub root: PathBuf,
}

impl HerdHome {
    /// Herd on this computer, if any. Only Windows is moved over (the owner switches there).
    pub fn find() -> Option<HerdHome> {
        if !cfg!(windows) {
            return None;
        }
        HerdHome::at(&dirs::home_dir()?.join(".config").join("herd"))
    }

    /// `root` when it holds a Herd setup (its Valet config).
    pub fn at(root: &Path) -> Option<HerdHome> {
        root.join("config").join("valet").join("config.json").is_file().then(|| HerdHome { root: root.to_path_buf() })
    }

    pub fn valet(&self) -> PathBuf {
        self.root.join("config").join("valet")
    }
}

/// What Hearth needs to know about the computer outside Herd's folder.
pub trait Machine {
    fn user_path(&self) -> String;
    fn machine_path(&self) -> String;
    fn machine_var(&self, name: &str) -> Option<String>;
    /// The current user's `Run` values (name, command).
    fn run_values(&self) -> Vec<(String, String)>;
    /// Installed programs (display name, uninstall command).
    fn uninstall_entries(&self) -> Vec<(String, String)>;
    fn hosts(&self) -> String;
    fn claude_json(&self) -> Option<PathBuf>;
    fn read_link(&self, path: &Path) -> Option<PathBuf>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "name")]
pub enum IniKind {
    /// A setting the owner changed from PHP's development defaults.
    Setting,
    /// A line pointing into Herd's folders: it stops working once Herd is gone.
    HerdPath,
    /// An extension Herd loads on top of PHP's own.
    Extension(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IniLine {
    pub key: String,
    pub value: String,
    pub kind: IniKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HerdRedis {
    pub port: u16,
    pub dump: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Uninstaller {
    pub program: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    /// Herd's home folder (`~/.config/herd`).
    pub root: PathBuf,
    pub sites: HerdSetup,
    pub php_versions: Vec<String>,
    pub php_default: Option<String>,
    pub php_ini: BTreeMap<String, Vec<IniLine>>,
    pub ca: Option<(PathBuf, PathBuf)>,
    pub redis: Option<HerdRedis>,
    pub mariadb: Option<String>,
    pub smtp_port: Option<u16>,
    pub node_versions: Vec<String>,
    pub node_default: Option<String>,
    pub user_path: Vec<String>,
    pub machine_path: Vec<String>,
    pub machine_vars: Vec<(String, String)>,
    pub autostart: Option<(String, String)>,
    pub mcp: Option<Value>,
    pub uninstaller: Option<Uninstaller>,
    pub other_hosts: Vec<String>,
}

fn json(path: &Path) -> Option<Value> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// A port that Herd writes as a number or as text.
fn port(v: &Value) -> Option<u16> {
    v.as_u64().and_then(|p| u16::try_from(p).ok()).or_else(|| v.as_str()?.trim().parse().ok())
}

/// PHP's `php.ini-development` values for the settings people tune; a different value is the owner's.
const TUNED: &[(&str, &str)] = &[
    ("memory_limit", "128M"),
    ("upload_max_filesize", "2M"),
    ("post_max_size", "8M"),
    ("max_execution_time", "30"),
    ("max_input_time", "60"),
    ("max_input_vars", "1000"),
    ("max_file_uploads", "20"),
    ("default_socket_timeout", "60"),
    ("display_errors", "On"),
    ("display_startup_errors", "On"),
    ("error_reporting", "E_ALL"),
    ("log_errors", "On"),
    ("date.timezone", ""),
    ("short_open_tag", "Off"),
    ("realpath_cache_size", "4096k"),
    ("session.gc_maxlifetime", "1440"),
    ("opcache.enable", "1"),
    ("opcache.enable_cli", "0"),
    ("opcache.memory_consumption", "128"),
    ("opcache.validate_timestamps", "1"),
    ("opcache.revalidate_freq", "2"),
    ("xdebug.mode", "develop"),
    ("xdebug.start_with_request", "default"),
    ("xdebug.client_port", "9003"),
];

/// Extensions PHP brings itself (Hearth enables what it needs of them).
const BUILT_IN: &[&str] = &[
    "bcmath",
    "ffi",
    "bz2",
    "curl",
    "exif",
    "fileinfo",
    "ftp",
    "gd",
    "gettext",
    "gmp",
    "intl",
    "imap",
    "ldap",
    "mbstring",
    "mysqli",
    "odbc",
    "openssl",
    "pdo_mysql",
    "pdo_odbc",
    "pdo_pgsql",
    "pdo_sqlite",
    "pgsql",
    "shmop",
    "soap",
    "sockets",
    "sodium",
    "sqlite3",
    "tidy",
    "xsl",
    "zip",
    "opcache",
];

fn points_into_herd(value: &str) -> bool {
    let v = value.to_ascii_lowercase().replace('/', "\\");
    v.contains("\\.config\\herd") || v.contains("program files\\herd")
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    ['"', '\''].iter().find_map(|q| v.strip_prefix(*q).and_then(|s| s.strip_suffix(*q))).unwrap_or(v).to_string()
}

/// A value without a trailing `; comment` (a `;` inside quotes stays).
fn without_comment(value: &str) -> &str {
    let mut quote = None;
    for (i, c) in value.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, ';') => return value[..i].trim_end(),
            _ => {}
        }
    }
    value.trim_end()
}

/// `php_redis.dll`, `C:\...\php_redis.dll` or `redis` gives `redis`.
fn extension_name(value: &str) -> String {
    let file = unquote(value);
    let file = file.rsplit(['\\', '/']).next().unwrap_or(&file).to_ascii_lowercase();
    let file = file.strip_suffix(".dll").or_else(|| file.strip_suffix(".so")).unwrap_or(&file);
    let file = file.strip_prefix("php_").unwrap_or(file);
    // Herd names its builds per PHP version: `php_redis-8.4.dll`.
    match file.rsplit_once('-') {
        Some((name, version)) if version.chars().all(|c| c.is_ascii_digit() || c == '.') => name.to_string(),
        _ => file.to_string(),
    }
}

/// The lines of a php.ini that matter for the move: the owner's tuned settings, Herd's own paths,
/// and Herd's extra extensions.
pub fn ini_lines(text: &str) -> Vec<IniLine> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim().to_ascii_lowercase(), without_comment(value.trim()).to_string());
        let kind = if key == "extension" || key == "zend_extension" {
            let name = extension_name(&value);
            if BUILT_IN.contains(&name.as_str()) {
                continue;
            }
            IniKind::Extension(name)
        } else if points_into_herd(&value) {
            IniKind::HerdPath
        } else if let Some((_, default)) = TUNED.iter().find(|(k, _)| *k == key) {
            if unquote(&value).eq_ignore_ascii_case(default) {
                continue;
            }
            IniKind::Setting
        } else {
            continue;
        };
        lines.push(IniLine { key, value, kind });
    }
    lines
}

/// `8.5` from `bin/php.bat`'s `...\php85\php.exe`.
fn default_from_bat(bat: &str) -> Option<String> {
    let at = bat.to_ascii_lowercase().find("\\php8")? + 4;
    let digits: String = bat[at..].chars().take_while(char::is_ascii_digit).collect();
    (digits.len() == 2).then(|| format!("{}.{}", &digits[..1], &digits[1..]))
}

/// The names in Herd's block of the hosts file.
fn herd_hosts(hosts: &str) -> Vec<String> {
    let mut inside = false;
    let mut names = Vec::new();
    for line in hosts.lines() {
        let line = line.trim();
        if line.starts_with("# Herd generated Hosts") {
            inside = true;
        } else if line.starts_with("# End Herd generated Hosts") {
            inside = false;
        } else if inside && !line.starts_with('#') {
            names.extend(line.split_whitespace().skip(1).map(str::to_ascii_lowercase));
        }
    }
    names
}

/// The `.test` names of Herd's real sites: links, proxies and the folders in parked folders.
pub(crate) fn site_hosts(sites: &HerdSetup) -> BTreeSet<String> {
    let mut names: BTreeSet<String> =
        sites.links.keys().chain(sites.proxies.keys()).map(|n| format!("{n}.test")).collect();
    for dir in &sites.parked {
        for entry in std::fs::read_dir(dir).into_iter().flatten().filter_map(|e| e.ok()) {
            if entry.path().is_dir() {
                if let Some(name) = crate::sites::registry::name_from_folder(&entry.file_name().to_string_lossy()) {
                    names.insert(format!("{name}.test"));
                }
            }
        }
    }
    names
}

fn split_path(path: &str) -> Vec<String> {
    path.split(';').map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect()
}

/// `"C:\...\Uninstall Herd.exe" /allusers` as program and arguments.
fn split_command(command: &str) -> Option<Uninstaller> {
    let command = command.trim();
    let (program, rest) = match command.strip_prefix('"') {
        Some(rest) => {
            let end = rest.find('"')?;
            (&rest[..end], &rest[end + 1..])
        }
        None => command.split_once(' ').unwrap_or((command, "")),
    };
    Some(Uninstaller { program: PathBuf::from(program), args: rest.split_whitespace().map(str::to_string).collect() })
}

/// Reads all of Herd.
pub fn scan(herd: &HerdHome, machine: &dyn Machine) -> Result<Inventory, CoreError> {
    let sites = herd_read::read(&herd.valet())?;
    let config = herd.root.join("config");
    let bin = herd.root.join("bin");

    let php_json = json(&config.join("php.json")).unwrap_or_default();
    let mut php_versions: Vec<String> = php_json
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys())
        .filter_map(|k| k.strip_prefix("installed_"))
        .filter(|v| !v.starts_with("internal_"))
        .map(str::to_string)
        .collect();
    php_versions.sort_by(|a, b| crate::php::manifest::compare_versions(a, b));

    let herd_config = json(&config.join("config.json")).unwrap_or_default();
    let php_default = herd_config["activeVersion"]
        .as_str()
        .map(str::to_string)
        .or_else(|| std::fs::read_to_string(bin.join("php.bat")).ok().and_then(|b| default_from_bat(&b)));

    let php_ini = php_versions
        .iter()
        .filter_map(|v| {
            let text = std::fs::read_to_string(bin.join(format!("php{}", v.replace('.', ""))).join("php.ini")).ok()?;
            Some((v.clone(), ini_lines(&text)))
        })
        .collect();

    let ca_dir = herd.valet().join("CA");
    // Herd on Windows writes `.crt`; Valet elsewhere `.pem`.
    let ca = [
        ("LaravelValetCASelfSigned.crt", "LaravelValetCASelfSigned.key"),
        ("LaravelValetCASelfSigned.pem", "LaravelValetCASelfSigned.key"),
    ]
    .iter()
    .map(|(c, k)| (ca_dir.join(c), ca_dir.join(k)))
    .find(|(c, k)| c.is_file() && k.is_file());

    let services = json(&config.join("services.json")).unwrap_or_default();
    let services = services["services"].as_array().cloned().unwrap_or_default();
    let redis = services.iter().find(|s| s["type"] == "redis").and_then(|s| {
        let dump = s["id"].as_str().map(|id| config.join("services").join(id).join("dump.rdb")).filter(|p| p.is_file());
        Some(HerdRedis { port: port(&s["port"])?, dump })
    });
    let mariadb =
        services.iter().find(|s| s["type"] == "mariadb").and_then(|s| s["version"].as_str().map(str::to_string));

    let nvm = bin.join("nvm");
    let mut node_versions: Vec<String> = std::fs::read_dir(&nvm)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('v'))
        .collect();
    node_versions.sort_by(|a, b| crate::php::manifest::compare_versions(&a[1..], &b[1..]));
    let symlink = machine.machine_var("NVM_SYMLINK");
    let node_default = symlink
        .as_deref()
        .and_then(|link| machine.read_link(Path::new(link)))
        .and_then(|target| target.file_name().map(|n| n.to_string_lossy().into_owned()));

    let herd_root = herd.root.to_string_lossy().to_ascii_lowercase();
    let into_herd = |p: &str| p.to_ascii_lowercase().starts_with(&herd_root) || points_into_herd(p);
    let user_path = split_path(&machine.user_path()).into_iter().filter(|p| into_herd(p)).collect();
    let machine_path = split_path(&machine.machine_path())
        .into_iter()
        .filter(|p| into_herd(p) || symlink.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(p)))
        .collect();
    let machine_vars = ["NVM_HOME", "NVM_SYMLINK"]
        .iter()
        .filter_map(|name| machine.machine_var(name).map(|v| (name.to_string(), v)))
        .collect();

    let autostart = machine.run_values().into_iter().find(|(name, command)| {
        name.to_ascii_lowercase().contains("herd") && command.to_ascii_lowercase().contains("herd")
    });
    let mcp = machine.claude_json().and_then(|p| json(&p)).and_then(|j| j["mcpServers"].get("herd").cloned());
    let uninstaller = machine
        .uninstall_entries()
        .into_iter()
        .find(|(name, _)| name == "Herd")
        .and_then(|(_, command)| split_command(&command));

    let sites_hosts = site_hosts(&sites);
    let mut other_hosts: Vec<String> =
        herd_hosts(&machine.hosts()).into_iter().filter(|h| !sites_hosts.contains(h)).collect();
    other_hosts.sort();
    other_hosts.dedup();

    Ok(Inventory {
        root: herd.root.clone(),
        sites,
        php_versions,
        php_default,
        php_ini,
        ca,
        redis,
        mariadb,
        smtp_port: port(&herd_config["smtpPort"]),
        node_versions,
        node_default,
        user_path,
        machine_path,
        machine_vars,
        autostart,
        mcp,
        uninstaller,
        other_hosts,
    })
}
