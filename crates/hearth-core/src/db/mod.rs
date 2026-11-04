//! Databases on the server a site's `.env` names: MySQL/MariaDB and Postgres through their own
//! client programs (a Hearth service's on the same port first, else the ones on PATH), SQLite as
//! files. Used by the db.* methods and by workspaces.

pub mod api;

use std::path::PathBuf;
use std::process::Stdio;

use crate::dotenv;
use crate::{CoreError, HearthPaths};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    Mysql,
    Pgsql,
    Sqlite,
}

impl Driver {
    pub fn name(self) -> &'static str {
        match self {
            Driver::Mysql => "mysql",
            Driver::Pgsql => "pgsql",
            Driver::Sqlite => "sqlite",
        }
    }
}

/// A server connection from a `.env`.
pub struct Conn {
    pub driver: Driver,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}

/// The driver of a `.env` (Laravel's default is SQLite), or the name of one Hearth cannot handle.
pub fn driver(env: &str) -> Result<Driver, String> {
    let name = dotenv::get(env, "DB_CONNECTION").unwrap_or_else(|| "sqlite".into());
    match name.to_lowercase().as_str() {
        "mysql" | "mariadb" => Ok(Driver::Mysql),
        "pgsql" | "postgres" | "postgresql" => Ok(Driver::Pgsql),
        "sqlite" => Ok(Driver::Sqlite),
        _ => Err(name),
    }
}

/// The server connection of a `.env`; `database` is empty when `DB_DATABASE` is.
pub fn conn(env: &str, driver: Driver) -> Conn {
    let get = |k: &str| dotenv::get(env, k).filter(|v| !v.is_empty());
    let default_port = if driver == Driver::Pgsql { 5432 } else { 3306 };
    Conn {
        driver,
        host: get("DB_HOST").unwrap_or_else(|| "127.0.0.1".into()),
        port: get("DB_PORT").and_then(|p| p.parse().ok()).unwrap_or(default_port),
        user: get("DB_USERNAME").unwrap_or_else(|| "root".into()),
        password: dotenv::get(env, "DB_PASSWORD").unwrap_or_default(),
        database: get("DB_DATABASE").unwrap_or_default(),
    }
}

pub(crate) fn tick(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

pub(crate) fn quote_pg(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The client program `names` (first found): in the Hearth service listening on `port`, else on PATH.
pub(crate) fn client(paths: &HearthPaths, kinds: &[&str], port: u16, names: &[&str]) -> PathBuf {
    let exe = std::env::consts::EXE_SUFFIX;
    for inst in
        crate::services::store::load(paths).iter().filter(|i| i.port == port && kinds.contains(&i.kind.as_str()))
    {
        let Ok(versions) = std::fs::read_dir(paths.data.join("services").join("bin").join(&inst.kind)) else {
            continue;
        };
        // The instance's own version line (`8.4` holds `8.4.11`), newest patch first.
        let line = format!("{}.", inst.line);
        let mut dirs: Vec<(String, PathBuf)> = versions
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .filter(|(v, _)| *v == inst.line || v.starts_with(&line) || inst.line == "current")
            .collect();
        dirs.sort_by(|a, b| crate::php::manifest::compare_versions(&a.0, &b.0));
        let dirs: Vec<PathBuf> = dirs.into_iter().map(|(_, p)| p).collect();
        for dir in dirs.iter().rev() {
            for name in names {
                let p = dir.join("bin").join(format!("{name}{exe}"));
                if p.is_file() {
                    return p;
                }
            }
        }
    }
    // PATH: the first of the names that is there, else the first name (the error then says so).
    for name in names {
        if let Some(found) = std::env::var_os("PATH")
            .and_then(|p| std::env::split_paths(&p).map(|d| d.join(format!("{name}{exe}"))).find(|p| p.is_file()))
        {
            return found;
        }
    }
    PathBuf::from(names[0])
}

pub(crate) fn mysql_args(c: &Conn) -> Vec<String> {
    vec!["--protocol=TCP".into(), "-h".into(), c.host.clone(), format!("-P{}", c.port), "-u".into(), c.user.clone()]
}

pub(crate) fn finish(what: &str, out: std::io::Result<std::process::Output>) -> Result<String, CoreError> {
    let out = out.map_err(|e| CoreError::Php(format!("{what} could not be started: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(CoreError::Php(format!("{what}: {}", String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// Runs one statement with the `mysql` client; tab-separated rows without a header.
pub(crate) fn mysql(paths: &HearthPaths, c: &Conn, database: Option<&str>, sql: &str) -> Result<String, CoreError> {
    let program = client(paths, &["mysql", "mariadb"], c.port, &["mysql", "mariadb"]);
    let mut cmd = crate::workspaces::command(&program);
    cmd.args(mysql_args(c)).args(["-N", "-B", "-e", sql]).env("MYSQL_PWD", &c.password);
    if let Some(db) = database {
        cmd.arg(db);
    }
    finish("mysql", cmd.stdin(Stdio::null()).output())
}

/// Runs one statement with `psql` against `database`; unaligned rows without a header.
pub(crate) fn psql(paths: &HearthPaths, c: &Conn, database: &str, sql: &str) -> Result<String, CoreError> {
    let program = client(paths, &["postgresql"], c.port, &["psql"]);
    let out = crate::workspaces::command(&program)
        .args(["-h", &c.host, "-p", &c.port.to_string(), "-U", &c.user, "-d", database])
        .args(["-v", "ON_ERROR_STOP=1", "-At", "-c", sql])
        .env("PGPASSWORD", &c.password)
        .stdin(Stdio::null())
        .output();
    finish("psql", out)
}

#[cfg(test)]
pub(crate) mod test_server;
#[cfg(test)]
mod tests;
