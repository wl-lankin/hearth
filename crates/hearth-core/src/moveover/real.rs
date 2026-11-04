//! The real `Mover`: Hearth's own methods, called through its IPC router so the move runs exactly the
//! code the app runs. Used from a blocking thread inside the move's job.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::apply::Mover;
use super::scan::Inventory;
use super::stop;
use crate::herd::import::ImportPlan;
use crate::ipc::{router, CoreState};
use crate::sites::registry;
use crate::{CoreError, HearthPaths};

pub struct RealMover {
    core: CoreState,
    runtime: tokio::runtime::Handle,
}

impl RealMover {
    pub fn new(core: CoreState, runtime: tokio::runtime::Handle) -> RealMover {
        RealMover { core, runtime }
    }

    fn paths(&self) -> &HearthPaths {
        &self.core.paths
    }

    fn ipc(&self, method: &str, params: Value) -> Result<Value, CoreError> {
        let call = router(self.core.clone())(method.to_string(), params);
        self.runtime.block_on(call).map_err(|e| CoreError::Php(e.message))
    }

    /// Waits for a background job (installs) to finish.
    fn wait_job(&self, id: Value) -> Result<(), CoreError> {
        let deadline = Instant::now() + Duration::from_secs(30 * 60);
        loop {
            let job = self.ipc("jobs.get", json!({ "id": id }))?;
            match job["state"].as_str() {
                Some("done") => return Ok(()),
                Some("failed") => {
                    return Err(CoreError::Php(job["error"].as_str().unwrap_or("The install failed").to_string()))
                }
                _ if Instant::now() > deadline => return Err(CoreError::Php("The install took too long".into())),
                _ => std::thread::sleep(Duration::from_millis(500)),
            }
        }
    }

    fn tls_dir(&self) -> PathBuf {
        self.paths().data.join("tls")
    }
}

fn read_pair(crt: &Path, key: &Path) -> Option<(Vec<u8>, Vec<u8>)> {
    Some((std::fs::read(crt).ok()?, std::fs::read(key).ok()?))
}

impl Mover for RealMover {
    fn stop_herd(&mut self, inv: &Inventory) -> Result<(), CoreError> {
        // Redis writes its data first, so nothing since its last save is lost.
        if let Some(redis) = &inv.redis {
            let cli = inv.root.clone().join("bin").join("services").join("redis");
            let cli = std::fs::read_dir(&cli)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .map(|e| e.path().join(format!("redis-cli{}", std::env::consts::EXE_SUFFIX)))
                .find(|p| p.is_file());
            if let Some(cli) = cli {
                let _ = crate::workspaces::command(cli).args(["-p", &redis.port.to_string(), "save"]).output();
            }
        }
        let install = inv.uninstaller.as_ref().and_then(|u| u.program.parent().map(Path::to_path_buf));
        let home = inv.root.clone();
        let mut folders: Vec<&Path> = vec![home.as_path()];
        if let Some(dir) = install.as_deref() {
            folders.push(dir);
        }
        for (pid, program) in stop::processes() {
            if stop::inside(&program, &folders) {
                stop::stop_pid(pid);
            }
        }
        // The ports free up a moment after the processes end.
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline && !crate::services::ports::port_free(80) {
            std::thread::sleep(Duration::from_millis(300));
        }
        Ok(())
    }

    fn port_user(&self, port: u16) -> Option<String> {
        if crate::services::ports::port_free(port) {
            return None;
        }
        Some(stop::holder(port).unwrap_or_else(|| "another program".into()))
    }

    fn import_sites(&mut self, plan: &ImportPlan) -> Result<(), CoreError> {
        let failed = {
            let _guard = self.core.sites_lock.lock().unwrap_or_else(|e| e.into_inner());
            crate::herd::import::apply(self.paths(), plan)
        };
        match failed.first() {
            Some(f) => Err(CoreError::Php(format!("{}: {}", f.name, f.reason))),
            None => Ok(()),
        }
    }

    fn current_ca(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        read_pair(&self.tls_dir().join("ca.crt"), &self.tls_dir().join("ca.key"))
    }

    fn adopt_ca(&mut self, crt: &Path, key: &Path) -> Result<(), CoreError> {
        let dir = self.tls_dir();
        std::fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
        let before = self.current_ca();
        let (cert, k) = read_pair(crt, key)
            .ok_or_else(|| CoreError::Tls("Herd's certificate authority could not be read".into()))?;
        crate::atomic::write(&dir.join("ca.crt"), &cert)?;
        crate::atomic::write_private(&dir.join("ca.key"), &k)?;
        if crate::tls::ca::load_ca(self.paths()).is_none() {
            // Put Hearth's own back: a CA Hearth cannot sign with would break every HTTPS site.
            if let Some((c, k)) = before {
                crate::atomic::write(&dir.join("ca.crt"), &c)?;
                crate::atomic::write_private(&dir.join("ca.key"), &k)?;
            }
            return Err(CoreError::Tls("Hearth cannot sign certificates with Herd's certificate authority".into()));
        }
        // The site certificates are issued again, from Herd's CA, on the next sync.
        let _ = std::fs::remove_dir_all(dir.join("sites"));
        Ok(())
    }

    fn ports(&self) -> (u16, u16) {
        let config = registry::load(self.paths());
        (config.http_port, config.https_port)
    }

    fn set_ports(&mut self, http: u16, https: u16) -> Result<(), CoreError> {
        registry::set_http_port(self.paths(), http)?;
        registry::set_https_port(self.paths(), https)
    }

    fn php_installed(&self) -> Vec<String> {
        crate::php::store::installed(self.paths()).into_iter().map(|p| p.minor).collect()
    }

    fn install_php(&mut self, minor: &str) -> Result<(), CoreError> {
        let job = self.ipc("php.install", json!({ "minor": minor }))?;
        self.wait_job(job["jobId"].clone())
    }

    fn php_default(&self) -> Option<String> {
        crate::php::store::load(self.paths()).default
    }

    fn set_php_default(&mut self, minor: &str) -> Result<(), CoreError> {
        self.ipc("php.use", json!({ "minor": minor })).map(|_| ())
    }

    fn ini_user(&self, minor: &str) -> String {
        crate::php::service::ini_parts(self.paths(), minor).map(|(_, user)| user).unwrap_or_default()
    }

    fn set_ini_user(&mut self, minor: &str, user: &str) -> Result<(), CoreError> {
        self.ipc("php.ini.set", json!({ "minor": minor, "user": user })).map(|_| ())
    }

    fn set_extension(&mut self, minor: &str, name: &str) -> Result<(), CoreError> {
        self.ipc("php.extension", json!({ "minor": minor, "name": name, "on": true })).map(|_| ())
    }

    fn create_redis(&mut self, port: u16, dump: Option<&Path>) -> Result<String, CoreError> {
        let available = self.ipc("services.available", Value::Null)?;
        let line = available["builds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["kind"] == "redis")
            .filter_map(|b| b["line"].as_str())
            .max_by(|a, b| crate::php::manifest::compare_versions(a, b))
            .ok_or_else(|| CoreError::Php("No Redis build for this computer".into()))?
            .to_string();
        let made = self.ipc("services.create", json!({ "kind": "redis", "line": line, "port": port }))?;
        let id = made["id"].as_str().or_else(|| made["instance"]["id"].as_str()).unwrap_or("redis").to_string();
        if let Some(dump) = dump {
            let inst = crate::services::store::get(self.paths(), &id)?;
            let data = crate::services::store::data_dir(self.paths(), &inst);
            std::fs::create_dir_all(&data).map_err(|e| CoreError::io(&data, e))?;
            std::fs::copy(dump, data.join("dump.rdb")).map_err(|e| CoreError::io(dump, e))?;
        }
        self.ipc("services.autostart", json!({ "id": id, "on": true }))?;
        self.ipc("services.start", json!({ "id": id }))?;
        Ok(id)
    }

    fn smtp_port(&self) -> u16 {
        crate::mail::smtp::configured_port(self.paths())
    }

    fn set_smtp(&mut self, port: u16) -> Result<(), CoreError> {
        self.ipc("mail.port", json!({ "port": port })).map(|_| ())
    }

    fn node_installed(&self) -> Vec<String> {
        crate::node::store::installed(self.paths())
            .into_iter()
            .map(|n| if n.version.starts_with('v') { n.version } else { format!("v{}", n.version) })
            .collect()
    }

    fn install_node(&mut self, version: &str) -> Result<(), CoreError> {
        let job = self.ipc("node.install", json!({ "spec": version.trim_start_matches('v') }))?;
        self.wait_job(job["jobId"].clone())
    }

    fn node_default(&self) -> Option<String> {
        self.ipc("node.list", Value::Null)
            .ok()
            .and_then(|l| l["default"].as_str().map(|v| format!("v{}", v.trim_start_matches('v'))))
    }

    fn set_node_default(&mut self, version: &str) -> Result<(), CoreError> {
        self.ipc("node.default", json!({ "spec": version.trim_start_matches('v') })).map(|_| ())
    }

    fn mariadb_client_dir(&self) -> Option<PathBuf> {
        if !cfg!(windows) {
            return None;
        }
        // The Windows service that runs mysqld or mariadbd; its folder holds the client programs.
        let script = "Get-CimInstance Win32_Service | Where-Object { $_.PathName -match 'mysqld|mariadbd' } | ForEach-Object { $_.PathName }";
        let out = crate::workspaces::command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).lines().find_map(|line| {
            let line = line.trim();
            let program = match line.strip_prefix('"') {
                Some(rest) => rest.split('"').next()?,
                None => line.split_whitespace().next()?,
            };
            let dir = Path::new(program).parent()?.to_path_buf();
            dir.join("mysql.exe").is_file().then_some(dir)
        })
    }

    fn redis_cli(&self) -> Option<PathBuf> {
        let root = self.paths().data.join("services").join("bin").join("redis");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path().join(format!("redis-cli{}", std::env::consts::EXE_SUFFIX)))
            .filter(|p| p.is_file())
            .collect();
        found.sort();
        found.pop()
    }

    fn write_forwarders(&mut self, forwarders: &[(String, PathBuf)]) -> Result<(), CoreError> {
        crate::php::shims::set_forwarders(self.paths(), forwarders)?;
        crate::php::service::refresh_shims(self.paths())
    }

    fn hearth_bin(&self) -> String {
        self.paths().bin.to_string_lossy().into_owned()
    }

    fn hearth_exe(&self) -> String {
        self.paths().bin.join(format!("hearth{}", std::env::consts::EXE_SUFFIX)).to_string_lossy().into_owned()
    }

    fn sync(&mut self) {
        crate::web::service::schedule_sync(&self.core);
    }
}
