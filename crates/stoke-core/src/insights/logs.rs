//! The Laravel logs of Stoke's sites: `<site folder>/storage/logs/*.log`, split into entries.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde::Serialize;

use crate::sites::registry;
use crate::{CoreError, StokePaths};

/// How much of the end of a log file is read.
const TAIL_BYTES: u64 = 2 * 1024 * 1024;
/// The most entries a read returns.
pub const ENTRIES_MAX: usize = 500;
/// The most text one entry keeps (message and detail each).
const ENTRY_TEXT_MAX: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogFile {
    pub name: String,
    pub size: u64,
    /// Unix milliseconds.
    pub modified: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteLogs {
    pub site: String,
    /// Newest first.
    pub files: Vec<LogFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// As written: `2026-11-03 10:00:00` or an ISO time.
    pub time: String,
    /// Laravel's environment (Monolog's channel): `local`, `production`.
    pub env: String,
    /// Lowercase: `debug` to `emergency`.
    pub level: String,
    /// The first line.
    pub message: String,
    /// The lines after it: context and stack trace.
    pub detail: String,
    pub truncated: bool,
}

/// `[time] env.LEVEL: message` at the start of a line: (time, env, level, message).
fn head(line: &str) -> Option<(&str, &str, &str, &str)> {
    let rest = line.strip_prefix('[')?;
    let close = rest.find(']')?;
    let time = &rest[..close];
    let b = time.as_bytes();
    let date_ok = b.len() >= 11
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
        && (b[10] == b' ' || b[10] == b'T');
    if !date_ok {
        return None;
    }
    let after = rest[close + 1..].strip_prefix(' ')?;
    let (channel, tail) = after.split_once('.')?;
    if channel.is_empty() || !channel.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-') {
        return None;
    }
    let colon = tail.find(':')?;
    let level = &tail[..colon];
    if level.is_empty() || !level.bytes().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let message = tail[colon + 1..].strip_prefix(' ').unwrap_or(&tail[colon + 1..]);
    Some((time, channel, level, message))
}

fn cut(text: &mut String) -> bool {
    if text.len() <= ENTRY_TEXT_MAX {
        return false;
    }
    let mut at = ENTRY_TEXT_MAX;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    text.truncate(at);
    true
}

/// The entries of a log's text, newest first, at most `ENTRIES_MAX`. Lines before the first entry
/// (the end of one cut off by reading only the tail) are dropped.
pub fn parse(text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = vec![];
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some((time, env, level, message)) = head(line) {
            entries.push(Entry {
                time: time.into(),
                env: env.into(),
                level: level.to_ascii_lowercase(),
                message: message.into(),
                detail: String::new(),
                truncated: false,
            });
        } else if let Some(entry) = entries.last_mut() {
            if entry.detail.len() <= ENTRY_TEXT_MAX {
                if !entry.detail.is_empty() {
                    entry.detail.push('\n');
                }
                entry.detail.push_str(line);
            } else {
                entry.truncated = true;
            }
        }
    }
    let skip = entries.len().saturating_sub(ENTRIES_MAX);
    entries
        .into_iter()
        .skip(skip)
        .rev()
        .map(|mut e| {
            e.detail = e.detail.trim_end_matches(['\n', ' ']).to_string();
            let cut_message = cut(&mut e.message);
            let cut_detail = cut(&mut e.detail);
            e.truncated |= cut_message || cut_detail;
            e
        })
        .collect()
}

/// The site's folder, if it has one.
fn site_folder(paths: &StokePaths, site: &str) -> Result<PathBuf, CoreError> {
    let found = registry::list(paths).into_iter().find(|s| s.name == site);
    found.and_then(|s| s.path).ok_or_else(|| CoreError::Php(format!("{site}.test has no folder")))
}

/// A log file of `site`: only a plain `*.log` name, a real file (not a link), whose real path stays
/// inside the site's folder. A project could hold a link to a file elsewhere, which Empty would cut.
fn log_file(paths: &StokePaths, site: &str, name: &str) -> Result<PathBuf, CoreError> {
    let plain = name.ends_with(".log") && !name.contains(['/', '\\']) && !name.contains("..") && name.len() > 4;
    if !plain {
        return Err(CoreError::Php(format!("{name:?} is not a log file name")));
    }
    let folder = site_folder(paths, site)?;
    let path = folder.join("storage").join("logs").join(name);
    let gone = || CoreError::Php(format!("{name} is gone"));
    let meta = std::fs::symlink_metadata(&path).map_err(|_| gone())?;
    if !meta.file_type().is_file() {
        return Err(CoreError::Php(format!("{name} is not a plain file")));
    }
    let (real, root) = (path.canonicalize().map_err(|_| gone())?, folder.canonicalize().map_err(|_| gone())?);
    if !real.starts_with(&root) {
        return Err(CoreError::Php(format!("{name} is outside {site}'s folder")));
    }
    Ok(path)
}

fn modified_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The sites with at least one log file, by name; their files newest first.
pub fn sites(paths: &StokePaths) -> Vec<SiteLogs> {
    let mut out = vec![];
    for site in registry::list(paths) {
        let Some(folder) = site.path else { continue };
        let Ok(entries) = std::fs::read_dir(folder.join("storage").join("logs")) else { continue };
        let mut files: Vec<LogFile> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                // Links are left out (see log_file); file_type does not follow them.
                if !e.file_type().ok()?.is_file() {
                    return None;
                }
                let meta = e.metadata().ok()?;
                (name.ends_with(".log")).then(|| LogFile { name, size: meta.len(), modified: modified_ms(&meta) })
            })
            .collect();
        if files.is_empty() {
            continue;
        }
        files.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.name.cmp(&a.name)));
        out.push(SiteLogs { site: site.name, files });
    }
    out.sort_by(|a, b| a.site.cmp(&b.site));
    out
}

/// The entries at the end of a log file, and the file's size.
pub fn read(paths: &StokePaths, site: &str, name: &str) -> Result<(Vec<Entry>, u64), CoreError> {
    let path = log_file(paths, site, name)?;
    let mut file = std::fs::File::open(&path).map_err(|e| CoreError::io(&path, e))?;
    let size = file.metadata().map_err(|e| CoreError::io(&path, e))?.len();
    let start = size.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).map_err(|e| CoreError::io(&path, e))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| CoreError::io(&path, e))?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        // Reading started inside a line: drop that part.
        text = text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default();
    }
    Ok((parse(&text), size))
}

/// The file's size, to tell whether it changed.
pub fn size(paths: &StokePaths, site: &str, name: &str) -> Result<u64, CoreError> {
    let path = log_file(paths, site, name)?;
    std::fs::metadata(&path).map(|m| m.len()).map_err(|e| CoreError::io(&path, e))
}

/// Empties a log file (the app may keep writing to it).
pub fn clear(paths: &StokePaths, site: &str, name: &str) -> Result<(), CoreError> {
    let path = log_file(paths, site, name)?;
    std::fs::OpenOptions::new().write(true).truncate(true).open(&path).map(|_| ()).map_err(|e| CoreError::io(&path, e))
}

/// An exception Laravel logged: from `[object] (Class(code: N): message at file:line)` and the
/// `[stacktrace]` after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Exception {
    pub time: String,
    pub level: String,
    /// The log file it is in.
    pub log: String,
    pub class: String,
    pub code: String,
    pub message: String,
    pub file: String,
    pub line: Option<u64>,
    pub trace: String,
}

/// Laravel writes the exception inside JSON: backslashes and quotes come escaped.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            ('\\', Some(next @ ('\\' | '"' | '/'))) => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

fn exception_of(entry: &Entry, log: &str) -> Option<Exception> {
    let text = format!("{}\n{}", entry.message, entry.detail);
    let object = text.find("[object] (");
    let trace_at = text.find("[stacktrace]");
    if object.is_none() && trace_at.is_none() {
        return None;
    }
    let trace = trace_at
        .map(|at| {
            let lines: Vec<&str> = text[at..].lines().skip(1).filter(|l| l.trim() != "\"}").collect();
            unescape(lines.join("\n").trim_end())
        })
        .unwrap_or_default();
    let mut found = Exception {
        time: entry.time.clone(),
        level: entry.level.clone(),
        log: log.to_string(),
        class: String::new(),
        code: String::new(),
        message: entry.message.clone(),
        file: String::new(),
        line: None,
        trace,
    };
    let Some(at) = object else { return Some(found) };
    let head = text[at + "[object] (".len()..].lines().next().unwrap_or_default();
    let head = head.trim_end().trim_end_matches("\"}").trim_end_matches(')');
    if let Some((class, rest)) = head.split_once("(code: ") {
        if let Some((code, rest)) = rest.split_once("): ") {
            found.class = unescape(class);
            found.code = code.to_string();
            match rest.rsplit_once(" at ") {
                Some((message, place)) => {
                    found.message = unescape(message);
                    match place.rsplit_once(':') {
                        Some((file, line)) if line.parse::<u64>().is_ok() => {
                            found.file = unescape(file);
                            found.line = line.parse().ok();
                        }
                        _ => found.file = unescape(place),
                    }
                }
                None => found.message = unescape(rest),
            }
        }
    }
    Some(found)
}

/// The newest exception in the site's logs (newest file first, the end of each), if any.
pub fn last_exception(paths: &StokePaths, site: &str) -> Result<Option<Exception>, CoreError> {
    let folder = site_folder(paths, site)?;
    let mut files: Vec<(u64, String)> = std::fs::read_dir(folder.join("storage").join("logs"))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let meta = e.metadata().ok()?;
                    name.ends_with(".log").then(|| (modified_ms(&meta), name))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| b.cmp(a));
    for (_, name) in files {
        let Ok((entries, _)) = read(paths, site, &name) else { continue };
        if let Some(found) = entries.iter().find_map(|e| exception_of(e, &name)) {
            return Ok(Some(found));
        }
    }
    Ok(None)
}
