//! A site's `.env`: read, change and check it, and point a category (database, queue, cache,
//! broadcasting) at a Stoke service. The lines a category replaced are kept in
//! `config/env-originals.json` until `unmanaged` puts them back.

pub mod api;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dotenv;
use crate::services::{kinds, store};
use crate::sites::registry::{self, SiteKind};
use crate::{CoreError, StokePaths};

fn error(message: impl Into<String>) -> CoreError {
    CoreError::Php(message.into())
}

/// The `.env` of site `name` (`<site folder>/.env`).
pub fn file_of(paths: &StokePaths, name: &str) -> Result<PathBuf, CoreError> {
    let site = registry::list(paths)
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| error(format!("No site named {name}.test")))?;
    match (&site.kind, site.path) {
        (SiteKind::Proxy, _) | (_, None) => Err(error(format!("{name}.test is a proxy and has no .env"))),
        (_, Some(path)) => Ok(path.join(".env")),
    }
}

fn read(paths: &StokePaths, name: &str) -> Result<(PathBuf, String), CoreError> {
    let file = file_of(paths, name)?;
    match std::fs::read_to_string(&file) {
        Ok(text) => Ok((file, text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(error(format!("{name}.test has no .env file"))),
        Err(source) => Err(CoreError::Io { path: file, source }),
    }
}

/// Every key and value of the site's `.env`, in file order.
pub fn values(paths: &StokePaths, name: &str) -> Result<Vec<(String, String)>, CoreError> {
    Ok(dotenv::all(&read(paths, name)?.1))
}

pub fn get(paths: &StokePaths, name: &str, key: &str) -> Result<Option<String>, CoreError> {
    Ok(dotenv::get(&read(paths, name)?.1, key))
}

fn checked(key: &str) -> Result<(), CoreError> {
    if dotenv::valid_key(key) {
        Ok(())
    } else {
        Err(error(format!("[{key}] is not a .env key")))
    }
}

/// Sets `key` (every line of it); a site without a `.env` gets one.
pub fn set(paths: &StokePaths, name: &str, key: &str, value: &str) -> Result<(), CoreError> {
    checked(key)?;
    if value.contains(['\r', '\n']) {
        return Err(error(format!("The value of {key} has a line break: .env values are one line")));
    }
    let file = file_of(paths, name)?;
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    crate::atomic::write(&file, dotenv::set(&text, &[(key, value)]).as_bytes())
}

pub fn unset(paths: &StokePaths, name: &str, key: &str) -> Result<(), CoreError> {
    checked(key)?;
    let (file, text) = read(paths, name)?;
    let next = dotenv::unset(&text, key);
    if next != text {
        crate::atomic::write(&file, next.as_bytes())?;
    }
    Ok(())
}

/// Keys `.env.example` has and `.env` lacks, and keys only `.env` has. Both sorted.
pub fn check(paths: &StokePaths, name: &str) -> Result<(Vec<String>, Vec<String>), CoreError> {
    let (file, text) = read(paths, name)?;
    let example = file.with_file_name(".env.example");
    let example = std::fs::read_to_string(&example).map_err(|_| error(format!("{name}.test has no .env.example")))?;
    let keys = |t: &str| dotenv::all(t).into_iter().map(|(k, _)| k).collect::<std::collections::BTreeSet<_>>();
    let (have, want) = (keys(&text), keys(&example));
    Ok((want.difference(&have).cloned().collect(), have.difference(&want).cloned().collect()))
}

/// What one category changed: the lines it replaced (`None`: the key was not there) and the keys
/// it last wrote.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Managed {
    selection: String,
    originals: BTreeMap<String, Option<String>>,
    wrote: Vec<String>,
}

/// site -> category -> what it changed.
type Originals = BTreeMap<String, BTreeMap<String, Managed>>;

/// One load-change-save of env-originals.json at a time.
static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn originals_file(paths: &StokePaths) -> PathBuf {
    paths.config.join("env-originals.json")
}

fn load_originals(paths: &StokePaths) -> Originals {
    std::fs::read(originals_file(paths)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_originals(paths: &StokePaths, all: &Originals) -> Result<(), CoreError> {
    let bytes = serde_json::to_vec_pretty(all).map_err(|e| error(e.to_string()))?;
    crate::atomic::write(&originals_file(paths), &bytes)
}

pub const CATEGORIES: [&str; 4] = ["database", "queue", "cache", "broadcasting"];

/// The service `selection` names: its id, its name or its kind (the first of that kind).
fn service(paths: &StokePaths, selection: &str) -> Option<store::Instance> {
    let all = store::load(paths);
    let wanted = selection.to_lowercase();
    all.iter()
        .find(|i| i.id == wanted || i.name.to_lowercase() == wanted)
        .or_else(|| all.iter().find(|i| i.kind == wanted))
        .cloned()
}

/// A database name from a site name: `my-shop` gives `my_shop`.
fn database_name(site: &str) -> String {
    site.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect()
}

/// The lines `selection` sets for `category` in `text`.
fn lines_for(
    paths: &StokePaths,
    category: &str,
    selection: &str,
    text: &str,
) -> Result<Vec<(String, String)>, CoreError> {
    let plain = |keys: &[&str], value: &str| -> Vec<(String, String)> {
        keys.iter().map(|k| (k.to_string(), value.to_string())).collect()
    };
    // A second key (CACHE_DRIVER, BROADCAST_DRIVER) changes along only when the file has it.
    let with_legacy = |key: &str, legacy: &str, value: &str| {
        let mut keys = vec![key];
        if dotenv::get(text, legacy).is_some() {
            keys.push(legacy);
        }
        plain(&keys, value)
    };
    let redis = |key: &str, legacy: Option<&str>| -> Option<Vec<(String, String)>> {
        let inst = service(paths, selection).filter(|i| i.kind == "redis")?;
        let mut lines = match legacy {
            Some(l) => with_legacy(key, l, "redis"),
            None => plain(&[key], "redis"),
        };
        lines.extend(kinds::env(&inst));
        Some(lines)
    };
    let choice = selection.to_lowercase();
    let lines = match category {
        "database" if choice == "sqlite" => Some(plain(&["DB_CONNECTION"], "sqlite")),
        "database" => service(paths, selection)
            .filter(|i| matches!(i.kind.as_str(), "mysql" | "mariadb" | "postgresql"))
            .map(|i| kinds::env(&i)),
        "queue" if matches!(choice.as_str(), "sync" | "database") => Some(plain(&["QUEUE_CONNECTION"], &choice)),
        "queue" => redis("QUEUE_CONNECTION", None),
        "cache" if matches!(choice.as_str(), "file" | "database" | "array") => {
            Some(with_legacy("CACHE_STORE", "CACHE_DRIVER", &choice))
        }
        "cache" => redis("CACHE_STORE", Some("CACHE_DRIVER")),
        "broadcasting" if matches!(choice.as_str(), "log" | "null" | "reverb" | "pusher") => {
            Some(with_legacy("BROADCAST_CONNECTION", "BROADCAST_DRIVER", &choice))
        }
        "broadcasting" => None,
        _ => {
            return Err(error(format!("Unknown category [{category}]. Use one of: {}.", CATEGORIES.join(", "))));
        }
    };
    lines.ok_or_else(|| {
        error(format!(
            "[{selection}] is not something {category} can use. Call `services list` for the services Stoke runs."
        ))
    })
}

/// Points `category` of the site's `.env` at `selection`, or with `unmanaged` puts back the lines
/// the category replaced. Returns the keys changed (`None`: removed).
pub fn use_service(
    paths: &StokePaths,
    name: &str,
    category: &str,
    selection: &str,
) -> Result<BTreeMap<String, Option<String>>, CoreError> {
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let (file, text) = read(paths, name)?;
    let mut all = load_originals(paths);
    let site = all.entry(name.to_string()).or_default();
    let mut changed = BTreeMap::new();
    let next = if selection.eq_ignore_ascii_case("unmanaged") {
        if !CATEGORIES.contains(&category) {
            return Err(error(format!("Unknown category [{category}]. Use one of: {}.", CATEGORIES.join(", "))));
        }
        let managed =
            site.remove(category).ok_or_else(|| error(format!("{name}.test has nothing to restore for {category}")))?;
        let mut next = text.clone();
        for (key, original) in managed.originals {
            // A key another category still writes (REDIS_* for queue and cache) moves to it.
            if let Some(other) = site.values_mut().find(|m| m.wrote.contains(&key)) {
                other.originals.insert(key, original);
                continue;
            }
            next = match &original {
                // A line Stoke commented out goes back where it was.
                Some(line) if dotenv::line_of(&next, &key).is_none() => dotenv::uncomment(&next, line),
                Some(line) => dotenv::put_line(&next, &key, line, false),
                None => dotenv::unset(&next, &key),
            };
            changed.insert(key.clone(), original.as_ref().and_then(|_| dotenv::get(&next, &key)));
        }
        next
    } else {
        let mut lines = lines_for(paths, category, selection, &text)?;
        // The database name follows the driver: a server needs one (Laravel's `laravel` rarely
        // exists), and SQLite reads `DB_DATABASE` as a file path, so its line is commented out.
        let database = dotenv::get(&text, "DB_DATABASE").filter(|v| !v.is_empty());
        let sqlite = category == "database" && selection.eq_ignore_ascii_case("sqlite");
        if category == "database" && !sqlite && database.is_none() {
            lines.push(("DB_DATABASE".into(), database_name(name)));
        }
        let recorded: Vec<String> = site.values().flat_map(|m| m.originals.keys().cloned()).collect();
        let managed = site.entry(category.to_string()).or_default();
        managed.selection = selection.to_string();
        managed.wrote = lines.iter().map(|(k, _)| k.clone()).collect();
        let mut removed = vec![];
        if sqlite && dotenv::line_of(&text, "DB_DATABASE").is_some() {
            removed.push("DB_DATABASE".to_string());
            managed.wrote.push("DB_DATABASE".into());
        }
        for key in lines.iter().map(|(k, _)| k).chain(&removed) {
            if !recorded.contains(key) {
                managed.originals.insert(key.clone(), dotenv::line_of(&text, key));
            }
        }
        let pairs: Vec<(&str, &str)> = lines.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        changed.extend(lines.iter().map(|(k, v)| (k.clone(), Some(v.clone()))));
        let mut next = dotenv::set(&text, &pairs);
        for key in removed {
            next = dotenv::comment_out(&next, &key);
            changed.insert(key, None);
        }
        if sqlite {
            // Laravel's default file, so the site works right away.
            if let Some(folder) = file.parent().map(|p| p.join("database")) {
                let db = folder.join("database.sqlite");
                if !db.exists() {
                    std::fs::create_dir_all(&folder).map_err(|e| CoreError::io(&folder, e))?;
                    std::fs::write(&db, b"").map_err(|e| CoreError::io(&db, e))?;
                }
            }
        }
        next
    };
    if site.is_empty() {
        all.remove(name);
    }
    if next != text {
        crate::atomic::write(&file, next.as_bytes())?;
    }
    save_originals(paths, &all)?;
    Ok(changed)
}

#[cfg(test)]
mod tests;
