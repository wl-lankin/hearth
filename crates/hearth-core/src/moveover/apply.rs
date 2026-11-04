//! The move: carries out the plan step by step. Each step goes into the record first (or, where its
//! result is needed, right after), and steps already in the record are skipped, so a second run
//! after a failure continues where the first one stopped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::outside::{apply_outside, MachineWrite};
use super::plan::{MovePlan, HEARTH_EXTENSIONS};
use super::record::{Done, Record};
use super::scan::{IniKind, Inventory};
use super::state;
use crate::herd::import::ImportPlan;
use crate::{CoreError, HearthPaths};

/// What the move asks of Hearth; the real one calls Hearth's own methods, tests pass their own.
pub trait Mover {
    /// Stops Herd's app and services (by their install folders, never by name).
    fn stop_herd(&mut self, inv: &Inventory) -> Result<(), CoreError>;
    /// The program holding `port`, if any.
    fn port_user(&self, port: u16) -> Option<String>;
    fn import_sites(&mut self, plan: &ImportPlan) -> Result<(), CoreError>;
    /// Hearth's CA (certificate and key) as it is now.
    fn current_ca(&self) -> Option<(Vec<u8>, Vec<u8>)>;
    /// Makes Herd's CA Hearth's, so the site certificates are issued from it.
    fn adopt_ca(&mut self, crt: &Path, key: &Path) -> Result<(), CoreError>;
    fn ports(&self) -> (u16, u16);
    fn set_ports(&mut self, http: u16, https: u16) -> Result<(), CoreError>;
    fn php_installed(&self) -> Vec<String>;
    /// Installs a PHP minor and waits for it.
    fn install_php(&mut self, minor: &str) -> Result<(), CoreError>;
    fn php_default(&self) -> Option<String>;
    fn set_php_default(&mut self, minor: &str) -> Result<(), CoreError>;
    fn ini_user(&self, minor: &str) -> String;
    fn set_ini_user(&mut self, minor: &str, user: &str) -> Result<(), CoreError>;
    /// Switches an extension on.
    fn set_extension(&mut self, minor: &str, name: &str) -> Result<(), CoreError>;
    /// Creates and starts Hearth's Redis on `port` with Herd's saved data; returns its id.
    fn create_redis(&mut self, port: u16, dump: Option<&Path>) -> Result<String, CoreError>;
    fn smtp_port(&self) -> u16;
    fn set_smtp(&mut self, port: u16) -> Result<(), CoreError>;
    fn node_installed(&self) -> Vec<String>;
    /// Installs a Node version (`v22.9.0`) and waits for it.
    fn install_node(&mut self, version: &str) -> Result<(), CoreError>;
    fn node_default(&self) -> Option<String>;
    fn set_node_default(&mut self, version: &str) -> Result<(), CoreError>;
    /// The folder with the client programs of the MariaDB that listens on 3306, if found.
    fn mariadb_client_dir(&self) -> Option<PathBuf>;
    /// Hearth's `redis-cli`, if its Redis is installed.
    fn redis_cli(&self) -> Option<PathBuf>;
    /// Writes the forwarders (command name, program) into Hearth's `bin`.
    fn write_forwarders(&mut self, forwarders: &[(String, PathBuf)]) -> Result<(), CoreError>;
    fn hearth_bin(&self) -> String;
    fn hearth_exe(&self) -> String;
    /// Lets the web server follow the new sites, ports and certificates.
    fn sync(&mut self);
}

/// `user` (a version's user php.ini) with `settings` set: an existing line of a key is replaced.
pub fn merge_ini(user: &str, settings: &[(String, String)]) -> String {
    let mut lines: Vec<String> = user.lines().map(str::to_string).collect();
    for (key, value) in settings {
        let line = format!("{key} = {value}");
        let same = |l: &String| l.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key));
        match lines.iter_mut().find(|l| same(l)) {
            Some(existing) => *existing = line,
            None => lines.push(line),
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Carries out `plan` for `inv`. `progress` gets the share done and the current step.
pub fn run(
    paths: &HearthPaths,
    inv: &Inventory,
    plan: &MovePlan,
    record: &mut Record,
    mover: &mut dyn Mover,
    outside: &mut dyn MachineWrite,
    progress: &mut dyn FnMut(f64, &str),
) -> Result<(), CoreError> {
    let total = 11.0;
    let mut step = |n: f64, what: &str| progress(n / total, what);

    step(0.0, "Stopping Herd");
    if !record.has("herdStopped") {
        let app = inv.uninstaller.as_ref().and_then(|u| u.program.parent()).map(|d| d.join("Herd.exe"));
        record.push(paths, Done::HerdStopped { app: app.map(|p| p.to_string_lossy().into_owned()) })?;
        mover.stop_herd(inv)?;
        let mut ports = vec![80, 443];
        ports.extend(inv.redis.as_ref().map(|r| r.port));
        ports.extend(inv.smtp_port);
        for port in ports {
            if let Some(program) = mover.port_user(port) {
                return Err(CoreError::Php(format!("Port {port} is still in use by {program}")));
            }
        }
    }

    step(1.0, "Herd starting with Windows");
    if let (Some((name, value)), false) = (&inv.autostart, record.has("autostart")) {
        let done = Done::Autostart { name: name.clone(), value: value.clone() };
        record.push(paths, done.clone())?;
        apply_outside(outside, &done)?;
    }

    step(2.0, "Sites");
    if !record.has("imported") {
        record.push(paths, Done::Imported { plan: plan.sites.clone() })?;
        mover.import_sites(&plan.sites)?;
    }

    step(3.0, "HTTPS");
    if let (Some((crt, key)), false) = (&inv.ca, record.has("ca")) {
        record.push(paths, Done::Ca { previous: mover.current_ca() })?;
        mover.adopt_ca(crt, key)?;
    }
    if !record.has("ports") {
        let (http, https) = mover.ports();
        record.push(paths, Done::Ports { http, https })?;
        mover.set_ports(80, 443)?;
    }

    step(4.0, "PHP");
    if !record.has("php") {
        let have = mover.php_installed();
        let missing: Vec<String> = inv.php_versions.iter().filter(|v| !have.contains(v)).cloned().collect();
        let settings: BTreeMap<&String, Vec<(String, String)>> = inv
            .php_ini
            .iter()
            .map(|(minor, lines)| {
                let set = lines
                    .iter()
                    .filter(|l| l.kind == IniKind::Setting)
                    .map(|l| (l.key.clone(), l.value.clone()))
                    .collect::<Vec<_>>();
                (minor, set)
            })
            .filter(|(_, set)| !set.is_empty())
            .collect();
        let ini_before = settings.keys().map(|minor| ((*minor).clone(), mover.ini_user(minor))).collect();
        record
            .push(paths, Done::Php { installed: missing.clone(), default_before: mover.php_default(), ini_before })?;
        for minor in &missing {
            step(4.0, &format!("Installing PHP {minor}"));
            mover.install_php(minor)?;
        }
        if let Some(default) = &inv.php_default {
            mover.set_php_default(default)?;
        }
        for (minor, set) in &settings {
            let user = merge_ini(&mover.ini_user(minor), set);
            mover.set_ini_user(minor, &user)?;
        }
        for (minor, lines) in &inv.php_ini {
            for line in lines {
                if let IniKind::Extension(name) = &line.kind {
                    if HEARTH_EXTENSIONS.contains(&name.as_str()) {
                        mover.set_extension(minor, name)?;
                    }
                }
            }
        }
    }

    step(5.0, "Redis");
    if let (Some(redis), false) = (&inv.redis, record.has("redis")) {
        // Recorded after creation: Undo needs the id it gets.
        let id = mover.create_redis(redis.port, redis.dump.as_deref())?;
        record.push(paths, Done::Redis { id })?;
    }

    step(6.0, "Mail");
    if let (Some(port), false) = (inv.smtp_port, record.has("smtp")) {
        record.push(paths, Done::Smtp { previous: mover.smtp_port() })?;
        mover.set_smtp(port)?;
    }

    step(7.0, "Node");
    if !record.has("node") && !inv.node_versions.is_empty() {
        let have = mover.node_installed();
        let missing: Vec<String> = inv.node_versions.iter().filter(|v| !have.contains(v)).cloned().collect();
        record.push(paths, Done::Node { installed: missing.clone(), default_before: mover.node_default() })?;
        for version in &missing {
            step(7.0, &format!("Installing Node {version}"));
            mover.install_node(version)?;
        }
        if let Some(default) = &inv.node_default {
            mover.set_node_default(default)?;
        }
    }

    step(8.0, "Database and Redis commands");
    if !record.has("forwarders") {
        let exe = std::env::consts::EXE_SUFFIX;
        let mut forwarders = Vec::new();
        if let Some(dir) = mover.mariadb_client_dir() {
            for name in ["mysql", "mysqldump", "mariadb"] {
                forwarders.push((name.to_string(), dir.join(format!("{name}{exe}"))));
            }
        }
        if let Some(cli) = mover.redis_cli() {
            forwarders.push(("redis-cli".to_string(), cli));
        }
        record.push(paths, Done::Forwarders { names: forwarders.iter().map(|(n, _)| n.clone()).collect() })?;
        mover.write_forwarders(&forwarders)?;
    }

    step(9.0, "PATH and Claude");
    if !record.has("userPath") && !inv.user_path.is_empty() {
        let done =
            Done::UserPath { removed: inv.user_path.clone(), added: mover.hearth_bin(), before: outside.user_path() };
        record.push(paths, done.clone())?;
        apply_outside(outside, &done)?;
    }
    if !record.has("machineEnv") && (!inv.machine_path.is_empty() || !inv.machine_vars.is_empty()) {
        let done = Done::MachineEnv { path_removed: inv.machine_path.clone(), vars_removed: inv.machine_vars.clone() };
        record.push(paths, done.clone())?;
        apply_outside(outside, &done)?;
    }
    if let (Some(entry), false) = (&inv.mcp, record.has("mcp")) {
        let done = Done::Mcp { herd_entry: entry.clone(), hearth: mover.hearth_exe() };
        record.push(paths, done.clone())?;
        apply_outside(outside, &done)?;
    }

    step(10.0, "Finishing");
    let mut st = state::load(paths);
    st.moved = true;
    state::save(paths, &st)?;
    mover.sync();
    step(11.0, "Moved");
    Ok(())
}
