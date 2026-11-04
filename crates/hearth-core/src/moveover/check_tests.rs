use std::path::PathBuf;

use super::check::{compare, gate, SiteCheck, Verdict};
use super::outside::MachineWrite;
use super::probe::Answer;
use super::record::{Done, Record};
use super::state::{self, MoveState};
use super::undo::{undo, Reverter};
use super::uninstall::{uninstall, Uninstaller};
use crate::dns::protocol::Request;
use crate::herd::import::ImportPlan;
use crate::{CoreError, HearthPaths};

fn answer(status: Option<u16>, location: Option<&str>, error: Option<&str>) -> Answer {
    Answer {
        host: "shop.test".into(),
        scheme: "http".into(),
        status,
        location: location.map(str::to_string),
        error: error.map(str::to_string),
    }
}

#[test]
fn answers_compare_by_status_and_redirect() {
    let ok = answer(Some(200), None, None);
    let login = answer(Some(302), Some("/login"), None);
    let home = answer(Some(302), Some("/"), None);
    let down = answer(None, None, Some("connection refused"));
    assert_eq!(compare(&ok, &ok), Verdict::Same);
    assert_eq!(compare(&login, &login), Verdict::Same);
    assert!(matches!(compare(&login, &home), Verdict::Changed(_)));
    assert!(matches!(compare(&ok, &down), Verdict::Broken(_)));
    assert_eq!(compare(&down, &down), Verdict::Same, "it failed under Herd too");
    assert_eq!(compare(&down, &ok), Verdict::Better);
    assert_eq!(
        compare(&answer(Some(500), None, None), &answer(Some(503), None, None)),
        Verdict::Same,
        "same status class"
    );
}

#[test]
fn the_uninstall_waits_for_every_red_row_to_be_accepted() {
    let checks = vec![
        SiteCheck { host: "shop.test".into(), scheme: "http".into(), verdict: Verdict::Same },
        SiteCheck {
            host: "blog.test".into(),
            scheme: "https".into(),
            verdict: Verdict::Changed("302 / instead of 200".into()),
        },
    ];
    let err = gate(&checks, &[]).unwrap_err();
    assert!(err.contains("https://blog.test"), "{err}");
    assert!(gate(&checks, &["https://blog.test".to_string()]).is_ok());
}

/// Logs every call; a call whose line starts with the second field fails once.
#[derive(Default)]
struct Log(Vec<String>, Option<String>);

impl Log {
    fn note(&mut self, line: String) -> Result<(), CoreError> {
        if self.1.as_deref().is_some_and(|f| line.starts_with(f)) {
            self.1 = None;
            return Err(CoreError::Php(format!("{line} failed")));
        }
        self.0.push(line);
        Ok(())
    }
}

impl Reverter for Log {
    fn start_herd(&mut self, app: &str) -> Result<(), CoreError> {
        self.note(format!("start_herd {app}"))
    }
    fn remove_sites(&mut self, _: &ImportPlan) -> Result<(), CoreError> {
        self.note("remove_sites".into())
    }
    fn restore_ca(&mut self, previous: Option<&(Vec<u8>, Vec<u8>)>) -> Result<(), CoreError> {
        self.note(format!("restore_ca {}", previous.is_some()))
    }
    fn set_ports(&mut self, http: u16, https: u16) -> Result<(), CoreError> {
        self.note(format!("set_ports {http} {https}"))
    }
    fn uninstall_php(&mut self, minor: &str) -> Result<(), CoreError> {
        self.note(format!("uninstall_php {minor}"))
    }
    fn set_php_default(&mut self, minor: Option<&str>) -> Result<(), CoreError> {
        self.note(format!("set_php_default {minor:?}"))
    }
    fn set_ini_user(&mut self, minor: &str, user: &str) -> Result<(), CoreError> {
        self.note(format!("set_ini_user {minor} {}", user.trim()))
    }
    fn delete_service(&mut self, id: &str) -> Result<(), CoreError> {
        self.note(format!("delete_service {id}"))
    }
    fn set_smtp(&mut self, port: u16) -> Result<(), CoreError> {
        self.note(format!("set_smtp {port}"))
    }
    fn uninstall_node(&mut self, version: &str) -> Result<(), CoreError> {
        self.note(format!("uninstall_node {version}"))
    }
    fn set_node_default(&mut self, version: Option<&str>) -> Result<(), CoreError> {
        self.note(format!("set_node_default {version:?}"))
    }
    fn remove_forwarders(&mut self) -> Result<(), CoreError> {
        self.note("remove_forwarders".into())
    }
    fn sync(&mut self) {
        self.0.push("sync".into());
    }
}

impl MachineWrite for Log {
    fn user_path(&self) -> Result<String, CoreError> {
        Ok(String::new())
    }
    fn set_user_path(&mut self, value: &str) -> Result<(), CoreError> {
        self.note(format!("user_path {value}"))
    }
    fn set_run(&mut self, name: &str, command: Option<&str>) -> Result<(), CoreError> {
        self.note(format!("run {name} {}", command.is_some()))
    }
    fn claude(&mut self, args: &[String]) -> Result<(), CoreError> {
        self.note(format!("claude {}", args[1]))
    }
    fn helper(&mut self, _: &Request) -> Result<(), CoreError> {
        self.note("helper restore".into())
    }
}

fn home() -> (tempfile::TempDir, HearthPaths) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, paths)
}

#[test]
fn undo_walks_the_record_backwards_and_forgets_it() {
    let (_d, paths) = home();
    let mut record = Record::default();
    for step in [
        Done::HerdStopped { app: Some(r"C:\Program Files\Herd\Herd.exe".into()) },
        Done::Autostart { name: "de.beyondco.herd".into(), value: "Herd.exe".into() },
        Done::Imported { plan: ImportPlan::default() },
        Done::Ca { previous: Some((b"crt".to_vec(), b"key".to_vec())) },
        Done::Ports { http: 8080, https: 8443 },
        Done::Php {
            installed: vec!["8.1".into()],
            default_before: Some("8.4".into()),
            ini_before: [("8.5".to_string(), "short_open_tag = On\n".to_string())].into(),
        },
        Done::Redis { id: "redis".into(), created: true },
        Done::Smtp { previous: 2526 },
        Done::Node { installed: vec!["v22.9.0".into()], default_before: None },
        Done::Forwarders { names: vec!["mysql".into()] },
        Done::UserPath { removed: vec!["herd".into()], added: "hearth".into(), before: "herd".into() },
        Done::MachineEnv { path_removed: vec![], vars_removed: vec![] },
        Done::Mcp { herd_entry: serde_json::json!({}), hearth: "hearth.exe".into() },
    ] {
        record.push(&paths, step).unwrap();
    }
    state::save(&paths, &MoveState { moved: true, ..MoveState::default() }).unwrap();
    let mut log = Log::default();
    let mut outside = Log::default();
    undo(&paths, &mut log, &mut outside).unwrap();
    assert_eq!(
        outside.0,
        ["claude remove", "claude add-json", "helper restore", "user_path herd", "run de.beyondco.herd true"]
    );
    assert_eq!(
        log.0,
        [
            "remove_forwarders",
            "uninstall_node v22.9.0",
            "set_node_default None",
            "set_smtp 2526",
            "delete_service redis",
            "set_ini_user 8.5 short_open_tag = On",
            "set_php_default Some(\"8.4\")",
            "uninstall_php 8.1",
            "set_ports 8080 8443",
            "restore_ca true",
            "remove_sites",
            r"start_herd C:\Program Files\Herd\Herd.exe",
            "sync",
        ]
    );
    assert_eq!(Record::load(&paths), Record::default(), "the record is forgotten");
    assert!(!state::load(&paths).moved);
}

#[test]
fn undo_is_refused_once_herd_is_uninstalled() {
    let (_d, paths) = home();
    state::save(&paths, &MoveState { moved: true, uninstalled: true, ..MoveState::default() }).unwrap();
    let err = undo(&paths, &mut Log::default(), &mut Log::default()).unwrap_err();
    assert_eq!(err.to_string(), "Herd is uninstalled: there is nothing to go back to");
}

struct FakeUninstaller {
    exit: i32,
    trusted_after: bool,
    trusted_again: bool,
}

impl Uninstaller for FakeUninstaller {
    fn run(&mut self, program: &std::path::Path, args: &[String]) -> Result<i32, CoreError> {
        assert!(program.ends_with("Uninstall Herd.exe"));
        assert_eq!(args, ["/allusers"]);
        Ok(self.exit)
    }
    fn ca_trusted(&self) -> bool {
        self.trusted_after || self.trusted_again
    }
    fn trust(&mut self) -> Result<(), CoreError> {
        self.trusted_again = true;
        Ok(())
    }
}

#[test]
fn the_uninstall_marks_herd_gone_and_brings_trust_back() {
    let (_d, paths) = home();
    let checks = vec![SiteCheck { host: "shop.test".into(), scheme: "http".into(), verdict: Verdict::Same }];
    state::save(&paths, &MoveState { moved: true, ..MoveState::default() }).unwrap();
    let program = PathBuf::from(r"C:\Program Files\Herd\Uninstall Herd.exe");
    let args = vec!["/allusers".to_string()];

    let mut cancelled = FakeUninstaller { exit: 1, trusted_after: true, trusted_again: false };
    assert_eq!(uninstall(&paths, &checks, &program, &args, &mut cancelled).unwrap(), 1);
    assert!(!state::load(&paths).uninstalled, "a cancelled uninstall leaves Herd, and Undo");

    let mut done = FakeUninstaller { exit: 0, trusted_after: false, trusted_again: false };
    assert_eq!(uninstall(&paths, &checks, &program, &args, &mut done).unwrap(), 0);
    assert!(state::load(&paths).uninstalled);
    assert!(done.trusted_again, "the uninstaller took the CA out of the trust store; Hearth puts it back");

    let red = vec![SiteCheck {
        host: "shop.test".into(),
        scheme: "http".into(),
        verdict: Verdict::Broken("no answer".into()),
    }];
    let mut blocked = FakeUninstaller { exit: 0, trusted_after: true, trusted_again: false };
    assert!(uninstall(&paths, &red, &program, &args, &mut blocked).is_err());
}

/// The record holds the CA key from before the move (for Undo): only the user may read it.
#[cfg(unix)]
#[test]
fn the_record_with_the_previous_ca_key_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, paths) = home();
    let mut record = Record::default();
    record.push(&paths, Done::Ca { previous: Some((b"crt".to_vec(), b"key".to_vec())) }).unwrap();
    let mode = std::fs::metadata(paths.config.join("moveover-record.json")).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_failed_undo_continues_where_it_stopped() {
    let (_d, paths) = home();
    let mut record = Record::default();
    for step in [
        Done::Imported { plan: ImportPlan::default() },
        Done::Ports { http: 8080, https: 8443 },
        Done::Smtp { previous: 2526 },
    ] {
        record.push(&paths, step).unwrap();
    }
    state::save(&paths, &MoveState { moved: true, ..MoveState::default() }).unwrap();
    let mut log = Log(vec![], Some("set_ports".into()));
    assert!(undo(&paths, &mut log, &mut Log::default()).is_err());
    let kinds: Vec<&str> = Record::load(&paths).steps.iter().map(|s| s.kind()).collect();
    assert_eq!(kinds, ["imported", "ports"], "what was put back is out of the record");
    undo(&paths, &mut log, &mut Log::default()).unwrap();
    assert_eq!(log.0, ["set_smtp 2526", "set_ports 8080 8443", "remove_sites", "sync"], "nothing twice");
    assert!(!state::load(&paths).moved);
}

#[test]
fn undo_keeps_a_redis_that_was_there_before_the_move() {
    let (_d, paths) = home();
    let mut record = Record::default();
    record.push(&paths, Done::Redis { id: "mine".into(), created: false }).unwrap();
    state::save(&paths, &MoveState { moved: true, ..MoveState::default() }).unwrap();
    let mut log = Log::default();
    undo(&paths, &mut log, &mut Log::default()).unwrap();
    assert_eq!(log.0, ["sync"]);
}
