use std::path::{Path, PathBuf};

use super::apply::{run, Mover};
use super::outside::MachineWrite;
use super::plan::plan;
use super::record::{Done, Record};
use super::scan::{scan, Inventory};
use super::scan_tests::fake_herd;
use crate::dns::protocol::Request;
use crate::herd::import::ImportPlan;
use crate::{CoreError, HearthPaths};

/// Logs every call; fails once at `fail_at`; reports `busy` ports as taken by "nginx.exe";
/// `redis_before` is a Hearth Redis that was there before the move.
#[derive(Default)]
struct FakeMover {
    calls: Vec<String>,
    fail_at: Option<String>,
    busy: Vec<u16>,
    redis_before: Option<String>,
}

impl FakeMover {
    fn call(&mut self, name: impl Into<String>) -> Result<(), CoreError> {
        let name = name.into();
        if self.fail_at.as_deref() == Some(name.as_str()) {
            self.fail_at = None;
            return Err(CoreError::Php(format!("{name} failed")));
        }
        self.calls.push(name);
        Ok(())
    }
}

impl Mover for FakeMover {
    fn stop_herd(&mut self, _: &Inventory) -> Result<(), CoreError> {
        self.call("stop_herd")
    }
    fn port_user(&self, port: u16) -> Option<String> {
        self.busy.contains(&port).then(|| "nginx.exe".to_string())
    }
    fn import_sites(&mut self, _: &ImportPlan) -> Result<(), CoreError> {
        self.call("import_sites")
    }
    fn current_ca(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        None
    }
    fn adopt_ca(&mut self, _: &Path, _: &Path) -> Result<(), CoreError> {
        self.call("adopt_ca")
    }
    fn ports(&self) -> (u16, u16) {
        (8080, 8443)
    }
    fn set_ports(&mut self, http: u16, https: u16) -> Result<(), CoreError> {
        self.call(format!("set_ports {http} {https}"))
    }
    fn php_installed(&self) -> Vec<String> {
        let mut have = vec!["8.4".to_string()];
        have.extend(self.calls.iter().filter_map(|c| c.strip_prefix("install_php ")).map(str::to_string));
        have
    }
    fn install_php(&mut self, minor: &str) -> Result<(), CoreError> {
        self.call(format!("install_php {minor}"))
    }
    fn php_default(&self) -> Option<String> {
        Some("8.4".into())
    }
    fn set_php_default(&mut self, minor: &str) -> Result<(), CoreError> {
        self.call(format!("set_php_default {minor}"))
    }
    fn ini_user(&self, _: &str) -> String {
        "short_open_tag = On\n".into()
    }
    fn set_ini_user(&mut self, minor: &str, user: &str) -> Result<(), CoreError> {
        self.call(format!("set_ini_user {minor} {}", user.replace('\n', "|")))
    }
    fn set_extension(&mut self, minor: &str, name: &str) -> Result<(), CoreError> {
        self.call(format!("set_extension {minor} {name}"))
    }
    fn existing_redis(&self, _: u16) -> Option<String> {
        self.redis_before.clone()
    }
    fn create_redis(&mut self, port: u16) -> Result<String, CoreError> {
        self.call(format!("create_redis {port}"))?;
        Ok("redis".into())
    }
    fn fill_redis(&mut self, id: &str, dump: Option<&Path>) -> Result<(), CoreError> {
        self.call(format!("fill_redis {id} {}", dump.is_some()))
    }
    fn smtp_port(&self) -> u16 {
        2525
    }
    fn set_smtp(&mut self, port: u16) -> Result<(), CoreError> {
        self.call(format!("set_smtp {port}"))
    }
    fn node_installed(&self) -> Vec<String> {
        self.calls.iter().filter_map(|c| c.strip_prefix("install_node ")).map(str::to_string).collect()
    }
    fn install_node(&mut self, version: &str) -> Result<(), CoreError> {
        self.call(format!("install_node {version}"))
    }
    fn node_default(&self) -> Option<String> {
        None
    }
    fn set_node_default(&mut self, version: &str) -> Result<(), CoreError> {
        self.call(format!("set_node_default {version}"))
    }
    fn mariadb_client_dir(&self) -> Option<PathBuf> {
        Some(PathBuf::from(r"C:\Program Files\MariaDB 11.4\bin"))
    }
    fn redis_cli(&self) -> Option<PathBuf> {
        Some(PathBuf::from(r"C:\h\data\services\bin\redis\8\redis-cli.exe"))
    }
    fn write_forwarders(&mut self, forwarders: &[(String, PathBuf)]) -> Result<(), CoreError> {
        let names: Vec<&str> = forwarders.iter().map(|(n, _)| n.as_str()).collect();
        self.call(format!("write_forwarders {}", names.join(",")))
    }
    fn hearth_bin(&self) -> String {
        r"C:\h\bin".into()
    }
    fn hearth_exe(&self) -> String {
        r"C:\h\bin\hearth.exe".into()
    }
    fn sync(&mut self) {
        self.calls.push("sync".into());
    }
}

#[derive(Default)]
struct FakeWrite {
    user_path: String,
    log: Vec<String>,
    /// A `claude` call starting with this fails once.
    fail_claude: Option<String>,
}

impl MachineWrite for FakeWrite {
    fn user_path(&self) -> Result<String, CoreError> {
        Ok(self.user_path.clone())
    }
    fn set_user_path(&mut self, value: &str) -> Result<(), CoreError> {
        self.user_path = value.into();
        self.log.push("user_path".into());
        Ok(())
    }
    fn set_run(&mut self, name: &str, _: Option<&str>) -> Result<(), CoreError> {
        self.log.push(format!("run {name}"));
        Ok(())
    }
    fn claude(&mut self, args: &[String]) -> Result<(), CoreError> {
        let line = format!("claude {}", args.join(" "));
        if self.fail_claude.as_deref().is_some_and(|f| line.starts_with(f)) {
            self.fail_claude = None;
            return Err(CoreError::Php("claude failed".into()));
        }
        self.log.push(line);
        Ok(())
    }
    fn helper(&mut self, _: &Request) -> Result<(), CoreError> {
        self.log.push("helper".into());
        Ok(())
    }
}

fn setup() -> (tempfile::TempDir, HearthPaths, Inventory) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    let (herd, machine) = fake_herd(d.path());
    let inv = scan(&herd, &machine).unwrap();
    (d, paths, inv)
}

fn no_progress(_: f64, _: &str) {}

#[test]
fn the_move_runs_every_step_in_order_and_records_it() {
    let (_d, paths, inv) = setup();
    let plan = plan(&paths, &inv, &["8.4".to_string()]);
    let (mut mover, mut write) =
        (FakeMover::default(), FakeWrite { user_path: inv.user_path.join(";"), ..FakeWrite::default() });
    let mut record = Record::default();
    run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).unwrap();
    assert_eq!(
        mover.calls,
        [
            "stop_herd",
            "import_sites",
            "adopt_ca",
            "set_ports 80 443",
            "install_php 8.1",
            "install_php 8.5",
            "set_php_default 8.5",
            "set_ini_user 8.5 short_open_tag = On|memory_limit = 2048M|",
            "set_extension 8.5 redis",
            "set_extension 8.5 xdebug",
            "create_redis 6379",
            "fill_redis redis true",
            "set_smtp 2525",
            "install_node v21.7.0",
            "install_node v22.9.0",
            "set_node_default v22.9.0",
            "write_forwarders mysql,mysqldump,mariadb,redis-cli",
            "sync",
        ]
    );
    assert_eq!(
        write.log,
        [
            "run de.beyondco.herd",
            "user_path",
            "helper",
            "claude mcp remove --scope user herd",
            r"claude mcp add --scope user herd -- C:\h\bin\hearth.exe mcp"
        ]
    );
    let kinds: Vec<&str> = Record::load(&paths).steps.iter().map(|s| s.kind()).collect();
    assert_eq!(
        kinds,
        [
            "herdStopped",
            "autostart",
            "imported",
            "ca",
            "ports",
            "php",
            "redis",
            "smtp",
            "node",
            "forwarders",
            "userPath",
            "machineEnv",
            "mcp"
        ]
    );
    assert!(super::state::load(&paths).moved);
}

#[test]
fn a_second_run_after_a_failure_continues_without_redoing_anything() {
    let (_d, paths, inv) = setup();
    let plan = plan(&paths, &inv, &["8.4".to_string()]);
    let mut mover = FakeMover { fail_at: Some("create_redis 6379".into()), ..FakeMover::default() };
    let mut write = FakeWrite::default();
    let mut record = Record::default();
    assert!(run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).is_err());
    assert!(!super::state::load(&paths).moved);
    let done_before = mover.calls.len();

    let mut record = Record::load(&paths);
    run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).unwrap();
    let again = &mover.calls[done_before..];
    assert_eq!(&again[..2], ["stop_herd", "create_redis 6379"], "Herd is stopped and the ports checked again");
    assert!(!again.iter().any(|c| c.starts_with("install_php") || c == "import_sites"), "{again:?}");
    assert_eq!(write.log.iter().filter(|l| l.starts_with("run ")).count(), 1, "autostart is switched off once");
}

#[test]
fn a_port_still_taken_on_the_second_run_stops_it_again() {
    let (_d, paths, inv) = setup();
    let plan = plan(&paths, &inv, &[]);
    let mut mover = FakeMover { busy: vec![443], ..FakeMover::default() };
    let mut write = FakeWrite::default();
    let mut record = Record::default();
    assert!(run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).is_err());
    let mut record = Record::load(&paths);
    let err = run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).unwrap_err();
    assert_eq!(err.to_string(), "Port 443 is still in use by nginx.exe");
    assert!(!mover.calls.iter().any(|c| c == "import_sites" || c.starts_with("set_ports")), "{:?}", mover.calls);
}

#[test]
fn a_step_that_failed_halfway_runs_again_for_what_is_missing() {
    let (_d, paths, inv) = setup();
    let plan = plan(&paths, &inv, &["8.4".to_string()]);
    let mut mover = FakeMover { fail_at: Some("install_php 8.5".into()), ..FakeMover::default() };
    let mut write = FakeWrite::default();
    let mut record = Record::default();
    assert!(run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).is_err());
    let done_before = mover.calls.len();

    let mut record = Record::load(&paths);
    run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).unwrap();
    let again = &mover.calls[done_before..];
    assert!(again.iter().any(|c| c == "install_php 8.5"), "{again:?}");
    assert!(again.iter().any(|c| c == "set_php_default 8.5"), "{again:?}");
    assert!(!again.iter().any(|c| c == "install_php 8.1"), "8.1 is there already: {again:?}");
    let record = Record::load(&paths);
    assert_eq!(record.steps.iter().filter(|s| s.kind() == "php").count(), 1);
    match record.get("php") {
        Some(Done::Php { installed, .. }) => assert_eq!(installed, &["8.1", "8.5"], "Undo still knows both"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_mcp_entry_is_added_again_when_the_add_failed() {
    let (_d, paths, inv) = setup();
    let plan = plan(&paths, &inv, &[]);
    let mut mover = FakeMover::default();
    let mut write = FakeWrite { fail_claude: Some("claude mcp add".into()), ..FakeWrite::default() };
    let mut record = Record::default();
    assert!(run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).is_err());
    let mut record = Record::load(&paths);
    run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).unwrap();
    assert_eq!(
        write.log.last().map(String::as_str),
        Some(r"claude mcp add --scope user herd -- C:\h\bin\hearth.exe mcp")
    );
    assert_eq!(Record::load(&paths).steps.iter().filter(|s| s.kind() == "mcp").count(), 1);
}

#[test]
fn a_redis_that_was_created_is_filled_on_the_next_run_not_created_twice() {
    let (_d, paths, inv) = setup();
    let plan = plan(&paths, &inv, &[]);
    let mut mover = FakeMover { fail_at: Some("fill_redis redis true".into()), ..FakeMover::default() };
    let mut write = FakeWrite::default();
    let mut record = Record::default();
    assert!(run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).is_err());
    assert_eq!(Record::load(&paths).get("redis"), Some(&Done::Redis { id: "redis".into(), created: true }));
    let mut record = Record::load(&paths);
    run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).unwrap();
    assert_eq!(mover.calls.iter().filter(|c| c.starts_with("create_redis")).count(), 1, "{:?}", mover.calls);
    assert!(mover.calls.iter().any(|c| c == "fill_redis redis true"));
}

#[test]
fn a_hearth_redis_already_on_the_port_is_used_and_kept() {
    let (_d, paths, inv) = setup();
    let plan = plan(&paths, &inv, &[]);
    let mut mover = FakeMover { redis_before: Some("mine".into()), ..FakeMover::default() };
    let mut write = FakeWrite::default();
    let mut record = Record::default();
    run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).unwrap();
    assert!(
        !mover.calls.iter().any(|c| c.starts_with("create_redis") || c.starts_with("fill_redis")),
        "its data stays"
    );
    assert_eq!(Record::load(&paths).get("redis"), Some(&Done::Redis { id: "mine".into(), created: false }));
}

#[test]
fn a_port_that_stays_taken_stops_the_move_before_anything_else() {
    let (_d, paths, inv) = setup();
    let plan = plan(&paths, &inv, &[]);
    let mut mover = FakeMover { busy: vec![443], ..FakeMover::default() };
    let mut write = FakeWrite::default();
    let mut record = Record::default();
    let err = run(&paths, &inv, &plan, &mut record, &mut mover, &mut write, &mut no_progress).unwrap_err();
    assert_eq!(err.to_string(), "Port 443 is still in use by nginx.exe");
    assert_eq!(mover.calls, ["stop_herd"]);
    assert!(write.log.is_empty());
}
