//! `.env` files, edited line by line: only the lines of the keys set change; comments, order,
//! blank lines and the line endings stay as they were.

/// `value` as it is written after `KEY=`: double-quoted when it holds a space, `#`, quotes or `$`.
fn written(value: &str) -> String {
    if value.chars().any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '$')) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

/// `text` with each key's first `KEY=` line replaced (`export KEY=` lines are left alone), and keys
/// that have none appended at the end, in the file's own line ending.
pub fn set(text: &str, values: &[(&str, &str)]) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = text.to_string();
    for (key, value) in values {
        let line = format!("{key}={}", written(value));
        let prefix = format!("{key}=");
        let mut start = 0;
        let mut found = None;
        for piece in out.split_inclusive('\n') {
            if piece.starts_with(&prefix) {
                found = Some((start, start + piece.trim_end_matches(['\r', '\n']).len()));
                break;
            }
            start += piece.len();
        }
        out = match found {
            Some((from, to)) => format!("{}{line}{}", &out[..from], &out[to..]),
            None => {
                let base = out.trim_end_matches(['\r', '\n']);
                if base.is_empty() {
                    format!("{line}{eol}")
                } else {
                    format!("{base}{eol}{line}{eol}")
                }
            }
        };
    }
    out
}

/// The value of `key` (also from an `export KEY=` line), without quotes or a trailing ` # comment`.
pub fn get(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else { continue };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        return Some(match v.chars().next() {
            Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or_default().to_string(),
            _ => v.split(" #").next().unwrap_or_default().trim().to_string(),
        });
    }
    None
}
