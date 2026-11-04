//! `.env` files, edited line by line: only the lines of the keys set change; comments, order,
//! blank lines and the line endings stay as they were. Read as phpdotenv reads them: the last
//! line of a key wins.

/// `value` as it is written after `KEY=`. Quoted when it holds a space, `#`, quotes or `$`: single
/// quotes keep it literal (Windows paths: phpdotenv rejects `\U` inside double quotes); with a
/// single quote inside, double quotes with `\`, `"` and `$` escaped.
fn written(value: &str) -> String {
    // Unquoted values are read literally (backslashes included) up to a space or `#`.
    if !value.chars().any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '$')) {
        return value.to_string();
    }
    if !value.contains('\'') {
        return format!("'{value}'");
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$");
    format!("\"{escaped}\"")
}

/// The key a line sets (`KEY=`, `KEY = `, `export KEY=`) and whether it had `export `.
fn key_of(line: &str) -> Option<(&str, bool)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let (body, export) = match trimmed.strip_prefix("export ") {
        Some(rest) => (rest.trim_start(), true),
        None => (trimmed, false),
    };
    let (key, _) = body.split_once('=')?;
    let key = key.trim();
    (!key.is_empty() && !key.contains(char::is_whitespace)).then_some((key, export))
}

/// `text` with every line of each key replaced (an `export ` stays), and keys that have none
/// appended at the end, in the file's own line ending.
pub fn set(text: &str, values: &[(&str, &str)]) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = text.to_string();
    for (key, value) in values {
        let line = format!("{key}={}", written(value));
        let mut found = false;
        let mut next = String::with_capacity(out.len());
        for piece in out.split_inclusive('\n') {
            let content = piece.trim_end_matches(['\r', '\n']);
            match key_of(content) {
                Some((k, export)) if k == *key => {
                    found = true;
                    next.push_str(if export { "export " } else { "" });
                    next.push_str(&line);
                    next.push_str(&piece[content.len()..]);
                }
                _ => next.push_str(piece),
            }
        }
        out = if found {
            next
        } else {
            let base = out.trim_end_matches(['\r', '\n']);
            if base.is_empty() {
                format!("{line}{eol}")
            } else {
                format!("{base}{eol}{line}{eol}")
            }
        };
    }
    out
}

/// The value of `key` (the last line that sets it), without quotes or a trailing ` # comment`.
pub fn get(text: &str, key: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let Some((k, _)) = key_of(line) else { continue };
        if k != key {
            continue;
        }
        let v = line.split_once('=').map(|(_, v)| v.trim()).unwrap_or_default();
        found = Some(match v.chars().next() {
            Some('\'') => v[1..].split('\'').next().unwrap_or_default().to_string(),
            Some('"') => {
                // Up to the closing quote; `\"`, `\\` and `\$` are escapes.
                let mut out = String::new();
                let mut chars = v[1..].chars();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => out.extend(chars.next()),
                        '"' => break,
                        c => out.push(c),
                    }
                }
                out
            }
            _ => v.split(" #").next().unwrap_or_default().trim().to_string(),
        });
    }
    found
}
