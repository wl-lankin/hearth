//! The SMTP catcher: takes every mail sent to it on 127.0.0.1 and keeps it. Any login works, and
//! the username names the app the mail came from (`MAIL_USERNAME`, as in Herd). Nothing is relayed.

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use crate::mail::store::{self, Envelope};
use crate::{atomic, CoreError, HearthPaths};

/// Herd's port, so apps set up for Herd send here too.
pub const DEFAULT_PORT: u16 = 2525;
/// The largest mail taken (25 MB).
pub const MAX_SIZE: usize = 25 * 1024 * 1024;
/// The longest command line.
const LINE_MAX: usize = 4096;
/// A client silent this long is dropped.
const IDLE: Duration = Duration::from_secs(300);

#[derive(Serialize, Deserialize)]
struct Config {
    port: u16,
}

fn config_file(paths: &HearthPaths) -> std::path::PathBuf {
    paths.config.join("mail.json")
}

/// The port from `config/mail.json`, 2525 when it is missing or unreadable.
pub fn configured_port(paths: &HearthPaths) -> u16 {
    std::fs::read(config_file(paths))
        .ok()
        .and_then(|b| serde_json::from_slice::<Config>(&b).ok())
        .map(|c| c.port)
        .filter(|p| *p > 0)
        .unwrap_or(DEFAULT_PORT)
}

pub fn set_configured_port(paths: &HearthPaths, port: u16) -> Result<(), CoreError> {
    atomic::write(&config_file(paths), &serde_json::to_vec(&Config { port }).unwrap_or_default())
}

/// Takes mails on `listener` until the task is dropped.
pub async fn serve(listener: TcpListener, paths: HearthPaths) {
    serve_limited(listener, paths, MAX_SIZE).await
}

/// `serve` with another size limit (tests).
pub(crate) async fn serve_limited(listener: TcpListener, paths: HearthPaths, max: usize) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        tokio::spawn(session(stream, paths.clone(), max));
    }
}

enum Line {
    Text(String),
    TooLong,
    Closed,
}

struct Conn {
    r: BufReader<OwnedReadHalf>,
    w: OwnedWriteHalf,
}

impl Conn {
    async fn say(&mut self, reply: &str) -> bool {
        self.w.write_all(format!("{reply}\r\n").as_bytes()).await.is_ok()
    }

    /// One command line without its line end; a longer one than `LINE_MAX` is read to its end and
    /// reported as too long.
    async fn line(&mut self) -> Line {
        let mut buf = Vec::new();
        let read = tokio::time::timeout(IDLE, (&mut self.r).take(LINE_MAX as u64).read_until(b'\n', &mut buf)).await;
        match read {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return Line::Closed,
            Ok(Ok(_)) => {}
        }
        if !buf.ends_with(b"\n") {
            loop {
                let mut rest = Vec::new();
                match tokio::time::timeout(IDLE, (&mut self.r).take(LINE_MAX as u64).read_until(b'\n', &mut rest)).await
                {
                    Ok(Ok(n)) if n > 0 => {
                        if rest.ends_with(b"\n") {
                            return Line::TooLong;
                        }
                    }
                    _ => return Line::Closed,
                }
            }
        }
        let text = String::from_utf8_lossy(&buf);
        Line::Text(text.trim_end_matches(['\r', '\n']).to_string())
    }

    /// The mail after DATA, dot-unstuffed, up to its final `.` line. `Ok(None)` when it was over
    /// `max` (read to the end, kept nowhere); `Err` when the client went away.
    async fn data(&mut self, max: usize) -> Result<Option<Vec<u8>>, ()> {
        let mut data = Vec::new();
        let mut too_big = false;
        let mut at_start = true;
        loop {
            let mut chunk = Vec::new();
            match tokio::time::timeout(IDLE, (&mut self.r).take(65536).read_until(b'\n', &mut chunk)).await {
                Ok(Ok(n)) if n > 0 => {}
                _ => return Err(()),
            }
            let complete = chunk.ends_with(b"\n");
            if at_start {
                if chunk == b".\r\n" || chunk == b".\n" {
                    break;
                }
                if chunk.starts_with(b".") {
                    chunk.remove(0);
                }
            }
            at_start = complete;
            if !too_big {
                if data.len() + chunk.len() > max {
                    too_big = true;
                    data = Vec::new();
                } else {
                    data.extend_from_slice(&chunk);
                }
            }
        }
        Ok((!too_big).then_some(data))
    }
}

fn decode(b64: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()
}

/// The username from a PLAIN response (`authzid \0 user \0 password`).
fn plain_user(b64: &str) -> Option<String> {
    let bytes = decode(b64)?;
    let parts: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    (parts.len() == 3).then(|| String::from_utf8_lossy(parts[1]).into_owned())
}

/// The address in `<...>` after `FROM:`/`TO:`, and the `SIZE=` parameter.
fn path_and_size(rest: &str) -> (String, Option<usize>) {
    let rest = rest.trim();
    let (address, params) = match (rest.find('<'), rest.find('>')) {
        (Some(open), Some(close)) if open < close => (&rest[open + 1..close], &rest[close + 1..]),
        _ => rest.split_once(' ').unwrap_or((rest, "")),
    };
    let size = params
        .split_whitespace()
        .find_map(|p| p.get(..5).filter(|k| k.eq_ignore_ascii_case("SIZE=")).and(p.get(5..)))
        .and_then(|n| n.parse().ok());
    (address.trim().to_string(), size)
}

/// `FROM:` or `TO:` (any case) at the start of `arg`, and what follows.
fn after<'a>(arg: &'a str, key: &str) -> Option<&'a str> {
    arg.get(..key.len()).filter(|k| k.eq_ignore_ascii_case(key)).map(|_| &arg[key.len()..])
}

/// An HTTP request line or header, as a browser sends it.
fn looks_like_http(line: &str) -> bool {
    let upper = line.to_ascii_uppercase();
    let methods = ["GET ", "POST ", "PUT ", "HEAD ", "OPTIONS ", "DELETE ", "PATCH ", "CONNECT "];
    methods.iter().any(|m| upper.starts_with(m)) || upper.contains(" HTTP/") || upper.starts_with("HOST:")
}

async fn session(stream: TcpStream, paths: HearthPaths, max: usize) {
    let (r, w) = stream.into_split();
    let mut c = Conn { r: BufReader::new(r), w };
    if !c.say("220 hearth ESMTP ready").await {
        return;
    }
    let mut site: Option<String> = None;
    let mut logged_in = false;
    let mut from: Option<String> = None;
    let mut to: Vec<String> = vec![];
    let mut unknown = 0;
    loop {
        let line = match c.line().await {
            Line::Closed => return,
            Line::TooLong => {
                c.say("500 5.5.2 Line too long").await;
                continue;
            }
            Line::Text(t) => t,
        };
        // A web page can post a form here with SMTP commands in its body: hang up at the request
        // line, before the body is read.
        if looks_like_http(&line) {
            c.say("421 4.7.0 This is a mail catcher, not a web server").await;
            return;
        }
        let (verb, arg) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let reply = match verb.to_ascii_uppercase().as_str() {
            "EHLO" => {
                (from, to) = (None, vec![]);
                format!("250-hearth\r\n250-SIZE {max}\r\n250-8BITMIME\r\n250 AUTH PLAIN LOGIN")
            }
            "HELO" => {
                (from, to) = (None, vec![]);
                "250 hearth".into()
            }
            "AUTH" if logged_in => "503 5.5.1 Already logged in".into(),
            "AUTH" => {
                let (mechanism, initial) = arg.split_once(' ').unwrap_or((arg, ""));
                let user = match mechanism.to_ascii_uppercase().as_str() {
                    "PLAIN" => {
                        let response = if initial.is_empty() {
                            if !c.say("334 ").await {
                                return;
                            }
                            match c.line().await {
                                Line::Text(t) => t,
                                _ => return,
                            }
                        } else {
                            initial.to_string()
                        };
                        plain_user(&response).ok_or("501 5.5.2 Cannot decode the login")
                    }
                    "LOGIN" => {
                        let name = if initial.is_empty() {
                            if !c.say("334 VXNlcm5hbWU6").await {
                                return;
                            }
                            match c.line().await {
                                Line::Text(t) => t,
                                _ => return,
                            }
                        } else {
                            initial.to_string()
                        };
                        if !c.say("334 UGFzc3dvcmQ6").await {
                            return;
                        }
                        if !matches!(c.line().await, Line::Text(_)) {
                            return;
                        }
                        decode(&name)
                            .map(|n| String::from_utf8_lossy(&n).into_owned())
                            .ok_or("501 5.5.2 Cannot decode the login")
                    }
                    _ => Err("504 5.5.4 Hearth knows AUTH PLAIN and LOGIN"),
                };
                match user {
                    Ok(user) => {
                        logged_in = true;
                        site = Some(user.trim().to_string()).filter(|u| !u.is_empty());
                        "235 2.7.0 Logged in".into()
                    }
                    Err(reply) => reply.into(),
                }
            }
            "MAIL" => match after(arg, "FROM:") {
                None => "501 5.5.4 Use MAIL FROM:<address>".into(),
                Some(_) if from.is_some() => "503 5.5.1 A mail is already started".into(),
                Some(rest) => match path_and_size(rest) {
                    (_, Some(size)) if size > max => format!("552 5.3.4 Mails over {max} bytes are not taken"),
                    (address, _) => {
                        from = Some(address);
                        "250 2.1.0 Ok".into()
                    }
                },
            },
            "RCPT" => match after(arg, "TO:") {
                None => "501 5.5.4 Use RCPT TO:<address>".into(),
                Some(_) if from.is_none() => "503 5.5.1 MAIL FROM comes first".into(),
                Some(rest) => {
                    to.push(path_and_size(rest).0);
                    "250 2.1.5 Ok".into()
                }
            },
            "DATA" if from.is_none() || to.is_empty() => "503 5.5.1 MAIL FROM and RCPT TO come first".into(),
            "DATA" => {
                if !c.say("354 End the mail with a line holding only a dot").await {
                    return;
                }
                let Ok(data) = c.data(max).await else { return };
                let envelope =
                    Envelope { site: site.clone(), from: from.take().unwrap_or_default(), to: std::mem::take(&mut to) };
                match data {
                    None => format!("552 5.3.4 Mails over {max} bytes are not taken"),
                    Some(raw) => {
                        let paths = paths.clone();
                        match tokio::task::spawn_blocking(move || store::save(&paths, &envelope, &raw)).await {
                            Ok(Ok(summary)) => format!("250 2.0.0 Ok: kept as {}", summary.id),
                            Ok(Err(e)) => format!("451 4.3.0 Hearth could not keep the mail: {e}"),
                            Err(_) => "451 4.3.0 Hearth could not keep the mail".into(),
                        }
                    }
                }
            }
            "RSET" => {
                (from, to) = (None, vec![]);
                "250 2.0.0 Ok".into()
            }
            "NOOP" => "250 2.0.0 Ok".into(),
            "VRFY" => "252 2.5.0 Any address is taken".into(),
            "QUIT" => {
                c.say("221 2.0.0 Bye").await;
                return;
            }
            _ => {
                unknown += 1;
                if unknown >= 3 {
                    c.say("421 4.7.0 Too many unknown commands").await;
                    return;
                }
                "502 5.5.2 Command not recognized".into()
            }
        };
        if !c.say(&reply).await {
            return;
        }
    }
}

/// What the Mail page shows about the catcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatcherStatus {
    pub port: u16,
    pub listening: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    port: u16,
    task: Option<tokio::task::JoinHandle<()>>,
    error: Option<String>,
}

/// The running catcher of the core.
#[derive(Default)]
pub struct Catcher {
    state: std::sync::Mutex<State>,
}

async fn bind(port: u16) -> Result<TcpListener, CoreError> {
    let taken = || {
        CoreError::Mail(format!(
            "Port {port} is taken by another program (Herd's mail catcher uses 2525 too). Pick another port for Hearth's mail."
        ))
    };
    // Something listening on all addresses does not always block a bind to 127.0.0.1 on Windows.
    let probe = tokio::time::timeout(Duration::from_millis(150), TcpStream::connect(("127.0.0.1", port))).await;
    if port != 0 && matches!(probe, Ok(Ok(_))) {
        return Err(taken());
    }
    TcpListener::bind(("127.0.0.1", port)).await.map_err(|_| taken())
}

impl Catcher {
    /// Listens on `port`, in place of the port before. When `port` cannot be had, the port before
    /// keeps listening; with none before, the status shows the error.
    pub async fn start(&self, paths: &HearthPaths, port: u16) -> Result<(), CoreError> {
        let bound = bind(port).await;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match bound {
            Ok(listener) => {
                if let Some(old) = state.task.take() {
                    old.abort();
                }
                state.port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
                state.error = None;
                state.task = Some(tokio::spawn(serve(listener, paths.clone())));
                Ok(())
            }
            Err(e) => {
                if state.task.is_none() {
                    state.port = port;
                    state.error = Some(e.to_string());
                }
                Err(e)
            }
        }
    }

    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(task) = state.task.take() {
            task.abort();
        }
    }

    pub fn status(&self) -> CatcherStatus {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        CatcherStatus {
            port: state.port,
            listening: state.task.as_ref().is_some_and(|t| !t.is_finished()),
            error: state.error.clone(),
        }
    }
}
