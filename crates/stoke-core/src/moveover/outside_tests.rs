use super::outside::{apply_outside, undo_outside, MachineWrite};
use super::record::{Done, Record};
use crate::dns::protocol::Request;
use crate::CoreError;

#[derive(Default)]
struct FakeWrite {
    user_path: String,
    /// Reading the user PATH fails.
    unreadable: bool,
    /// `claude mcp remove` fails (no such entry).
    no_entry: bool,
    run: Vec<(String, String)>,
    claude: Vec<Vec<String>>,
    helper: Vec<Request>,
}

impl MachineWrite for FakeWrite {
    fn user_path(&self) -> Result<String, CoreError> {
        if self.unreadable {
            return Err(CoreError::Php("The user PATH could not be read".into()));
        }
        Ok(self.user_path.clone())
    }
    fn set_user_path(&mut self, value: &str) -> Result<(), CoreError> {
        self.user_path = value.into();
        Ok(())
    }
    fn set_run(&mut self, name: &str, command: Option<&str>) -> Result<(), CoreError> {
        self.run.retain(|(n, _)| n != name);
        if let Some(c) = command {
            self.run.push((name.into(), c.into()));
        }
        Ok(())
    }
    fn claude(&mut self, args: &[String]) -> Result<(), CoreError> {
        if self.no_entry && args[1] == "remove" {
            return Err(CoreError::Php("No MCP server named herd".into()));
        }
        self.claude.push(args.to_vec());
        Ok(())
    }
    fn helper(&mut self, request: &Request) -> Result<(), CoreError> {
        self.helper.push(request.clone());
        Ok(())
    }
}

const HERD_BIN: &str = r"C:\Users\x\.config\herd\bin";
const HERD_REDIS: &str = r"C:\Users\x\.config\herd\bin\services\redis\7.0.0";
const STOKE_BIN: &str = r"C:\Users\x\.config\stoke\bin";

#[test]
fn the_user_path_loses_herds_entries_and_gets_them_back_exactly() {
    let before = format!(r"C:\a;{HERD_BIN};C:\b;{HERD_REDIS}");
    let mut m = FakeWrite { user_path: before.clone(), ..FakeWrite::default() };
    let step = Done::UserPath {
        removed: vec![HERD_BIN.into(), HERD_REDIS.into()],
        added: STOKE_BIN.into(),
        before: before.clone(),
    };
    apply_outside(&mut m, &step).unwrap();
    assert_eq!(m.user_path, format!(r"{STOKE_BIN};C:\a;C:\b"));
    undo_outside(&mut m, &step).unwrap();
    assert_eq!(m.user_path, before, "nothing else changed: the exact old value");

    // Changed in between: only Stoke's entry goes and Herd's come back at their places; the other
    // change stays.
    apply_outside(&mut m, &step).unwrap();
    m.user_path.push_str(r";C:\new");
    undo_outside(&mut m, &step).unwrap();
    assert_eq!(m.user_path, format!(r"C:\a;{HERD_BIN};C:\b;{HERD_REDIS};C:\new"));
}

#[test]
fn undo_keeps_stokes_entry_when_it_was_there_before_the_move() {
    let before = format!(r"{STOKE_BIN};C:\a;{HERD_BIN}");
    let mut m = FakeWrite { user_path: before.clone(), ..FakeWrite::default() };
    let step = Done::UserPath { removed: vec![HERD_BIN.into()], added: STOKE_BIN.into(), before };
    apply_outside(&mut m, &step).unwrap();
    m.user_path.push_str(r";C:\new");
    undo_outside(&mut m, &step).unwrap();
    assert_eq!(m.user_path, format!(r"{STOKE_BIN};C:\a;{HERD_BIN};C:\new"));
}

#[test]
fn an_unreadable_user_path_is_never_overwritten() {
    let mut m = FakeWrite { user_path: r"C:\a".into(), unreadable: true, ..FakeWrite::default() };
    let step = Done::UserPath { removed: vec![HERD_BIN.into()], added: STOKE_BIN.into(), before: r"C:\a".into() };
    assert!(apply_outside(&mut m, &step).is_err());
    assert!(undo_outside(&mut m, &step).is_err());
    assert_eq!(m.user_path, r"C:\a");
}

#[test]
fn the_mcp_swap_runs_again_when_herds_entry_is_gone_already() {
    let mut m = FakeWrite { no_entry: true, ..FakeWrite::default() };
    let step = Done::Mcp { herd_entry: serde_json::json!({}), stoke: "stoke.exe".into() };
    apply_outside(&mut m, &step).unwrap();
    assert_eq!(m.claude.last().map(|a| a[1].as_str()), Some("add"));
}

#[test]
fn herds_autostart_goes_and_comes_back() {
    let mut m = FakeWrite { run: vec![("de.beyondco.herd".into(), "Herd.exe".into())], ..FakeWrite::default() };
    let step = Done::Autostart { name: "de.beyondco.herd".into(), value: "Herd.exe".into() };
    apply_outside(&mut m, &step).unwrap();
    assert!(m.run.is_empty());
    undo_outside(&mut m, &step).unwrap();
    assert_eq!(m.run, [("de.beyondco.herd".to_string(), "Herd.exe".to_string())]);
}

#[test]
fn the_mcp_entry_herd_points_at_stoke_and_back() {
    let mut m = FakeWrite::default();
    let entry = serde_json::json!({ "type": "stdio", "command": "cmd", "args": ["/c", "herd", "mcp"] });
    let step = Done::Mcp { herd_entry: entry.clone(), stoke: r"C:\Users\x\.config\stoke\bin\stoke.exe".into() };
    apply_outside(&mut m, &step).unwrap();
    undo_outside(&mut m, &step).unwrap();
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    assert_eq!(
        m.claude,
        [
            s(&["mcp", "remove", "--scope", "user", "herd"]),
            s(&["mcp", "add", "--scope", "user", "herd", "--", r"C:\Users\x\.config\stoke\bin\stoke.exe", "mcp"]),
            s(&["mcp", "remove", "--scope", "user", "herd"]),
            vec!["mcp".into(), "add-json".into(), "--scope".into(), "user".into(), "herd".into(), entry.to_string()],
        ]
    );
}

#[test]
fn herds_node_in_the_machine_environment_goes_through_the_helper() {
    let mut m = FakeWrite::default();
    let nvm = r"C:\Users\x\.config\herd\bin\nvm".to_string();
    let step = Done::MachineEnv {
        path_removed: vec![nvm.clone(), r"C:\Program Files\nodejs".into()],
        vars_removed: vec![("NVM_HOME".into(), nvm.clone())],
    };
    apply_outside(&mut m, &step).unwrap();
    undo_outside(&mut m, &step).unwrap();
    assert_eq!(
        m.helper,
        [
            Request::MachineEnv {
                remove_path: vec![nvm.clone(), r"C:\Program Files\nodejs".into()],
                remove_vars: vec!["NVM_HOME".into()],
            },
            // Undo names nothing: the helper restores what it recorded.
            Request::MachineEnvRestore,
        ]
    );
}

#[test]
fn the_record_survives_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let paths = crate::StokePaths::at(d.path().join("stoke"));
    paths.ensure().unwrap();
    assert_eq!(Record::load(&paths), Record::default());
    let mut record = Record::default();
    record.push(&paths, Done::Smtp { previous: 2525 }).unwrap();
    record.push(&paths, Done::Ports { http: 8080, https: 8443 }).unwrap();
    let again = Record::load(&paths);
    assert_eq!(again, record);
    assert!(again.has("ports"));
    assert!(!again.has("redis"));
}
