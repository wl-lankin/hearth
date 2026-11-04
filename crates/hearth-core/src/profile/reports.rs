//! The reports SPX writes to its data folder: `<key>.json` (what was profiled) plus
//! `<key>.txt.gz` (the profile, read by SPX's own web UI).

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::CoreError;

/// One profiled request or command, as the Profiles page lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// SPX's report key; its web UI opens the report by it.
    pub key: String,
    /// Unix seconds.
    pub time: u64,
    /// The site's host without the port; empty for a command.
    pub host: String,
    pub method: String,
    /// The request URI, or the command line of a command-line profile.
    pub uri: String,
    pub cli: bool,
    pub wall_ms: u64,
    /// Bytes.
    pub peak_memory: u64,
    pub functions: u64,
}

/// A report from SPX's metadata json; `None` for anything that is not one (half written, other files).
fn parse(text: &str) -> Option<Report> {
    let v: Value = serde_json::from_str(text).ok()?;
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
    let key = v.get("key")?.as_str()?.to_string();
    let cli = n("cli") == 1;
    let host = s("http_host");
    let host =
        host.rsplit_once(':').filter(|(_, port)| port.parse::<u16>().is_ok()).map_or(host.clone(), |(h, _)| h.into());
    Some(Report {
        key,
        time: n("exec_ts"),
        host,
        method: s("http_method"),
        uri: if cli { s("cli_command_line") } else { s("http_request_uri") },
        cli,
        wall_ms: n("wall_time_ms"),
        peak_memory: n("peak_memory_usage"),
        functions: n("called_function_count"),
    })
}

/// The newest `limit` reports in `dir`, newest first; a missing folder has none.
pub fn list(dir: &Path, limit: usize) -> Vec<Report> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut reports: Vec<Report> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| parse(&text))
        .collect();
    reports.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| b.key.cmp(&a.key)));
    reports.truncate(limit);
    reports
}

/// Deletes every report (`.json` and `.txt.gz`) in `dir`; returns how many reports there were.
pub fn clear(dir: &Path) -> Result<usize, CoreError> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(0) };
    let mut count = 0;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".json") || name.ends_with(".txt.gz") {
            std::fs::remove_file(entry.path()).map_err(|e| CoreError::io(entry.path(), e))?;
            count += usize::from(name.ends_with(".json"));
        }
    }
    Ok(count)
}

#[cfg(test)]
#[path = "reports_tests.rs"]
mod tests;
