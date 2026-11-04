//! The actions: each one calls Hearth's core and turns the reply into the text an agent reads.

use std::time::Duration;

use hearth_core::ipc;
use hearth_core::{CoreError, HearthPaths};
use serde_json::{json, Value};

use super::{site, start_dir};

/// A core error as an agent should read it.
pub fn readable(e: CoreError) -> String {
    match e {
        CoreError::NotRunning => "Hearth is not running. Start the Hearth app.".into(),
        CoreError::Rpc { message, .. } => message,
        other => other.to_string(),
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// A text argument; `null` and empty count as absent.
fn text(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn number(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

/// A yes/no argument; agents also send `"true"`, `"false"`, `1` and `0`.
fn flag(args: &Value, key: &str) -> Option<bool> {
    match args.get(key)? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|n| n != 0),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// The `since` argument as unix seconds; an error for a value it cannot read (never "all").
fn since_arg(args: &Value) -> Result<Option<i64>, String> {
    match text(args, "since") {
        None => Ok(None),
        Some(v) => since(&v).map(Some).ok_or_else(|| {
            format!("Could not read since=\"{v}\": use a date like 2026-11-04 10:00, a unix time or a phrase like `10 minutes ago`")
        }),
    }
}

async fn call(paths: &HearthPaths, method: &str, params: Value) -> Result<Value, String> {
    ipc::call(paths, method, params).await.map_err(readable)
}

/// For calls that may take minutes (installs, workspaces, waiting for mail).
async fn call_long(paths: &HearthPaths, method: &str, params: Value, secs: u64) -> Result<Value, String> {
    ipc::call_waiting(paths, method, params, Duration::from_secs(secs)).await.map_err(readable)
}

/// The site the action is about.
fn which_site(paths: &HearthPaths, args: &Value) -> Result<hearth_core::sites::registry::Site, String> {
    site::resolve(paths, text(args, "site").as_deref(), &start_dir())
}

const NOT_YET: &str = "is not available in Hearth yet";

/// Runs `tool`'s `action` with `args`; the text, or the error text.
pub async fn run(paths: &HearthPaths, tool: &str, action: &str, args: &Value) -> Result<String, String> {
    match tool {
        "site" => site_action(paths, action, args).await,
        "mail" => mail_action(paths, action, args).await,
        "insights" => insights_action(paths, action, args).await,
        "logs" => logs_action(paths, action, args).await,
        "processes" => processes_action(paths, action, args).await,
        "services" => services_action(paths, action, args).await,
        "workspaces" => workspaces_action(paths, action, args).await,
        "runtime" => runtime_action(paths, action, args).await,
        "env" => env_action(paths, action, args).await,
        "db" => db_action(paths, action, args).await,
        _ => Err(format!("`{tool} {action}` {NOT_YET}")),
    }
}

// env ---------------------------------------------------------------------------------------------

async fn env_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    let site = which_site(paths, args)?.name;
    let key = text(args, "key").unwrap_or_default();
    match action {
        "get" if key.is_empty() => Ok(pretty(&call(paths, "env.get", json!({ "site": site })).await?["values"])),
        "get" => {
            let got = call(paths, "env.get", json!({ "site": site, "key": key })).await?;
            match got["value"].as_str() {
                Some(value) => Ok(pretty(&json!({ key: value }))),
                None => Err(format!("{key} is not set in the .env of {site}.test")),
            }
        }
        "set" => {
            // Agents send numbers and true/false as such; `.env` holds them as text.
            let value = match args.get("value") {
                Some(Value::String(s)) => s.clone(),
                Some(v @ (Value::Number(_) | Value::Bool(_))) => v.to_string(),
                _ => return Err(format!("The value of {key} must be text, a number or true/false")),
            };
            call(paths, "env.set", json!({ "site": site, "key": key, "value": value })).await?;
            Ok(format!("{key} is set in the .env of {site}.test"))
        }
        "unset" => {
            call(paths, "env.unset", json!({ "site": site, "key": key })).await?;
            Ok(format!("{key} is removed from the .env of {site}.test"))
        }
        "check" => Ok(pretty(&call(paths, "env.check", json!({ "site": site })).await?)),
        _ => {
            let (category, selection) =
                (text(args, "category").unwrap_or_default(), text(args, "selection").unwrap_or_default());
            let done =
                call(paths, "env.useService", json!({ "site": site, "category": category, "selection": selection }))
                    .await?;
            Ok(pretty(&json!({ "category": category, "selection": selection, "changed": done["changed"] })))
        }
    }
}

// db ----------------------------------------------------------------------------------------------

async fn db_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    let site = which_site(paths, args)?.name;
    let name = text(args, "name");
    match action {
        "connection" | "list" => Ok(pretty(&call(paths, &format!("db.{action}"), json!({ "site": site })).await?)),
        "create" => {
            let made = call(paths, "db.create", json!({ "site": site, "name": name })).await?;
            let name = made["name"].as_str().unwrap_or_default();
            Ok(if made["created"] == true {
                format!("Created the database {name}")
            } else {
                format!("The database {name} already exists")
            })
        }
        "use" => {
            let create = flag(args, "create").unwrap_or(false);
            let used = call(paths, "db.use", json!({ "site": site, "name": name, "create": create })).await?;
            let made = if used["created"] == true { " (created)" } else { "" };
            Ok(format!("{site}.test uses the database {}{made} now", used["name"].as_str().unwrap_or_default()))
        }
        _ => {
            let dropped = call(paths, "db.drop", json!({ "site": site, "name": name })).await?;
            Ok(format!("Dropped the database {}", dropped["name"].as_str().unwrap_or_default()))
        }
    }
}

// site --------------------------------------------------------------------------------------------

/// `8.4` from `8.4.1` or `8.4`: Hearth isolates sites to a minor version.
fn minor_of(version: &str) -> String {
    version.trim().split('.').take(2).collect::<Vec<_>>().join(".")
}

fn url(list: &Value, name: &str, secured: bool) -> String {
    let (scheme, port, default) = if secured {
        ("https", list["httpsPort"].as_u64().unwrap_or(443), 443)
    } else {
        ("http", list["httpPort"].as_u64().unwrap_or(80), 80)
    };
    let port = if port == default { String::new() } else { format!(":{port}") };
    format!("{scheme}://{name}.test{port}")
}

async fn site_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    let list = call(paths, "sites.list", Value::Null).await?;
    match action {
        "list" => {
            let default =
                call(paths, "php.list", Value::Null).await.map(|p| p["default"].clone()).unwrap_or(Value::Null);
            let sites: Vec<Value> = list["sites"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| {
                    let name = s["name"].as_str().unwrap_or_default();
                    json!({
                        "name": name,
                        "url": url(&list, name, s["secured"] == true),
                        "kind": s["kind"],
                        "path": s["path"].as_str().or(s["upstream"].as_str()),
                        "secure": s["secured"],
                        // The version it runs on, as Herd shows it; `phpIsolated` when it is not the default.
                        "php": if s["php"].is_null() { default.clone() } else { s["php"].clone() },
                        "phpIsolated": !s["php"].is_null(),
                    })
                })
                .collect();
            Ok(pretty(&json!(sites)))
        }
        "link" => {
            // A relative folder is relative to where the server runs, not to Hearth's core.
            let dir = match text(args, "path").map(std::path::PathBuf::from) {
                Some(p) if p.is_relative() => start_dir().join(p),
                Some(p) => p,
                None => start_dir(),
            };
            let dir = std::fs::canonicalize(&dir)
                .map(|p| std::path::PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\").to_string()))
                .map_err(|_| format!("{} is not a folder", dir.display()))?;
            let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // `Shop.test` and `shop` are the same site name.
            let name = match text(args, "name") {
                Some(n) => {
                    let lower = n.to_lowercase();
                    lower.strip_suffix(".test").unwrap_or(&lower).to_string()
                }
                None => hearth_core::sites::registry::name_from_folder(&folder)
                    .ok_or("Name the site: the folder's name is no valid site name")?,
            };
            call(paths, "sites.link", json!({ "name": name, "dir": dir })).await?;
            let secure = flag(args, "secure") == Some(true);
            if secure {
                call(paths, "sites.secure", json!({ "name": name })).await?;
            }
            if let Some(v) = text(args, "phpVersion") {
                call(paths, "sites.isolate", json!({ "name": name, "minor": minor_of(&v) })).await?;
            }
            let list = call(paths, "sites.list", Value::Null).await?;
            let address = url(&list, &name, secure);
            // As Herd does: the app's own URL follows the site.
            let env_path = dir.join(".env");
            let env = std::fs::read_to_string(&env_path).unwrap_or_default();
            let note = if hearth_core::workspaces::env_file::get(&env, "APP_URL").is_some() {
                let updated = hearth_core::workspaces::env_file::set(&env, &[("APP_URL", address.as_str())]);
                match hearth_core::atomic::write(&env_path, updated.as_bytes()) {
                    Ok(()) => " (APP_URL in its .env points there now)".to_string(),
                    Err(e) => format!(" (APP_URL could not be updated: {e})"),
                }
            } else {
                String::new()
            };
            Ok(format!("{name} is now served at {address}{note}"))
        }
        "studio" => Err("Hearth has no Studio: open the site's URL in the browser instead".into()),
        "error" => {
            let name = which_site(paths, args)?.name;
            let found = call(paths, "logs.lastException", json!({ "site": name })).await?;
            if found.is_null() {
                Ok(format!("No exception in the log of {name}.test"))
            } else {
                Ok(pretty(&found))
            }
        }
        _ => {
            let site = which_site(paths, args)?;
            let name = site.name.clone();
            match action {
                "info" => {
                    let status = call(paths, "web.status", Value::Null).await.unwrap_or(Value::Null);
                    let php = call(paths, "php.list", Value::Null).await.unwrap_or(Value::Null);
                    let node = match &site.path {
                        Some(dir) => call(paths, "node.which", json!({ "dir": dir })).await.unwrap_or(Value::Null),
                        None => Value::Null,
                    };
                    let workspaces =
                        call(paths, "workspaces.list", json!({ "site": name })).await.unwrap_or(Value::Null);
                    let ws: Vec<Value> = workspaces["workspaces"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(
                            |w| json!({ "site": w["site"], "url": w["url"], "branch": w["branch"], "path": w["path"] }),
                        )
                        .collect();
                    Ok(pretty(&json!({
                        "name": name,
                        "url": url(&list, &name, site.secured),
                        "kind": site.kind,
                        "path": site.path,
                        "docroot": site.docroot,
                        "upstream": site.upstream,
                        "secure": site.secured,
                        "php": site.php.clone().map(Value::from).unwrap_or_else(|| php["default"].clone()),
                        "phpIsolated": site.php.is_some(),
                        "phpDefault": php["default"],
                        "node": node["version"],
                        "debug": site.debug,
                        "problem": site.problem,
                        "note": status["notes"][&name],
                        "workspaces": ws,
                    })))
                }
                "secure" => {
                    call(paths, "sites.secure", json!({ "name": name })).await?;
                    Ok(format!("{name} is now served at {}", url(&list, &name, true)))
                }
                "unsecure" => {
                    call(paths, "sites.unsecure", json!({ "name": name })).await?;
                    Ok(format!("{name} is now served at {}", url(&list, &name, false)))
                }
                "isolate" => {
                    let v = minor_of(&text(args, "phpVersion").unwrap_or_default());
                    call(paths, "sites.isolate", json!({ "name": name, "minor": v })).await?;
                    Ok(format!("{name} now uses PHP {v}"))
                }
                "unisolate" => {
                    call(paths, "sites.isolate", json!({ "name": name, "minor": null })).await?;
                    Ok(format!("{name} now uses the default PHP"))
                }
                _ => Err(format!("`site {action}` {NOT_YET}")),
            }
        }
    }
}

// mail --------------------------------------------------------------------------------------------

/// Now, in unix seconds.
fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `2026-11-04 10:01:00` for a unix time (UTC).
fn datetime(secs: i64) -> String {
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// A point in time from `since`: unix seconds, `YYYY-MM-DD[ HH:MM[:SS]]` (UTC) or `N minutes ago`.
fn since(value: &str) -> Option<i64> {
    let v = value.trim().to_lowercase();
    if let Ok(n) = v.parse::<i64>() {
        return Some(n);
    }
    if let Some(rest) = v.strip_suffix(" ago") {
        let mut parts = rest.split_whitespace();
        let n: i64 = match parts.next()? {
            "a" | "an" | "one" => 1,
            n => n.parse().ok()?,
        };
        let unit = parts.next()?.trim_end_matches('s');
        let secs = match unit {
            "second" | "sec" => 1,
            "minute" | "min" => 60,
            "hour" => 3600,
            "day" => 86_400,
            "week" => 604_800,
            _ => return None,
        };
        return Some(now() - n * secs);
    }
    let (date, time) = v.split_once([' ', 't']).unwrap_or((&v, "00:00:00"));
    // An offset (`+01:00`, `-0500`, `z`) shifts the time to UTC.
    let time = time.trim_end_matches('z');
    let (time, offset) = match time.rfind(['+', '-']) {
        Some(i) => {
            let (sign, digits) = (if &time[i..=i] == "-" { -1 } else { 1 }, time[i + 1..].replace(':', ""));
            let hm: i64 = digits.parse().ok()?;
            (&time[..i], sign * ((hm / 100) * 3600 + (hm % 100) * 60))
        }
        None => (time, 0),
    };
    let d: Vec<i64> = date.split('-').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let t: Vec<i64> = time.split(':').map(|p| p.split('.').next().unwrap_or(p).parse().ok()).collect::<Option<_>>()?;
    if d.len() != 3 || t.len() > 3 {
        return None;
    }
    let (h, mi, s) = (t.first().copied().unwrap_or(0), t.get(1).copied().unwrap_or(0), t.get(2).copied().unwrap_or(0));
    Some(days_from_civil(d[0], d[1], d[2]) * 86_400 + h * 3600 + mi * 60 + s - offset)
}

fn contains(haystack: &str, needle: &Option<String>) -> bool {
    needle.as_ref().is_none_or(|n| haystack.to_lowercase().contains(&n.to_lowercase()))
}

/// The mails matching the list filters (mailbox, to, from, subject, since).
fn matching(mails: &Value, args: &Value) -> Result<Vec<Value>, String> {
    let mailbox = text(args, "mailbox");
    let (to, from, subject) = (text(args, "to"), text(args, "from"), text(args, "subject"));
    let after = since_arg(args)?;
    Ok(mails
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            let inbox = m["site"].as_str().unwrap_or("Default");
            mailbox.as_ref().is_none_or(|b| b.eq_ignore_ascii_case(inbox))
                && contains(
                    &m["to"]
                        .as_array()
                        .map(|t| t.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
                        .unwrap_or_default(),
                    &to,
                )
                && contains(m["from"].as_str().unwrap_or_default(), &from)
                && contains(m["subject"].as_str().unwrap_or_default(), &subject)
                && after.is_none_or(|a| m["received"].as_i64().unwrap_or(0) >= a)
        })
        .cloned()
        .collect())
}

fn summary(m: &Value) -> Value {
    json!({
        "id": m["id"],
        "date": datetime(m["received"].as_i64().unwrap_or(0)),
        "mailbox": m["site"].as_str().unwrap_or("Default"),
        "subject": m["subject"],
        "from": m["from"],
        "to": m["to"],
        "attachments": m["attachments"],
    })
}

/// A mail in the asked format: text (headers and the text body), html, raw or json.
async fn show(paths: &HearthPaths, id: &str, format: &str) -> Result<String, String> {
    match format {
        "raw" => {
            Ok(call(paths, "mail.raw", json!({ "id": id })).await?["text"].as_str().unwrap_or_default().to_string())
        }
        "json" => Ok(pretty(&call(paths, "mail.get", json!({ "id": id })).await?)),
        "html" => {
            let mail = call(paths, "mail.get", json!({ "id": id })).await?;
            Ok(mail["html"].as_str().unwrap_or("(This mail has no HTML part.)").to_string())
        }
        _ => {
            let m = call(paths, "mail.get", json!({ "id": id })).await?;
            let join = |v: &Value| {
                v.as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            };
            let mut out = format!(
                "Id: {}\nDate: {}\nMailbox: {}\nFrom: {}\nTo: {}\nSubject: {}\n",
                m["id"].as_str().unwrap_or_default(),
                datetime(m["received"].as_i64().unwrap_or(0)),
                m["site"].as_str().unwrap_or("Default"),
                m["from"].as_str().unwrap_or_default(),
                join(&m["to"]),
                m["subject"].as_str().unwrap_or_default()
            );
            if m["cc"].as_array().is_some_and(|c| !c.is_empty()) {
                out.push_str(&format!("Cc: {}\n", join(&m["cc"])));
            }
            for f in m["files"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "Attachment: {} ({}, {} bytes)\n",
                    f["name"].as_str().unwrap_or_default(),
                    f["contentType"].as_str().unwrap_or_default(),
                    f["size"]
                ));
            }
            out.push('\n');
            out.push_str(m["text"].as_str().unwrap_or_default());
            Ok(out)
        }
    }
}

/// Every http(s) link in a text, in order, once each.
fn links(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("http") {
        let tail = &rest[start..];
        if tail.starts_with("http://") || tail.starts_with("https://") {
            let end = tail
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']'))
                .unwrap_or(tail.len());
            let link = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']).replace("&amp;", "&");
            if !found.contains(&link) {
                found.push(link);
            }
            rest = &tail[end..];
        } else {
            rest = &tail[4..];
        }
    }
    found
}

async fn mail_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    match action {
        "list" => {
            let mails = call(paths, "mail.list", Value::Null).await?;
            let limit = number(args, "limit").unwrap_or(50).clamp(1, 1000) as usize;
            let rows: Vec<Value> = matching(&mails, args)?.iter().take(limit).map(summary).collect();
            Ok(pretty(&json!(rows)))
        }
        "show" => {
            show(paths, &text(args, "id").unwrap_or_default(), text(args, "format").as_deref().unwrap_or("text")).await
        }
        "links" => {
            let m = call(paths, "mail.get", json!({ "id": text(args, "id") })).await?;
            let body =
                format!("{}\n{}", m["html"].as_str().unwrap_or_default(), m["text"].as_str().unwrap_or_default());
            Ok(pretty(&json!(links(&body))))
        }
        "wait" => {
            // No catcher, no mail: say so now instead of after the whole timeout.
            let status = call(paths, "mail.status", Value::Null).await?;
            if status["listening"] != true {
                let why = status["error"].as_str().map(|e| format!(": {e}")).unwrap_or_default();
                return Err(format!("Hearth's mail catcher is not running{why}. Check the Mail page."));
            }
            let timeout = number(args, "timeout").unwrap_or(30).clamp(1, 300);
            let known: Vec<String> = call(paths, "mail.list", Value::Null)
                .await?
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect();
            let deadline = std::time::Instant::now() + Duration::from_secs(timeout as u64);
            loop {
                let mails = call(paths, "mail.list", Value::Null).await?;
                let fresh = matching(&mails, args)?
                    .into_iter()
                    .find(|m| m["id"].as_str().is_some_and(|id| !known.iter().any(|k| k == id)));
                if let Some(m) = fresh {
                    let id = m["id"].as_str().unwrap_or_default().to_string();
                    return show(paths, &id, text(args, "format").as_deref().unwrap_or("text")).await;
                }
                if std::time::Instant::now() >= deadline {
                    return Err(format!("No matching mail arrived within {timeout} seconds"));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        "clear" => {
            let params = match text(args, "mailbox") {
                Some(b) if b.eq_ignore_ascii_case("default") => json!({ "site": null }),
                Some(b) => {
                    // The inbox's own spelling: `list` matches any case, the store exactly.
                    let mails = call(paths, "mail.list", Value::Null).await?;
                    let inbox = mails
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|m| m["site"].as_str())
                        .find(|s| s.eq_ignore_ascii_case(&b))
                        .ok_or_else(|| format!("There is no mailbox {b}: call `mail list` to see them"))?
                        .to_string();
                    json!({ "site": inbox })
                }
                None => json!({}),
            };
            call(paths, "mail.clear", params).await?;
            Ok("Done.".into())
        }
        "config" => Ok(pretty(&call(paths, "mail.status", Value::Null).await?)),
        _ => Err(format!("`mail {action}` {NOT_YET}")),
    }
}

// insights ----------------------------------------------------------------------------------------

async fn insights_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    let all = flag(args, "allSites") == Some(true);
    let site = if all || matches!(action, "capture" | "clear" | "debug_start" | "debug_stop") {
        None
    } else {
        Some(which_site(paths, args)?.name)
    };
    let mine = |v: &Value| {
        site.as_ref().is_none_or(|s| {
            v["site"]
                .as_str()
                .is_some_and(|x| x.eq_ignore_ascii_case(s) || x.eq_ignore_ascii_case(&format!("{s}.test")))
        })
    };
    match action {
        "list" => {
            let limit = number(args, "limit").unwrap_or(50).clamp(1, 200) as usize;
            let kind = text(args, "type");
            let mut rows: Vec<(i64, String)> = Vec::new();
            if kind.as_deref().is_none_or(|k| k == "dump") {
                for d in call(paths, "dumps.list", json!({})).await?.as_array().into_iter().flatten().filter(|d| mine(d)) {
                    let at = match (d["file"].as_str(), d["line"].as_u64()) {
                        (Some(f), Some(l)) => format!(" at {f}:{l}"),
                        _ => String::new(),
                    };
                    rows.push((d["time"].as_i64().unwrap_or(0), format!("dump{at}\n{}", d["text"].as_str().unwrap_or_default())));
                }
            }
            if kind.as_deref().is_none_or(|k| matches!(k, "request" | "query")) {
                for r in call(paths, "requests.list", json!({ "after": 0 })).await?.as_array().into_iter().flatten().filter(|r| mine(r)) {
                    let what = match r["command"].as_str() {
                        Some(c) => format!("artisan {c}"),
                        None => format!("{} {}", r["method"].as_str().unwrap_or_default(), r["uri"].as_str().unwrap_or_default()),
                    };
                    let mut line = format!(
                        "request #{} {what} -> {} in {:.0} ms, {} queries ({:.1} ms)",
                        r["id"], r["status"], r["ms"].as_f64().unwrap_or(0.0), r["queryCount"], r["queryMs"].as_f64().unwrap_or(0.0)
                    );
                    if kind.as_deref() == Some("query") {
                        let detail = call(paths, "requests.get", json!({ "id": r["id"] })).await?;
                        for q in detail["queries"].as_array().into_iter().flatten() {
                            line.push_str(&format!("\n  {:.1} ms  {}", q["ms"].as_f64().unwrap_or(0.0), q["sql"].as_str().unwrap_or_default()));
                        }
                    }
                    rows.push((r["time"].as_i64().unwrap_or(0), line));
                }
            }
            rows.sort_by_key(|(t, _)| *t);
            let start = rows.len().saturating_sub(limit);
            if rows.is_empty() {
                return Ok("Nothing captured yet. Dumps and requests show up here once the site runs with capturing on (`insights capture`).".into());
            }
            Ok(rows[start..].iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n\n"))
        }
        "analyze" => {
            let after = if flag(args, "allRequests") == Some(true) { 0 } else { number(args, "after").unwrap_or(0).max(0) };
            let limit = number(args, "limit").unwrap_or(20).clamp(1, 200) as usize;
            let summaries = call(paths, "requests.list", json!({ "after": after })).await?;
            let mut out = Vec::new();
            let mut cursor = after;
            for r in summaries.as_array().into_iter().flatten() {
                cursor = cursor.max(r["id"].as_i64().unwrap_or(0));
                if !mine(r) || r["findings"].as_array().is_none_or(Vec::is_empty) || out.len() >= limit {
                    continue;
                }
                let detail = call(paths, "requests.get", json!({ "id": r["id"] })).await?;
                let what = detail["command"].as_str().map(|c| format!("artisan {c}")).unwrap_or_else(|| format!("{} {}", detail["method"].as_str().unwrap_or_default(), detail["uri"].as_str().unwrap_or_default()));
                for f in detail["findings"].as_array().into_iter().flatten() {
                    let first = f["queries"].as_array().and_then(|q| q.first()).and_then(Value::as_u64).map(|i| &detail["queries"][i as usize]);
                    let place = first.and_then(|q| Some(format!(" ({}:{})", q["file"].as_str()?, q["line"])))
                        .unwrap_or_default();
                    let mut entry = format!("[{}] {}{place} in request #{} {what}", f["kind"].as_str().unwrap_or_default(), f["text"].as_str().unwrap_or_default(), detail["id"]);
                    if flag(args, "examples") == Some(true) {
                        if let Some(q) = first {
                            entry.push_str(&format!("\n  e.g. {} {:?}", q["sql"].as_str().unwrap_or_default(), q["bindings"]));
                        }
                    }
                    out.push(entry);
                }
            }
            let head = if out.is_empty() { "No N+1, duplicate or slow queries found.".to_string() } else { out.join("\n") };
            Ok(format!("{head}\n\nCursor: {cursor} (pass it as `after` next time)"))
        }
        "clear" => {
            call(paths, "dumps.clear", Value::Null).await?;
            call(paths, "requests.clear", Value::Null).await?;
            Ok("Cleared every site's dumps and requests (Hearth keeps them together).".into())
        }
        "capture" => {
            let on = flag(args, "enabled").unwrap_or(false);
            call(paths, "dumps.enable", json!({ "on": on })).await?;
            call(paths, "requests.enable", json!({ "on": on })).await?;
            Ok(format!("Capturing dumps and requests is {}", if on { "on" } else { "off" }))
        }
        _ => Err("Hearth has no debug sessions: it captures dumps and requests all the time. Use `insights list` and `insights analyze`.".into()),
    }
}

// logs --------------------------------------------------------------------------------------------

async fn logs_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    let site = which_site(paths, args)?.name;
    let sites = call(paths, "logs.sites", Value::Null).await?;
    let files = sites
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["site"] == site.as_str())
        .map(|s| s["files"].clone())
        .unwrap_or(json!([]));
    match action {
        "files" => Ok(pretty(&files)),
        _ => {
            let names: Vec<String> =
                files.as_array().into_iter().flatten().filter_map(|f| f["name"].as_str().map(str::to_string)).collect();
            let file = text(args, "file")
                .or_else(|| names.iter().find(|n| *n == "laravel.log").cloned())
                .or_else(|| names.first().cloned())
                .ok_or_else(|| format!("{site} has no log files yet"))?;
            let read = call(paths, "logs.read", json!({ "site": site, "file": file })).await?;
            let levels: Option<Vec<String>> =
                text(args, "levels").map(|l| l.split(',').map(|x| x.trim().to_lowercase()).collect());
            let search = text(args, "search");
            let after = since_arg(args)?.map(datetime);
            let limit = number(args, "limit").unwrap_or(20).clamp(1, 500) as usize;
            let mut entries: Vec<Value> = read["entries"].as_array().cloned().unwrap_or_default();
            entries.reverse();
            let picked: Vec<Value> = entries
                .into_iter()
                .filter(|e| levels.as_ref().is_none_or(|l| l.iter().any(|x| e["level"].as_str() == Some(x))))
                .filter(|e| {
                    contains(
                        &format!(
                            "{} {}",
                            e["message"].as_str().unwrap_or_default(),
                            e["detail"].as_str().unwrap_or_default()
                        ),
                        &search,
                    )
                })
                .filter(|e| after.as_ref().is_none_or(|a| e["time"].as_str().is_some_and(|t| t >= a.as_str())))
                .take(limit)
                .collect();
            Ok(pretty(&json!(picked)))
        }
    }
}

// processes ---------------------------------------------------------------------------------------

async fn processes_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    let site = which_site(paths, args)?.name;
    let name = text(args, "process").unwrap_or_default();
    match action {
        "list" => {
            let all = call(paths, "processes.list", Value::Null).await?;
            let mine = all["sites"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|s| s["site"] == site.as_str())
                .map(|s| s["processes"].clone());
            match mine {
                Some(p) => Ok(pretty(&p)),
                None => Ok(format!("{site} has no processes yet. Add them on Hearth's Processes page.")),
            }
        }
        "start" | "stop" => {
            call_long(paths, &format!("processes.{action}"), json!({ "site": site, "name": name }), 60).await?;
            Ok(format!("{site} {name} {}", if action == "start" { "runs" } else { "is stopped" }))
        }
        _ => {
            let out = call(paths, "processes.output", json!({ "site": site, "name": name })).await?;
            let lines = number(args, "lines").unwrap_or(50).clamp(1, 500) as usize;
            let text = out["text"].as_str().unwrap_or_default();
            let all: Vec<&str> = text.lines().collect();
            Ok(all[all.len().saturating_sub(lines)..].join("\n"))
        }
    }
}

// services ----------------------------------------------------------------------------------------

async fn service_id(paths: &HearthPaths, wanted: &str) -> Result<String, String> {
    let list = call(paths, "services.list", Value::Null).await?;
    list.as_array()
        .into_iter()
        .flatten()
        .find(|s| s["id"] == wanted || s["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(wanted)))
        .and_then(|s| s["id"].as_str().map(str::to_string))
        .ok_or_else(|| format!("There is no service {wanted}. Call `services list`."))
}

async fn services_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    match action {
        "list" => Ok(pretty(&call(paths, "services.list", Value::Null).await?)),
        "available" => Ok(pretty(&call(paths, "services.available", Value::Null).await?)),
        "install" => {
            let kind = text(args, "type").unwrap_or_default().to_lowercase();
            // A version names its line (`8.4.11` and `8.4` are both the 8.4 line); without one,
            // the catalog's newest line of the type.
            let line = match text(args, "version") {
                Some(v) => {
                    let parts: Vec<&str> = v.split('.').collect();
                    if parts.len() >= 3 {
                        parts[..2].join(".")
                    } else {
                        v
                    }
                }
                None => {
                    let available = call(paths, "services.available", Value::Null).await?;
                    available["builds"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|b| b["kind"] == kind.as_str())
                        .and_then(|b| b["line"].as_str().map(str::to_string))
                        .ok_or_else(|| format!("Hearth has no {kind} to install. Call `services available`."))?
                }
            };
            let mut params = json!({ "kind": kind, "line": line });
            if let Some(port) = number(args, "port") {
                params["port"] = json!(port);
            }
            if let Some(name) = text(args, "name") {
                params["name"] = json!(name);
            }
            let made = call_long(paths, "services.create", params, 600).await?;
            // Created is not running: the first start downloads the build, which takes a while.
            let id = made["id"].as_str().unwrap_or_default().to_string();
            call_long(paths, "services.start", json!({ "id": id }), 900).await?;
            let list = call(paths, "services.list", Value::Null).await?;
            let row = list.as_array().into_iter().flatten().find(|s| s["id"] == id.as_str()).cloned();
            Ok(pretty(&row.unwrap_or(made)))
        }
        _ => {
            let id = service_id(paths, &text(args, "service").unwrap_or_default()).await?;
            // A first start downloads the build.
            call_long(paths, &format!("services.{action}"), json!({ "id": id }), 900).await?;
            Ok(format!("{id} {}", if action == "start" { "runs" } else { "is stopped" }))
        }
    }
}

// workspaces --------------------------------------------------------------------------------------

async fn workspaces_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    match action {
        "list" => {
            let params = if flag(args, "allSites") == Some(true) {
                json!({})
            } else {
                json!({ "site": which_site(paths, args)?.name })
            };
            Ok(pretty(&call(paths, "workspaces.list", params).await?["workspaces"]))
        }
        "create" => {
            let site = which_site(paths, args)?.name;
            let mut params = json!({ "site": site, "name": text(args, "name"), "database": text(args, "database"), "migrate": flag(args, "migrate").unwrap_or(true), "by": "mcp" });
            if let Some(secure) = flag(args, "secure") {
                params["secure"] = json!(secure);
            }
            let made = call_long(paths, "workspaces.create", params, 900).await?;
            Ok(pretty(&json!({ "workspace": made["workspace"], "notes": made["notes"] })))
        }
        "delete" => {
            let done = call_long(
                paths,
                "workspaces.delete",
                json!({ "name": text(args, "name"), "keepDatabase": flag(args, "keepDatabase").unwrap_or(false) }),
                300,
            )
            .await?;
            Ok(pretty(&json!({ "deleted": text(args, "name"), "notes": done["notes"] })))
        }
        _ => {
            let done = call_long(
                paths,
                "workspaces.merge",
                json!({ "name": text(args, "name"), "delete": flag(args, "delete").unwrap_or(false) }),
                300,
            )
            .await?;
            Ok(pretty(&done))
        }
    }
}

// runtime -----------------------------------------------------------------------------------------

/// Waits for a core job (an install) to finish; its error if it failed.
async fn finish_job(paths: &HearthPaths, started: &Value) -> Result<(), String> {
    let id = started["jobId"].clone();
    let deadline = std::time::Instant::now() + Duration::from_secs(900);
    loop {
        let job = call(paths, "jobs.get", json!({ "id": id })).await?;
        match job["state"].as_str() {
            Some("done") => return Ok(()),
            Some("failed") => return Err(job["error"].as_str().unwrap_or("The install failed").to_string()),
            _ if std::time::Instant::now() > deadline => {
                return Err("The install is still running: check Hearth's window".into())
            }
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}

async fn runtime_action(paths: &HearthPaths, action: &str, args: &Value) -> Result<String, String> {
    match action {
        "status" => {
            let core = call(paths, "core.status", Value::Null).await?;
            let web = call(paths, "web.status", Value::Null).await.unwrap_or(Value::Null);
            let php = call(paths, "php.list", Value::Null).await.unwrap_or(Value::Null);
            let node = call(paths, "node.list", Value::Null).await.unwrap_or(Value::Null);
            let mail = call(paths, "mail.status", Value::Null).await.unwrap_or(Value::Null);
            let dumps = call(paths, "dumps.status", Value::Null).await.unwrap_or(Value::Null);
            let services = call(paths, "services.list", Value::Null).await.unwrap_or(Value::Null);
            Ok(pretty(&json!({
                "version": core["version"],
                "uptimeSecs": core["uptimeSecs"],
                "php": php["default"],
                "node": node["default"],
                "web": { "running": web["running"], "httpPort": web["httpPort"], "httpsPort": web["httpsPort"], "error": web["error"] },
                "mail": mail,
                "collector": dumps,
                "services": services,
            })))
        }
        "php_versions" => {
            let installed = call(paths, "php.list", Value::Null).await?;
            let available = call(paths, "php.available", Value::Null).await.unwrap_or(Value::Null);
            Ok(pretty(&json!({ "installed": installed, "available": available })))
        }
        "php_install" => {
            let v = text(args, "version").unwrap_or_default();
            let minor: String = v.split('.').take(2).collect::<Vec<_>>().join(".");
            let started = call(paths, "php.install", json!({ "minor": minor })).await?;
            finish_job(paths, &started).await?;
            Ok(format!("PHP {minor} is installed"))
        }
        "node_versions" => {
            let installed = call(paths, "node.list", Value::Null).await?;
            let available = call(paths, "node.available", Value::Null).await.unwrap_or(Value::Null);
            Ok(pretty(&json!({ "installed": installed, "available": available })))
        }
        _ => {
            let v = text(args, "version").unwrap_or_default();
            let started = call(paths, "node.install", json!({ "spec": v })).await?;
            finish_job(paths, &started).await?;
            Ok(format!("Node {} is installed", started["version"].as_str().unwrap_or(&v)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{datetime, links, since};

    #[test]
    fn dates_and_links() {
        assert_eq!(datetime(0), "1970-01-01 00:00:00");
        assert_eq!(datetime(1_793_786_400), "2026-11-04 10:00:00");
        assert_eq!(since("2026-11-04 10:00"), Some(1_793_786_400));
        assert_eq!(since("1793786400"), Some(1_793_786_400));
        assert!(since("10 minutes ago").is_some_and(|t| t > 1_700_000_000));
        assert_eq!(since("someday"), None);
        assert_eq!(
            links("Go to https://a.test/x?y=1&amp;z=2. Or <a href=\"http://b.test\">here</a>, https://a.test/x?y=1&amp;z=2"),
            ["https://a.test/x?y=1&z=2", "http://b.test"]
        );
    }
}
