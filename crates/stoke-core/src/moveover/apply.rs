//! The move: carries out the plan step by step. Each step goes into the record before it is carried
//! out, with what Undo needs, and is marked finished once it is done. A second run after a failure
//! skips the finished steps and runs the others again for whatever is still missing; the record of
//! a step is written once, so Undo keeps the state from before the first run.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::outside::{apply_outside, MachineWrite};
use super::plan::{MovePlan, STOKE_EXTENSIONS};
use super::record::{Done, Record};
use super::scan::{IniKind, Inventory};
use super::state;
use crate::herd::import::ImportPlan;
use crate::{CoreError, StokePaths};

/// What the move asks of Stoke; the real one calls Stoke's own methods, tests pass their own.
pub trait Mover {
    /// Stops Herd's app and services (by their install folders, never by name). Nothing to do when
    /// they are stopped already.
    fn stop_herd(&mut self, inv: &Inventory) -> Result<(), CoreError>;
    /// The program holding `port`, if any, and if it is not Stoke's own.
    fn port_user(&self, port: u16) -> Option<String>;
    fn import_sites(&mut self, plan: &ImportPlan) -> Result<(), CoreError>;
    /// Stoke's CA (certificate and key) as it is now.
    fn current_ca(&self) -> Option<(Vec<u8>, Vec<u8>)>;
    /// Makes Herd's CA Stoke's, so the site certificates are issued from it.
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
    /// The id of a Stoke Redis that already uses `port`, if there is one.
    fn existing_redis(&self, port: u16) -> Option<String>;
    /// Creates Stoke's Redis on `port`; returns its id.
    fn create_redis(&mut self, port: u16) -> Result<String, CoreError>;
    /// Gives the Redis `id` Herd's saved data, switches its autostart on and starts it.
    fn fill_redis(&mut self, id: &str, dump: Option<&Path>) -> Result<(), CoreError>;
    fn smtp_port(&self) -> u16;
    fn set_smtp(&mut self, port: u16) -> Result<(), CoreError>;
    fn node_installed(&self) -> Vec<String>;
    /// Installs a Node version (`v22.9.0`) and waits for it.
    fn install_node(&mut self, version: &str) -> Result<(), CoreError>;
    fn node_default(&self) -> Option<String>;
    fn set_node_default(&mut self, version: &str) -> Result<(), CoreError>;
    /// The folder with the client programs of the MariaDB that listens on 3306, if found.
    fn mariadb_client_dir(&self) -> Option<PathBuf>;
    /// Stoke's `redis-cli`, if its Redis is installed.
    fn redis_cli(&self) -> Option<PathBuf>;
    /// Writes the forwarders (command name, program) into Stoke's `bin`.
    fn write_forwarders(&mut self, forwarders: &[(String, PathBuf)]) -> Result<(), CoreError>;
    fn stoke_bin(&self) -> String;
    fn stoke_exe(&self) -> String;
    /// Makes sure Stoke's helper service is installed and current (Windows asks for permission
    /// when it is not): it changes the machine environment, and writes the `.test` names Herd's
    /// hosts entries stop covering once Herd is gone.
    fn ensure_helper(&mut self) -> Result<(), CoreError>;
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

/// Records `done` unless its kind is recorded already; the recorded one (from the first run) is
/// returned, since it holds the state from before the move.
fn record_once(paths: &StokePaths, record: &mut Record, done: impl FnOnce() -> Done) -> Result<Done, CoreError> {
    let step = done();
    if let Some(existing) = record.get(step.kind()) {
        return Ok(existing.clone());
    }
    record.push(paths, step.clone())?;
    Ok(step)
}

/// Carries out `plan` for `inv`. `progress` gets the share done and the current step.
pub fn run(
    paths: &StokePaths,
    inv: &Inventory,
    plan: &MovePlan,
    record: &mut Record,
    mover: &mut dyn Mover,
    outside: &mut dyn MachineWrite,
    progress: &mut dyn FnMut(f64, &str),
) -> Result<(), CoreError> {
    let total = 11.0;
    let mut step = |n: f64, what: &str| progress(n / total, what);

    // Every run: Herd may have been started again since the last one.
    step(0.0, "Stopping Herd");
    let app = inv.uninstaller.as_ref().and_then(|u| u.program.parent()).map(|d| d.join("Herd.exe"));
    record_once(paths, record, || Done::HerdStopped { app: app.map(|p| p.to_string_lossy().into_owned()) })?;
    mover.stop_herd(inv)?;
    let mut ports = vec![80, 443];
    ports.extend(inv.redis.as_ref().map(|r| r.port));
    ports.extend(inv.smtp_port);
    for port in ports {
        if let Some(program) = mover.port_user(port) {
            return Err(CoreError::Php(format!("Port {port} is still in use by {program}")));
        }
    }

    step(1.0, "Herd starting with Windows");
    if let (Some((name, value)), false) = (&inv.autostart, record.finished("autostart")) {
        let done = record_once(paths, record, || Done::Autostart { name: name.clone(), value: value.clone() })?;
        apply_outside(outside, &done)?;
        record.finish(paths, "autostart")?;
    }

    step(2.0, "Sites");
    if !record.finished("imported") {
        let done = record_once(paths, record, || Done::Imported { plan: plan.sites.clone() })?;
        if let Done::Imported { plan } = &done {
            mover.import_sites(plan)?;
        }
        record.finish(paths, "imported")?;
    }

    step(3.0, "HTTPS");
    if let (Some((crt, key)), false) = (&inv.ca, record.finished("ca")) {
        record_once(paths, record, || Done::Ca { previous: mover.current_ca() })?;
        mover.adopt_ca(crt, key)?;
        record.finish(paths, "ca")?;
    }
    if !record.finished("ports") {
        let (http, https) = mover.ports();
        record_once(paths, record, || Done::Ports { http, https })?;
        mover.set_ports(80, 443)?;
        record.finish(paths, "ports")?;
    }

    step(4.0, "PHP");
    if !record.finished("php") {
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
        record_once(paths, record, || {
            let ini_before = settings.keys().map(|minor| ((*minor).clone(), mover.ini_user(minor))).collect();
            Done::Php { installed: missing.clone(), default_before: mover.php_default(), ini_before }
        })?;
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
                    if STOKE_EXTENSIONS.contains(&name.as_str()) {
                        mover.set_extension(minor, name)?;
                    }
                }
            }
        }
        record.finish(paths, "php")?;
    }

    step(5.0, "Redis");
    if let (Some(redis), false) = (&inv.redis, record.finished("redis")) {
        let done = match record.get("redis") {
            Some(done) => done.clone(),
            None => {
                // Recorded right after creation: Undo needs the id it gets.
                let done = match mover.existing_redis(redis.port) {
                    Some(id) => Done::Redis { id, created: false },
                    None => Done::Redis { id: mover.create_redis(redis.port)?, created: true },
                };
                record.push(paths, done.clone())?;
                done
            }
        };
        // A Redis that was there before keeps its own data.
        if let Done::Redis { id, created: true } = &done {
            mover.fill_redis(id, redis.dump.as_deref())?;
        }
        record.finish(paths, "redis")?;
    }

    step(6.0, "Mail");
    if let (Some(port), false) = (inv.smtp_port, record.finished("smtp")) {
        record_once(paths, record, || Done::Smtp { previous: mover.smtp_port() })?;
        mover.set_smtp(port)?;
        record.finish(paths, "smtp")?;
    }

    step(7.0, "Node");
    if !record.finished("node") && !inv.node_versions.is_empty() {
        let have = mover.node_installed();
        let missing: Vec<String> = inv.node_versions.iter().filter(|v| !have.contains(v)).cloned().collect();
        record_once(paths, record, || Done::Node { installed: missing.clone(), default_before: mover.node_default() })?;
        for version in &missing {
            step(7.0, &format!("Installing Node {version}"));
            mover.install_node(version)?;
        }
        if let Some(default) = &inv.node_default {
            mover.set_node_default(default)?;
        }
        record.finish(paths, "node")?;
    }

    step(8.0, "Database and Redis commands");
    if !record.finished("forwarders") {
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
        record_once(paths, record, || Done::Forwarders { names: forwarders.iter().map(|(n, _)| n.clone()).collect() })?;
        mover.write_forwarders(&forwarders)?;
        record.finish(paths, "forwarders")?;
    }

    step(9.0, "PATH and Claude");
    if !record.finished("userPath") && !inv.user_path.is_empty() {
        let before = outside.user_path()?;
        let done = record_once(paths, record, || Done::UserPath {
            removed: inv.user_path.clone(),
            added: mover.stoke_bin(),
            before,
        })?;
        apply_outside(outside, &done)?;
        record.finish(paths, "userPath")?;
    }
    if !record.finished("machineEnv") && (!inv.machine_path.is_empty() || !inv.machine_vars.is_empty()) {
        step(9.0, "Stoke's helper service");
        mover.ensure_helper()?;
        let done = record_once(paths, record, || Done::MachineEnv {
            path_removed: inv.machine_path.clone(),
            vars_removed: inv.machine_vars.clone(),
        })?;
        apply_outside(outside, &done)?;
        record.finish(paths, "machineEnv")?;
    }
    if let (Some(entry), false) = (&inv.mcp, record.finished("mcp")) {
        // The recorded entry is Herd's; after a first run, the scan may see Stoke's in its place.
        let done = record_once(paths, record, || Done::Mcp { herd_entry: entry.clone(), stoke: mover.stoke_exe() })?;
        apply_outside(outside, &done)?;
        record.finish(paths, "mcp")?;
    }

    step(10.0, "Finishing");
    let mut st = state::load(paths);
    st.moved = true;
    st.checked = false;
    state::save(paths, &st)?;
    mover.sync();
    step(11.0, "Moved");
    Ok(())
}
