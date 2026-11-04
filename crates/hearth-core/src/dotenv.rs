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
    values
        .iter()
        .fold(text.to_string(), |out, (key, value)| put_line(&out, key, &format!("{key}={}", written(value)), true))
}

/// Whether `key` is a name a `.env` line can set.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && !key.starts_with('#') && !key.contains(['=', '\r', '\n']) && !key.contains(char::is_whitespace)
}

/// `text` with every line of `key` replaced by `line` (keeping an `export ` when `keep_export`),
/// or `line` appended when the key has none.
pub fn put_line(text: &str, key: &str, line: &str, keep_export: bool) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut found = false;
    let mut next = String::with_capacity(text.len() + line.len());
    for piece in text.split_inclusive('\n') {
        let content = piece.trim_end_matches(['\r', '\n']);
        match key_of(content) {
            Some((k, export)) if k == key => {
                found = true;
                next.push_str(if export && keep_export { "export " } else { "" });
                next.push_str(line);
                next.push_str(&piece[content.len()..]);
            }
            _ => next.push_str(piece),
        }
    }
    if found {
        return next;
    }
    let base = text.trim_end_matches(['\r', '\n']);
    if base.is_empty() {
        format!("{line}{eol}")
    } else {
        format!("{base}{eol}{line}{eol}")
    }
}

/// `text` without any line of `key`.
pub fn unset(text: &str, key: &str) -> String {
    text.split_inclusive('\n')
        .filter(|piece| key_of(piece.trim_end_matches(['\r', '\n'])).is_none_or(|(k, _)| k != key))
        .collect()
}

/// `text` with every line of `key` turned into a comment (`# KEY=...`).
pub fn comment_out(text: &str, key: &str) -> String {
    text.split_inclusive('\n')
        .map(|piece| match key_of(piece.trim_end_matches(['\r', '\n'])) {
            Some((k, _)) if k == key => format!("# {piece}"),
            _ => piece.to_string(),
        })
        .collect()
}

/// `text` with the first `# line` that `comment_out` made turned back into `line`.
pub fn uncomment(text: &str, line: &str) -> String {
    let commented = format!("# {line}");
    let mut done = false;
    text.split_inclusive('\n')
        .map(|piece| {
            let content = piece.trim_end_matches(['\r', '\n']);
            if !done && content == commented {
                done = true;
                format!("{line}{}", &piece[content.len()..])
            } else {
                piece.to_string()
            }
        })
        .collect()
}

/// The last line that sets `key`, as written (without its line ending).
pub fn line_of(text: &str, key: &str) -> Option<String> {
    text.lines().rfind(|line| key_of(line).is_some_and(|(k, _)| k == key)).map(str::to_string)
}

/// Every key once, in the order of its first line, with the value its last line gives.
pub fn all(text: &str) -> Vec<(String, String)> {
    let mut keys: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some((k, _)) = key_of(line) {
            if !keys.iter().any(|seen| seen == k) {
                keys.push(k.to_string());
            }
        }
    }
    keys.into_iter()
        .map(|k| {
            let v = get(text, &k).unwrap_or_default();
            (k, v)
        })
        .collect()
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
