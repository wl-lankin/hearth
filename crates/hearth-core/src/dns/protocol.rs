//! What the Windows helper service accepts over its pipe: one JSON line per request, one JSON line
//! back. It only ever writes Hearth's block in the hosts file.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::dns::hosts::{apply_block, names_in_block, valid_host};

/// The pipe the helper listens on.
pub const PIPE: &str = r"\\.\pipe\hearth-helper";
/// Raised when requests or answers change, so Hearth can ask for a re-install of an older helper.
pub const PROTOCOL: u32 = 1;
/// What a client may open the pipe with: read data, write data, read attributes, synchronize.
/// Plain read/write access would also ask for rights the pipe does not grant.
pub const CLIENT_ACCESS: u32 = 0x0010_0083;
pub const MAX_REQUEST: usize = 64 * 1024;
pub const MAX_NAMES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Request {
    Status,
    Hosts { names: Vec<String> },
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

/// Answers one request line against the hosts file at `hosts`.
pub fn handle(line: &[u8], hosts: &Path) -> Response {
    if line.len() > MAX_REQUEST {
        return Response::error("Request too large");
    }
    let Ok(request) = serde_json::from_slice::<Request>(line) else {
        return Response::error("Unknown request");
    };
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
