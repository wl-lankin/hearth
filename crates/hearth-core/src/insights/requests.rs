//! Requests (and short artisan commands) of Laravel apps with their SQL queries, and what is wrong
//! with those queries: N+1 loops, duplicates and slow ones.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// How many requests the core keeps.
pub const KEEP_REQUESTS: usize = 200;
/// The same query shape from one line this often is an N+1 loop.
const N_PLUS_ONE: usize = 5;
/// A query this slow (ms) is pointed out.
const SLOW_MS: f64 = 100.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    pub sql: String,
    #[serde(default)]
    pub bindings: Vec<String>,
    pub ms: f64,
    /// When it started, in ms after the request started.
    #[serde(default, alias = "start")]
    pub start_ms: f64,
    #[serde(default)]
    pub connection: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    /// `n+1`, `duplicate` or `slow`.
    pub kind: String,
    pub text: String,
    /// Indexes into the request's queries.
    pub queries: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: u64,
    /// Unix milliseconds, when the core got it.
    pub time: u64,
    pub site: Option<String>,
    pub sapi: String,
    pub method: Option<String>,
    pub uri: Option<String>,
    /// The artisan command, for command-line runs.
    pub command: Option<String>,
    pub status: Option<u16>,
    pub ms: f64,
    pub memory: u64,
    pub queries: Vec<Query>,
    /// Queries over the 500 PHP keeps.
    pub dropped: u32,
    pub findings: Vec<Finding>,
}

/// A request as the list shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSummary {
    pub id: u64,
    pub time: u64,
    pub site: Option<String>,
    pub sapi: String,
    pub method: Option<String>,
    pub uri: Option<String>,
    pub command: Option<String>,
    pub status: Option<u16>,
    pub ms: f64,
    pub memory: u64,
    pub query_count: usize,
    pub query_ms: f64,
    pub dropped: u32,
    /// The kinds found, each once, in the order `n+1`, `duplicate`, `slow`.
    pub findings: Vec<String>,
}

/// What PHP sends.
#[derive(Deserialize)]
pub(crate) struct Incoming {
    site: Option<String>,
    sapi: Option<String>,
    method: Option<String>,
    uri: Option<String>,
    command: Option<String>,
    status: Option<u16>,
    #[serde(default)]
    ms: f64,
    #[serde(default)]
    memory: u64,
    #[serde(default)]
    queries: Vec<Query>,
    #[serde(default)]
    dropped: u32,
}

impl Request {
    pub(crate) fn from_incoming(id: u64, time: u64, incoming: Incoming) -> Request {
        let findings = analyse(&incoming.queries);
        Request {
            id,
            time,
            site: incoming.site.filter(|s| !s.is_empty()),
            sapi: incoming.sapi.unwrap_or_default(),
            method: incoming.method,
            uri: incoming.uri,
            command: incoming.command,
            status: incoming.status,
            ms: incoming.ms,
            memory: incoming.memory,
            queries: incoming.queries,
            dropped: incoming.dropped,
            findings,
        }
    }

    pub fn summary(&self) -> RequestSummary {
        let mut kinds: Vec<String> = vec![];
        for kind in ["n+1", "duplicate", "slow"] {
            if self.findings.iter().any(|f| f.kind == kind) {
                kinds.push(kind.into());
            }
        }
        RequestSummary {
            id: self.id,
            time: self.time,
            site: self.site.clone(),
            sapi: self.sapi.clone(),
            method: self.method.clone(),
            uri: self.uri.clone(),
            command: self.command.clone(),
            status: self.status,
            ms: self.ms,
            memory: self.memory,
            query_count: self.queries.len(),
            query_ms: self.queries.iter().map(|q| q.ms).sum(),
            dropped: self.dropped,
            findings: kinds,
        }
    }
}

/// The shape of a query: string and number literals become `?`, `(?, ?, ?)` becomes `(?)`, and
/// whitespace runs become one space. Two queries of one shape differ only in their values.
pub fn normalize(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            // A string literal ('' is a quote inside it).
            while let Some(n) = chars.next() {
                if n == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
            out.push('?');
        } else if c.is_ascii_digit() && !out.chars().last().is_some_and(|p| p.is_alphanumeric() || p == '_') {
            while chars.peek().is_some_and(|n| n.is_ascii_digit() || *n == '.') {
                chars.next();
            }
            out.push('?');
        } else if c.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    let mut folded = out.trim().to_string();
    for pair in ["?, ?", "?,?"] {
        while folded.contains(pair) {
            folded = folded.replace(pair, "?");
        }
    }
    folded
}

fn place(q: &Query) -> String {
    let file = q.file.as_deref().unwrap_or("unknown file");
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    match q.line {
        Some(line) => format!("{name}:{line}"),
        None => name.to_string(),
    }
}

fn short(sql: &str) -> String {
    let mut s: String = sql.chars().take(120).collect();
    if sql.chars().count() > 120 {
        s.push_str("...");
    }
    s
}

/// A query that reads: only reads can be N+1 loops or needless duplicates (migrations and seeders
/// write row after row on purpose).
fn reads(sql: &str) -> bool {
    let start = sql.trim_start().get(..6).unwrap_or("").to_ascii_lowercase();
    start == "select" || start.starts_with("with")
}

/// N+1 loops, duplicate queries and slow queries, in that order.
pub fn analyse(queries: &[Query]) -> Vec<Finding> {
    let mut found = vec![];
    // N+1: one shape from one line, again and again.
    // (shape, file, line) to the queries with it.
    type Shape<'a> = (String, Option<&'a str>, Option<u32>);
    let mut shapes: HashMap<Shape, Vec<usize>> = HashMap::new();
    for (i, q) in queries.iter().enumerate().filter(|(_, q)| reads(&q.sql)) {
        shapes.entry((normalize(&q.sql), q.file.as_deref(), q.line)).or_default().push(i);
    }
    let mut loops: Vec<(String, Vec<usize>)> =
        shapes.into_iter().filter(|(_, ids)| ids.len() >= N_PLUS_ONE).map(|(k, ids)| (k.0, ids)).collect();
    loops.sort_by_key(|(_, ids)| ids[0]);
    let mut in_loop = vec![false; queries.len()];
    for (shape, ids) in loops {
        for &i in &ids {
            in_loop[i] = true;
        }
        let text = format!("Ran {} times from {}: {}", ids.len(), place(&queries[ids[0]]), short(&shape));
        found.push(Finding { kind: "n+1".into(), text, queries: ids });
    }
    // Duplicates: the same query with the same values, outside the loops already named.
    let mut same: HashMap<(&str, &[String]), Vec<usize>> = HashMap::new();
    for (i, q) in queries.iter().enumerate().filter(|(_, q)| reads(&q.sql)) {
        same.entry((q.sql.as_str(), q.bindings.as_slice())).or_default().push(i);
    }
    let mut dups: Vec<Vec<usize>> =
        same.into_values().filter(|ids| ids.len() >= 2 && !ids.iter().all(|&i| in_loop[i])).collect();
    dups.sort_by_key(|ids| ids[0]);
    for ids in dups {
        let q = &queries[ids[0]];
        let text = format!("The same query ran {} times, first from {}: {}", ids.len(), place(q), short(&q.sql));
        found.push(Finding { kind: "duplicate".into(), text, queries: ids });
    }
    // Slow ones, each on its own.
    for (i, q) in queries.iter().enumerate() {
        if q.ms >= SLOW_MS {
            let text = format!("Took {:.0} ms at {}: {}", q.ms, place(q), short(&q.sql));
            found.push(Finding { kind: "slow".into(), text, queries: vec![i] });
        }
    }
    found
}
