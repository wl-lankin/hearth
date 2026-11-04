use std::path::{Path, PathBuf};

use super::scan::{scan, HerdHome, IniKind, Machine};

/// A Windows machine as a test describes it.
#[derive(Default)]
pub(crate) struct FakeMachine {
    pub user_path: String,
    pub machine_path: String,
    pub vars: Vec<(String, String)>,
    pub run: Vec<(String, String)>,
    pub uninstall: Vec<(String, String)>,
    pub hosts: String,
    pub claude_json: Option<PathBuf>,
    pub links: Vec<(PathBuf, PathBuf)>,
}

impl Machine for FakeMachine {
    fn user_path(&self) -> String {
        self.user_path.clone()
    }
    fn machine_path(&self) -> String {
        self.machine_path.clone()
    }
    fn machine_var(&self, name: &str) -> Option<String> {
        self.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }
    fn run_values(&self) -> Vec<(String, String)> {
        self.run.clone()
    }
    fn uninstall_entries(&self) -> Vec<(String, String)> {
        self.uninstall.clone()
    }
    fn hosts(&self) -> String {
        self.hosts.clone()
    }
    fn claude_json(&self) -> Option<PathBuf> {
        self.claude_json.clone()
    }
    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        self.links.iter().find(|(from, _)| from == path).map(|(_, to)| to.clone())
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A made-up Herd home with the files `scan` reads, and a machine to match.
pub(crate) fn fake_herd(d: &Path) -> (HerdHome, FakeMachine) {
    let root = d.join("herd");
    let valet = root.join("config").join("valet");
    let projects = d.join("projects");
    std::fs::create_dir_all(projects.join("shop")).unwrap();
    std::fs::create_dir_all(valet.join("Sites")).unwrap();
    write(
        &valet.join("config.json"),
        &serde_json::json!({ "tld": "test", "paths": [valet.join("Sites"), projects] }).to_string(),
    );
    write(
        &valet.join("Nginx").join("vite.test.conf"),
        "server {\n  location / {\n    proxy_pass http://localhost:5173;\n  }\n}\n",
    );
    write(
        &root.join("config").join("php.json"),
        r#"{"installed_8.1":true,"installed_internal_8.1":true,"installed_8.4":true,"installed_8.5":true}"#,
    );
    write(&root.join("config").join("config.json"), r#"{"smtpPort":2525,"activeVersion":"8.5","autoStart":true}"#);
    write(&root.join("bin").join("php.bat"), "@echo off\r\n\"C:\\x\\.config\\herd\\bin\\php85\\php.exe\" %*");
    write(
        &root.join("bin").join("php85").join("php.ini"),
        "[PHP]\nmemory_limit=2048M\ndisplay_errors=On\nauto_prepend_file=\"C:\\x\\.config\\herd\\bin\\dump-loader.php\"\n\
         extension=\"C:\\Program Files\\Herd\\resources\\app.asar.unpacked\\resources\\bin\\php_redis.dll\"\n\
         extension=mbstring\nzend_extension=xdebug\nextension=\"C:\\Program Files\\Herd\\ext\\php_mongodb.dll\"\n",
    );
    let redis_id = "0c1e6aa5";
    write(
        &root.join("config").join("services.json"),
        &serde_json::json!({ "services": [
            { "id": redis_id, "type": "redis", "version": "7.0.0", "port": 6379 },
            { "id": "fb39", "type": "mariadb", "version": "10.11", "port": "3307" }
        ]})
        .to_string(),
    );
    write(&root.join("config").join("services").join(redis_id).join("dump.rdb"), "REDIS0011");
    for v in ["v21.7.0", "v22.9.0"] {
        std::fs::create_dir_all(root.join("bin").join("nvm").join(v)).unwrap();
    }
    let claude = d.join(".claude.json");
    write(&claude, r#"{"mcpServers":{"herd":{"type":"stdio","command":"cmd","args":["/c","herd","mcp"]},"other":{}}}"#);
    let herd_bin = root.join("bin").to_string_lossy().into_owned();
    let nvm = root.join("bin").join("nvm").to_string_lossy().into_owned();
    let machine = FakeMachine {
        user_path: format!(r"C:\a;{herd_bin};C:\b"),
        machine_path: format!(r"C:\Windows\System32;{nvm};C:\Program Files\nodejs"),
        vars: vec![("NVM_HOME".into(), nvm.clone()), ("NVM_SYMLINK".into(), r"C:\Program Files\nodejs".into())],
        run: vec![
            ("OneDrive".into(), r#""C:\OneDrive.exe" /background"#.into()),
            ("de.beyondco.herd".into(), r#""C:\Program Files\Herd\Herd.exe""#.into()),
        ],
        uninstall: vec![("Herd".into(), r#""C:\Program Files\Herd\Uninstall Herd.exe" /allusers"#.into())],
        hosts: "127.0.0.1 localhost\r\n# Herd generated Hosts. Do not change.\r\n127.0.0.1 shop.test\r\n\
                127.0.0.1 vite.test\r\n127.0.0.1 bcd-1.api.shop.test\r\n127.0.0.1 herd-studio.test\r\n\
                # End Herd generated Hosts\r\n"
            .into(),
        claude_json: Some(claude),
        links: vec![(PathBuf::from(r"C:\Program Files\nodejs"), root.join("bin").join("nvm").join("v22.9.0"))],
    };
    (HerdHome { root }, machine)
}

#[test]
fn the_inventory_holds_all_of_herd() {
    let d = tempfile::tempdir().unwrap();
    let (herd, machine) = fake_herd(d.path());
    let inv = scan(&herd, &machine).unwrap();

    assert_eq!(inv.sites.proxies.get("vite").map(String::as_str), Some("http://localhost:5173"));
    assert_eq!(inv.sites.parked.len(), 1, "Herd's own Sites folder is not parked");
    assert_eq!(inv.php_versions, ["8.1", "8.4", "8.5"]);
    assert_eq!(inv.php_default.as_deref(), Some("8.5"));

    let ini = &inv.php_ini["8.5"];
    let kind = |key: &str| ini.iter().find(|l| l.key == key).map(|l| l.kind.clone());
    assert_eq!(kind("memory_limit"), Some(IniKind::Setting));
    assert_eq!(ini.iter().find(|l| l.key == "memory_limit").unwrap().value, "2048M");
    assert_eq!(kind("display_errors"), None, "On is the development default, so no change of the owner's");
    assert_eq!(kind("auto_prepend_file"), Some(IniKind::HerdPath));
    let extensions: Vec<IniKind> = ini.iter().filter(|l| l.key == "extension").map(|l| l.kind.clone()).collect();
    assert!(extensions.contains(&IniKind::Extension("redis".into())));
    assert!(extensions.contains(&IniKind::Extension("mongodb".into())));
    assert!(!extensions.contains(&IniKind::Extension("mbstring".into())), "built-in extensions are no change");
    assert!(ini.iter().any(|l| l.kind == IniKind::Extension("xdebug".into())));

    let redis = inv.redis.as_ref().expect("redis");
    assert_eq!(redis.port, 6379);
    assert!(redis.dump.as_ref().is_some_and(|p| p.ends_with("dump.rdb")));
    assert_eq!(inv.mariadb.as_deref(), Some("10.11"));
    assert_eq!(inv.smtp_port, Some(2525));

    assert_eq!(inv.node_versions, ["v21.7.0", "v22.9.0"]);
    assert_eq!(inv.node_default.as_deref(), Some("v22.9.0"));

    assert_eq!(inv.user_path, [herd.root.join("bin").to_string_lossy().into_owned()]);
    assert_eq!(inv.machine_path.len(), 2, "{:?}", inv.machine_path);
    assert_eq!(inv.machine_vars.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["NVM_HOME", "NVM_SYMLINK"]);
    assert_eq!(inv.autostart.as_ref().map(|(name, _)| name.as_str()), Some("de.beyondco.herd"));
    assert_eq!(inv.mcp.as_ref().and_then(|m| m["command"].as_str()), Some("cmd"));
    let uninstaller = inv.uninstaller.as_ref().expect("uninstaller");
    assert!(uninstaller.program.ends_with("Uninstall Herd.exe"));
    assert_eq!(uninstaller.args, ["/allusers"]);

    assert_eq!(inv.other_hosts, ["bcd-1.api.shop.test", "herd-studio.test"]);
}

#[test]
fn no_herd_folder_means_no_move() {
    let d = tempfile::tempdir().unwrap();
    assert!(HerdHome::at(&d.path().join("nope")).is_none());
    assert!(HerdHome::at(d.path()).is_none(), "a folder without Herd's config is no Herd");
}

#[cfg(windows)]
#[test]
fn the_real_machine_reads_this_computer() {
    use super::machine::RealMachine;
    let m = RealMachine;
    assert!(!m.machine_path().is_empty(), "every Windows has a machine PATH");
    assert!(m.uninstall_entries().iter().any(|(name, _)| !name.is_empty()));
    assert!(m.hosts().contains("localhost") || !m.hosts().is_empty());
}
