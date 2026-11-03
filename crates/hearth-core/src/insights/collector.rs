//! The collector: PHP's prepend script sends one JSON line per event to 127.0.0.1:<port>, and the
//! core keeps the newest ones in memory for the app.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::insights::requests::{self, Request, RequestSummary};
use crate::{atomic, CoreError, HearthPaths};

pub const DEFAULT_PORT: u16 = 2306;
/// How many dumps are kept.
pub const KEEP: usize = 500;
/// The most text one dump keeps.
pub const TEXT_MAX: usize = 256 * 1024;
/// A longer line is skipped whole. JSON escaping can make 256 KB of text up to six times longer.
const LINE_MAX: usize = 6 * TEXT_MAX + 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorConfig {
    pub port: u16,
    /// Whether the prepend script defines dump() and dd().
    pub dumps: bool,
    /// Whether the prepend script records Laravel requests and their queries.
    #[serde(default = "on")]
    pub requests: bool,
}

fn on() -> bool {
    true
}

impl Default for CollectorConfig {
    fn default() -> Self {
        CollectorConfig { port: DEFAULT_PORT, dumps: true, requests: true }
    }
}

fn config_file(paths: &HearthPaths) -> std::path::PathBuf {
    paths.config.join("collector.json")
}

/// `config/collector.json`, the defaults when it is missing or unreadable.
pub fn config(paths: &HearthPaths) -> CollectorConfig {
    std::fs::read(config_file(paths))
        .ok()
        .and_then(|b| serde_json::from_slice::<CollectorConfig>(&b).ok())
        .filter(|c| c.port > 0)
        .unwrap_or_default()
}

pub fn set_config(paths: &HearthPaths, config: &CollectorConfig) -> Result<(), CoreError> {
    atomic::write(&config_file(paths), &serde_json::to_vec(config).unwrap_or_default())
}

/// One `dump()` call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dump {
    pub id: u64,
    /// Unix milliseconds, when the core got it.
    pub time: u64,
    /// The host the request was for, or the working folder's name on the command line.
    pub site: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    /// PHP's SAPI: `cli`, `fpm-fcgi`, `cgi-fcgi`.
    pub sapi: String,
    pub uri: Option<String>,
    pub text: String,
    pub truncated: bool,
}

#[derive(Deserialize)]
struct Incoming {
    site: Option<String>,
    file: Option<String>,
    line: Option<u32>,
    sapi: Option<String>,
    uri: Option<String>,
    text: String,
    /// PHP already cut the text.
    #[serde(default)]
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorStatus {
    pub port: u16,
    pub listening: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct Listening {
    port: u16,
    task: Option<tokio::task::JoinHandle<()>>,
    error: Option<String>,
}

#[derive(Default)]
struct Store {
    dumps: Mutex<VecDeque<Dump>>,
    next: AtomicU64,
    requests: Mutex<VecDeque<Request>>,
    next_request: AtomicU64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Store {
    fn add_request(&self, incoming: requests::Incoming) {
        let id = self.next_request.fetch_add(1, Ordering::Relaxed) + 1;
        let request = Request::from_incoming(id, now_ms(), incoming);
        let mut all = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        all.push_front(request);
        all.truncate(requests::KEEP_REQUESTS);
    }

    fn add(&self, incoming: Incoming) {
        let mut text = incoming.text;
        let truncated = incoming.truncated || text.len() > TEXT_MAX;
        if truncated {
            let mut cut = TEXT_MAX;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
        }
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let dump = Dump {
            id: self.next.fetch_add(1, Ordering::Relaxed) + 1,
            time,
            site: incoming.site.filter(|s| !s.is_empty()),
            file: incoming.file.filter(|s| !s.is_empty()),
            line: incoming.line,
            sapi: incoming.sapi.unwrap_or_default(),
            uri: incoming.uri.filter(|s| !s.is_empty()),
            text,
            truncated,
        };
        let mut dumps = self.dumps.lock().unwrap_or_else(|e| e.into_inner());
        dumps.push_front(dump);
        dumps.truncate(KEEP);
    }
}

/// The collector of the core.
#[derive(Default)]
pub struct Collector {
    listening: Mutex<Listening>,
    store: Arc<Store>,
}

fn looks_like_http(line: &[u8]) -> bool {
    let methods: [&[u8]; 8] = [b"GET ", b"POST ", b"PUT ", b"HEAD ", b"OPTIONS ", b"DELETE ", b"PATCH ", b"CONNECT "];
    methods.iter().any(|m| line.starts_with(m)) || line.windows(6).any(|w| w == b" HTTP/")
}

async fn session(stream: TcpStream, store: Arc<Store>) {
    let mut r = BufReader::new(stream);
    loop {
        let mut buf = Vec::new();
        match (&mut r).take(LINE_MAX as u64).read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if buf.len() >= LINE_MAX && !buf.ends_with(b"\n") {
            // Skip the rest of the oversized line.
            loop {
                let mut rest = Vec::new();
                match (&mut r).take(LINE_MAX as u64).read_until(b'\n', &mut rest).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) if rest.ends_with(b"\n") => break,
                    Ok(_) => {}
                }
            }
            continue;
        }
        // Dump lines are JSON objects; a browser's request line is not.
        if !buf.starts_with(b"{") && looks_like_http(&buf) {
            return;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&buf) else { continue };
        match value.get("type").and_then(|t| t.as_str()) {
            Some("dump") => {
                if let Ok(incoming) = serde_json::from_value::<Incoming>(value) {
                    store.add(incoming);
                }
            }
            Some("request") => {
                if let Ok(incoming) = serde_json::from_value::<requests::Incoming>(value) {
                    store.add_request(incoming);
                }
            }
            _ => {}
        }
    }
}

async fn serve(listener: TcpListener, store: Arc<Store>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(session(stream, store.clone()));
            }
            // Out of sockets or similar: wait instead of spinning.
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

async fn bind(port: u16) -> Result<TcpListener, CoreError> {
    let taken = || CoreError::Php(format!("Port {port} is taken by another program. Pick another port for dumps."));
    // Something listening on all addresses does not always block a bind to 127.0.0.1 on Windows.
    let probe = tokio::time::timeout(Duration::from_millis(150), TcpStream::connect(("127.0.0.1", port))).await;
    if port != 0 && matches!(probe, Ok(Ok(_))) {
        return Err(taken());
    }
    TcpListener::bind(("127.0.0.1", port)).await.map_err(|_| taken())
}

impl Collector {
    /// Listens on `port`, in place of the port before. When `port` cannot be had, the port before
    /// keeps listening; with none before, the status shows the error.
    pub async fn start(&self, port: u16) -> Result<(), CoreError> {
        let bound = bind(port).await;
        let mut state = self.listening.lock().unwrap_or_else(|e| e.into_inner());
        match bound {
            Ok(listener) => {
                if let Some(old) = state.task.take() {
                    old.abort();
                }
                state.port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
                state.error = None;
                state.task = Some(tokio::spawn(serve(listener, self.store.clone())));
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
        let mut state = self.listening.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(task) = state.task.take() {
            task.abort();
        }
    }

    pub fn status(&self) -> CollectorStatus {
        let state = self.listening.lock().unwrap_or_else(|e| e.into_inner());
        CollectorStatus {
            port: state.port,
            listening: state.task.as_ref().is_some_and(|t| !t.is_finished()),
            error: state.error.clone(),
        }
    }

    /// The kept dumps, newest first.
    pub fn dumps(&self) -> Vec<Dump> {
        self.store.dumps.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }

    /// The dumps newer than `id`, newest first (the page asks only for what it has not seen).
    pub fn dumps_after(&self, id: u64) -> Vec<Dump> {
        let dumps = self.store.dumps.lock().unwrap_or_else(|e| e.into_inner());
        dumps.iter().take_while(|d| d.id > id).cloned().collect()
    }

    /// The requests newer than `id`, newest first.
    pub fn requests_after(&self, id: u64) -> Vec<RequestSummary> {
        let all = self.store.requests.lock().unwrap_or_else(|e| e.into_inner());
        all.iter().take_while(|r| r.id > id).map(Request::summary).collect()
    }

    pub fn request(&self, id: u64) -> Option<Request> {
        let all = self.store.requests.lock().unwrap_or_else(|e| e.into_inner());
        all.iter().find(|r| r.id == id).cloned()
    }

    pub fn clear_requests(&self) {
        self.store.requests.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    pub fn clear(&self) {
        self.store.dumps.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}
