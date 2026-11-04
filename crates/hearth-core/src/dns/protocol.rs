//! What the Windows helper service accepts over its pipe: one JSON line per request, one JSON line
//! back. It only ever writes Hearth's block in the hosts file, and, for the move from Herd, Herd's Node
//! entries in the machine environment (nothing else: see `herds_entry`).

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dns::hosts::{apply_block, names_in_block, valid_host};

/// The pipe the helper listens on.
pub const PIPE: &str = r"\\.\pipe\hearth-helper";
/// Raised when requests or answers change, so Hearth can ask for a re-install of an older helper.
pub const PROTOCOL: u32 = 2;
/// What a client may open the pipe with: read data, write data, read attributes, synchronize.
/// Plain read/write access would also ask for rights the pipe does not grant.
pub const CLIENT_ACCESS: u32 = 0x0010_0083;
pub const MAX_REQUEST: usize = 64 * 1024;
pub const MAX_NAMES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Request {
    Status,
    Hosts {
        names: Vec<String>,
    },
    /// Takes Herd's Node manager out of the machine environment: these PATH entries and variables,
    /// remembered by the helper itself.
    #[serde(rename_all = "camelCase")]
    MachineEnv {
        #[serde(default)]
        remove_path: Vec<String>,
        #[serde(default)]
        remove_vars: Vec<String>,
    },
    /// Puts back exactly what `MachineEnv` took out (the client names nothing: nothing to inject).
    MachineEnvRestore,
}

/// What the helper took out of the machine environment, kept where only administrators can write.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub path: Vec<String>,
    pub vars: Vec<(String, String)>,
}

/// The machine environment, as the helper changes it (the real one is the registry).
pub trait MachineEnv {
    fn path(&self) -> String;
    fn set_path(&mut self, value: &str) -> Result<(), String>;
    fn var(&self, name: &str) -> Option<String>;
    fn set_var(&mut self, name: &str, value: Option<&str>) -> Result<(), String>;
    fn removed(&self) -> Removed;
    fn set_removed(&mut self, removed: &Removed) -> Result<(), String>;
}

/// The only variables the helper takes out.
const HERD_VARS: [&str; 2] = ["NVM_HOME", "NVM_SYMLINK"];

/// A machine PATH entry the helper may take out: in a user's Herd folder
/// (`X:\Users\<name>\.config\herd` or below), or Herd's Node link `C:\Program Files\nodejs`. Anchored,
/// no network paths, no `%`, `..`, `;`, quotes or control characters.
pub fn herds_entry(value: &str) -> bool {
    let v = value.trim().trim_end_matches('\\');
    if v.starts_with(r"\\") || v.contains("..") || v.contains(|c: char| c.is_control() || matches!(c, '%' | ';' | '"'))
    {
        return false;
    }
    let lower = v.to_ascii_lowercase();
    if lower == r"c:\program files\nodejs" {
        return true;
    }
    let mut parts = lower.split('\\');
    let drive = parts.next().unwrap_or_default();
    let drive_ok = drive.len() == 2 && drive.as_bytes()[0].is_ascii_alphabetic() && drive.ends_with(':');
    let (users, name, config, herd) = (parts.next(), parts.next(), parts.next(), parts.next());
    drive_ok
        && users == Some("users")
        && name.is_some_and(|n| !n.is_empty())
        && config == Some(".config")
        && herd == Some("herd")
}

/// Takes Herd's entries out, recording first what was there.
fn take_out(env: &mut dyn MachineEnv, remove_path: &[String], remove_vars: &[String]) -> Response {
    let bad_entry = remove_path.iter().find(|e| !herds_entry(e));
    let bad_var = remove_vars.iter().find(|n| !HERD_VARS.contains(&n.as_str()));
    if let Some(bad) = bad_entry.or(bad_var) {
        return Response::error(format!("Not one of Herd's entries: {bad:?}"));
    }
    let current = env.path();
    let present = |e: &String| current.split(';').any(|c| c.eq_ignore_ascii_case(e));
    let mut removed = env.removed();
    for entry in remove_path.iter().filter(|e| present(e)) {
        if !removed.path.iter().any(|r| r.eq_ignore_ascii_case(entry)) {
            removed.path.push(entry.clone());
        }
    }
    for name in remove_vars {
        if let Some(value) = env.var(name) {
            if !removed.vars.iter().any(|(n, _)| n == name) {
                removed.vars.push((name.clone(), value));
            }
        }
    }
    // Recorded before anything changes, so a crash halfway can still be restored.
    if let Err(e) = env.set_removed(&removed) {
        return Response::error(e);
    }
    let next: Vec<&str> = current
        .split(';')
        .filter(|c| !c.is_empty() && !remove_path.iter().any(|r| r.eq_ignore_ascii_case(c)))
        .collect();
    let next = next.join(";");
    if next != current {
        if let Err(e) = env.set_path(&next) {
            return Response::error(e);
        }
    }
    for name in remove_vars {
        if let Err(e) = env.set_var(name, None) {
            return Response::error(e);
        }
    }
    Response::default()
}

/// Puts back what `take_out` recorded, then forgets it.
fn put_back(env: &mut dyn MachineEnv) -> Response {
    let removed = env.removed();
    let current = env.path();
    let mut entries: Vec<String> = current.split(';').filter(|c| !c.is_empty()).map(str::to_string).collect();
    for entry in &removed.path {
        if !entries.iter().any(|e| e.eq_ignore_ascii_case(entry)) {
            entries.push(entry.clone());
        }
    }
    let next = entries.join(";");
    if next != current {
        if let Err(e) = env.set_path(&next) {
            return Response::error(e);
        }
    }
    for (name, value) in &removed.vars {
        if let Err(e) = env.set_var(name, Some(value)) {
            return Response::error(e);
        }
    }
    match env.set_removed(&Removed::default()) {
        Ok(()) => Response::default(),
        Err(e) => Response::error(e),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    fn error(message: impl Into<String>) -> Response {
        Response { error: Some(message.into()), ..Response::default() }
    }
}

/// Answers one request line against the hosts file at `hosts` (machine environment requests are
/// refused here: they need `handle_with` and the helper's own record).
pub fn handle(line: &[u8], hosts: &Path) -> Response {
    handle_with(line, hosts, &mut super::machine_env::NoMachineEnv)
}

/// Answers one request line against the hosts file at `hosts` and the environment `env`.
pub fn handle_with(line: &[u8], hosts: &Path, env: &mut dyn MachineEnv) -> Response {
    if line.len() > MAX_REQUEST {
        return Response::error("Request too large");
    }
    let Ok(request) = serde_json::from_slice::<Request>(line) else {
        return Response::error("Unknown request");
    };
    match &request {
        Request::MachineEnv { remove_path, remove_vars } => return take_out(env, remove_path, remove_vars),
        Request::MachineEnvRestore => return put_back(env),
        _ => {}
    }
    let current = match std::fs::read(hosts) {
        Ok(bytes) => bytes,
        Err(e) => return Response::error(format!("Could not read the hosts file: {e}")),
    };
    match request {
        Request::Status => Response {
            protocol: Some(PROTOCOL),
            names: Some(names_in_block(&current).into_iter().collect()),
            error: None,
        },
        Request::MachineEnv { .. } | Request::MachineEnvRestore => Response::default(),
        Request::Hosts { names } => {
            if names.len() > MAX_NAMES {
                return Response::error("Too many names");
            }
            if let Some(bad) = names.iter().find(|n| !valid_host(n)) {
                return Response::error(format!("Not a .test site name: {bad:?}"));
            }
            let names: BTreeSet<String> = names.into_iter().collect();
            let updated = apply_block(&current, &names);
            if updated == current {
                return Response::default();
            }
            // In place, not replaced: the file keeps its permissions and identity.
            match std::fs::write(hosts, updated) {
                Ok(()) => Response::default(),
                Err(e) => Response::error(format!("Could not write the hosts file: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn hosts_file(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("hosts");
        std::fs::write(&p, content).unwrap();
        (d, p)
    }

    const BASE: &str = "127.0.0.1 localhost\r\n#herd\r\n127.0.0.1 shop.test\r\n#/herd\r\n";

    #[test]
    fn a_hosts_request_writes_the_block() {
        let (_d, p) = hosts_file(BASE);
        let answer = handle(br#"{"op":"hosts","names":["b.test","a.test"]}"#, &p);
        assert_eq!(answer, Response::default());
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text, format!("{BASE}# hearth-start\r\n127.0.0.1 a.test\r\n127.0.0.1 b.test\r\n# hearth-end\r\n"));
    }

    #[test]
    fn one_invalid_name_refuses_the_whole_request() {
        let (_d, p) = hosts_file(BASE);
        let answer = handle(br#"{"op":"hosts","names":["a.test","a.test\n1.2.3.4 bank.com"]}"#, &p);
        assert_eq!(answer.error.as_deref(), Some("Not a .test site name: \"a.test\\n1.2.3.4 bank.com\""));
        assert_eq!(std::fs::read(&p).unwrap(), BASE.as_bytes());
        let answer = handle(br#"{"op":"hosts","names":["bank.com"]}"#, &p);
        assert!(answer.error.is_some());
        assert_eq!(std::fs::read(&p).unwrap(), BASE.as_bytes());
    }

    #[test]
    fn status_reports_the_protocol_and_the_names_in_the_block() {
        let (_d, p) = hosts_file(BASE);
        handle(br#"{"op":"hosts","names":["a.test"]}"#, &p);
        let answer = handle(br#"{"op":"status"}"#, &p);
        assert_eq!(answer, Response { protocol: Some(PROTOCOL), names: Some(vec!["a.test".into()]), error: None });
    }

    #[test]
    fn oversized_or_unknown_requests_are_refused() {
        let (_d, p) = hosts_file(BASE);
        let big = vec![b' '; MAX_REQUEST + 1];
        assert_eq!(handle(&big, &p).error.as_deref(), Some("Request too large"));
        let many: Vec<String> = (0..=MAX_NAMES).map(|i| format!("s{i}.test")).collect();
        let line = serde_json::to_vec(&Request::Hosts { names: many }).unwrap();
        assert_eq!(handle(&line, &p).error.as_deref(), Some("Too many names"));
        assert_eq!(handle(br#"{"op":"delete","path":"C:\\x"}"#, &p).error.as_deref(), Some("Unknown request"));
        assert_eq!(std::fs::read(&p).unwrap(), BASE.as_bytes());
    }

    #[derive(Default)]
    struct FakeEnv {
        path: String,
        vars: BTreeMap<String, String>,
        removed: Removed,
    }

    impl MachineEnv for FakeEnv {
        fn path(&self) -> String {
            self.path.clone()
        }
        fn set_path(&mut self, value: &str) -> Result<(), String> {
            self.path = value.into();
            Ok(())
        }
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).cloned()
        }
        fn set_var(&mut self, name: &str, value: Option<&str>) -> Result<(), String> {
            match value {
                Some(v) => self.vars.insert(name.into(), v.into()),
                None => self.vars.remove(name),
            };
            Ok(())
        }
        fn removed(&self) -> Removed {
            self.removed.clone()
        }
        fn set_removed(&mut self, removed: &Removed) -> Result<(), String> {
            self.removed = removed.clone();
            Ok(())
        }
    }

    const NVM: &str = r"C:\Users\x\.config\herd\bin\nvm";

    #[test]
    fn herds_node_entries_go_and_only_the_recorded_ones_come_back() {
        let (_d, p) = hosts_file(BASE);
        let mut env =
            FakeEnv { path: format!(r"C:\Windows\System32;{NVM};C:\Program Files\nodejs"), ..FakeEnv::default() };
        env.vars.insert("NVM_HOME".into(), NVM.into());
        let out = serde_json::json!({
            "op": "machineenv", "removePath": [NVM, r"C:\Program Files\nodejs", r"C:\Users\x\.config\herd\absent"],
            "removeVars": ["NVM_HOME", "NVM_SYMLINK"]
        });
        assert_eq!(handle_with(out.to_string().as_bytes(), &p, &mut env), Response::default());
        assert_eq!(env.path, r"C:\Windows\System32");
        assert!(env.vars.is_empty());
        assert_eq!(env.removed.path, [NVM, r"C:\Program Files\nodejs"], "only what was there is recorded");
        assert_eq!(env.removed.vars, [("NVM_HOME".to_string(), NVM.to_string())]);

        assert_eq!(handle_with(br#"{"op":"machineenvrestore"}"#, &p, &mut env), Response::default());
        assert_eq!(env.path, format!(r"C:\Windows\System32;{NVM};C:\Program Files\nodejs"));
        assert_eq!(env.vars.get("NVM_HOME").map(String::as_str), Some(NVM));
        assert_eq!(env.removed, Removed::default(), "the record is forgotten");
        // A second restore has nothing to put back.
        assert_eq!(handle_with(br#"{"op":"machineenvrestore"}"#, &p, &mut env), Response::default());
        assert_eq!(env.path, format!(r"C:\Windows\System32;{NVM};C:\Program Files\nodejs"));
    }

    #[test]
    fn nothing_but_herds_own_entries_can_be_taken_out_and_nothing_can_be_put_in() {
        let (_d, p) = hosts_file(BASE);
        let mut env = FakeEnv { path: r"C:\Windows\System32;C:\Tools".into(), ..FakeEnv::default() };
        for bad in [
            r"C:\Windows\System32",
            r"C:\Tools\.config\herd",
            r"\\server\share\Users\x\.config\herd",
            r"C:\Users\x\.config\herd\..\..\Windows",
            r"%USERPROFILE%\.config\herd",
            r"C:\Users\\.config\herd",
        ] {
            let line = serde_json::json!({ "op": "machineenv", "removePath": [bad] }).to_string();
            let answer = handle_with(line.as_bytes(), &p, &mut env);
            assert!(answer.error.as_deref().is_some_and(|e| e.starts_with("Not one of Herd's")), "{bad}: {answer:?}");
        }
        let line = serde_json::json!({ "op": "machineenv", "removeVars": ["PATHEXT"] }).to_string();
        assert!(handle_with(line.as_bytes(), &p, &mut env).error.is_some());
        // The old shape that named values to add is no request at all.
        let add = serde_json::json!({ "op": "machineenv", "addPath": [r"C:\Users\x\.config\herd\evil"] }).to_string();
        assert_eq!(handle_with(add.as_bytes(), &p, &mut env), Response::default(), "unknown fields add nothing");
        assert_eq!(env.path, r"C:\Windows\System32;C:\Tools");
        assert_eq!(env.removed, Removed::default());
        // Without the helper's own record, a plain `handle` refuses environment changes.
        let line = serde_json::json!({ "op": "machineenv", "removePath": [NVM] }).to_string();
        assert!(handle(line.as_bytes(), &p).error.is_some());
    }

    #[test]
    fn an_unchanged_block_is_not_written_again() {
        let (_d, p) = hosts_file(BASE);
        handle(br#"{"op":"hosts","names":["a.test"]}"#, &p);
        let before = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        handle(br#"{"op":"hosts","names":["a.test"]}"#, &p);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), before);
    }
}
