//! A workspace's database: the parent's (shared), a new empty one, or a copy. MySQL/MariaDB and
//! Postgres go through their own client programs (a Hearth service's on the same port first, else
//! the ones on PATH); SQLite is a file. Nothing here fails a workspace: a database that cannot be
//! made leaves the workspace on the parent's, with a note.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::workspaces::{env_file, names};
use crate::{CoreError, HearthPaths};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Shared,
    Empty,
    Clone,
}

impl Mode {
    /// Herd's names and aliases: `copy`/`cloned`, `fresh`/`new`, `none`/empty.
    pub fn parse(text: &str) -> Result<Mode, CoreError> {
        match text.trim().to_lowercase().as_str() {
            "clone" | "copy" | "cloned" => Ok(Mode::Clone),
            "empty" | "fresh" | "new" => Ok(Mode::Empty),
            "shared" | "none" | "" => Ok(Mode::Shared),
            _ => Err(CoreError::Php("Use shared, empty or clone for the database".into())),
        }
    }
}

/// What `prepare` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The mode in effect (shared when the asked one could not be made).
    pub mode: Mode,
    /// The workspace's own database (server) or file name (SQLite).
    pub name: Option<String>,
    pub driver: Option<String>,
    /// `.env` lines to write in the workspace.
    pub env: Vec<(String, String)>,
    pub notes: Vec<String>,
}

impl Prepared {
    fn shared(note: Option<String>) -> Prepared {
        Prepared { mode: Mode::Shared, name: None, driver: None, env: vec![], notes: note.into_iter().collect() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Driver {
    Mysql,
    Pgsql,
    Sqlite,
}

impl Driver {
    fn name(self) -> &'static str {
        match self {
            Driver::Mysql => "mysql",
            Driver::Pgsql => "pgsql",
            Driver::Sqlite => "sqlite",
        }
    }
}

/// A server connection from a `.env`.
struct Conn {
    driver: Driver,
    host: String,
    port: u16,
    user: String,
    password: String,
    database: String,
}

/// The driver of a `.env` (Laravel's default is SQLite), or why it is not supported.
fn driver(env: &str) -> Result<Driver, String> {
    let name = env_file::get(env, "DB_CONNECTION").unwrap_or_else(|| "sqlite".into());
    match name.to_lowercase().as_str() {
        "mysql" | "mariadb" => Ok(Driver::Mysql),
        "pgsql" | "postgres" | "postgresql" => Ok(Driver::Pgsql),
        "sqlite" => Ok(Driver::Sqlite),
        other => Err(format!("Hearth cannot make a {other} database: the workspace uses the parent's")),
    }
}

fn conn(env: &str, driver: Driver) -> Result<Conn, String> {
    let get = |k: &str| env_file::get(env, k).filter(|v| !v.is_empty());
    let database =
        get("DB_DATABASE").ok_or("DB_DATABASE is empty in .env: the workspace uses the parent's database")?;
    let default_port = if driver == Driver::Pgsql { 5432 } else { 3306 };
    Ok(Conn {
        driver,
        host: get("DB_HOST").unwrap_or_else(|| "127.0.0.1".into()),
        port: get("DB_PORT").and_then(|p| p.parse().ok()).unwrap_or(default_port),
        user: get("DB_USERNAME").unwrap_or_else(|| "root".into()),
        password: env_file::get(env, "DB_PASSWORD").unwrap_or_default(),
        database,
    })
}

/// Makes the workspace's database as `mode` asks, from the workspace's `.env` (a copy of the
/// parent's). `worktree` and `parent` are the two project folders.
pub fn prepare(paths: &HearthPaths, worktree: &Path, parent: &Path, env: &str, mode: Mode, branch: &str) -> Prepared {
    let driver = match driver(env) {
        Ok(d) => d,
        Err(_) if mode == Mode::Shared => return Prepared::shared(None),
        Err(note) => return Prepared::shared(Some(note)),
    };
    if driver == Driver::Sqlite {
        return sqlite(worktree, parent, env, mode);
    }
    if mode == Mode::Shared {
        return Prepared::shared(None);
    }
    let c = match conn(env, driver) {
        Ok(c) => c,
        Err(note) => return Prepared::shared(Some(note)),
    };
    match server(paths, &c, mode, branch) {
        Ok(name) => Prepared {
            mode,
            name: Some(name.clone()),
            driver: Some(driver.name().into()),
            env: vec![("DB_DATABASE".into(), name)],
            notes: vec![],
        },
        Err(e) => Prepared::shared(Some(format!("Its own database could not be made ({e}): it uses the parent's"))),
    }
}

/// Drops the database Hearth made for a workspace: `name` and `driver` as recorded, never what a
/// `.env` names now. The connection (host, port, user) comes from the workspace's `.env`, else the
/// parent's. Never the parent's own database.
pub fn drop(
    paths: &HearthPaths,
    env: &str,
    fallback_env: &str,
    name: &str,
    driver: &str,
    parent_db: &str,
) -> Result<(), CoreError> {
    let driver = match driver {
        "mysql" => Driver::Mysql,
        "pgsql" => Driver::Pgsql,
        // A SQLite file lives in the worktree and goes with it.
        _ => return Ok(()),
    };
    if name.is_empty() || name == parent_db {
        return Err(CoreError::Php(format!("`{name}` is the parent's database: it is not dropped")));
    }
    let source = if env_file::get(env, "DB_HOST").is_some() || env_file::get(env, "DB_PORT").is_some() {
        env
    } else {
        fallback_env
    };
    let mut c = conn(&env_file::set(source, &[("DB_DATABASE", name)]), driver).map_err(CoreError::Php)?;
    c.database = name.to_string();
    match driver {
        Driver::Mysql => mysql(paths, &c, None, &format!("DROP DATABASE IF EXISTS {}", tick(&c.database))).map(|_| ()),
        Driver::Pgsql => {
            let name = quote_pg(&c.database);
            let lit = c.database.replace('\'', "''");
            psql(
                paths,
                &c,
                "postgres",
                &format!("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{lit}'"),
            )?;
            psql(paths, &c, "postgres", &format!("DROP DATABASE IF EXISTS {name}")).map(|_| ())
        }
        Driver::Sqlite => Ok(()),
    }
}

// SQLite ------------------------------------------------------------------------------------------

fn sqlite(worktree: &Path, parent: &Path, env: &str, mode: Mode) -> Prepared {
    let value = env_file::get(env, "DB_DATABASE").filter(|v| !v.is_empty());
    if value.as_deref() == Some(":memory:") {
        return Prepared { mode, name: None, driver: Some("sqlite".into()), env: vec![], notes: vec![] };
    }
    let path = PathBuf::from(value.unwrap_or_else(|| "database/database.sqlite".into()));
    // Inside the project (relative, or an absolute path into the parent): the same place in the worktree.
    let relative =
        if path.is_absolute() { path.strip_prefix(parent).ok().map(Path::to_path_buf) } else { Some(path.clone()) };
    let done = |mode: Mode, env: Vec<(String, String)>, notes: Vec<String>, name: &Path| Prepared {
        mode,
        name: name.file_name().map(|n| n.to_string_lossy().into_owned()),
        driver: Some("sqlite".into()),
        env,
        notes,
    };
    let point = |p: &Path| vec![("DB_DATABASE".to_string(), p.to_string_lossy().into_owned())];
    match relative {
        Some(rel) => {
            let (from, own) = (parent.join(&rel), worktree.join(&rel));
            // An absolute path into the parent must now name the worktree's file.
            let env_for_own = if path.is_absolute() { point(&own) } else { vec![] };
            match mode {
                Mode::Shared if own.exists() => done(Mode::Shared, vec![], vec![], &own),
                Mode::Shared => done(Mode::Shared, point(&from), vec![], &from),
                Mode::Clone if from.is_file() => match copy_file(&from, &own) {
                    Ok(()) => done(Mode::Clone, env_for_own, vec![], &own),
                    Err(e) => done(
                        Mode::Shared,
                        point(&from),
                        vec![format!("The SQLite file could not be copied ({e}): it uses the parent's")],
                        &from,
                    ),
                },
                Mode::Clone | Mode::Empty => match empty_file(&own) {
                    Ok(()) => {
                        let note = (mode == Mode::Clone).then(|| {
                            "The parent has no SQLite file yet: the workspace starts with an empty one".to_string()
                        });
                        done(Mode::Empty, env_for_own, note.into_iter().collect(), &own)
                    }
                    Err(e) => done(
                        Mode::Shared,
                        point(&from),
                        vec![format!("The SQLite file could not be made ({e})")],
                        &from,
                    ),
                },
            }
        }
        None => {
            // Outside the project: a copy (or a new file) in the worktree's database folder.
            let own = worktree.join("database").join(path.file_name().unwrap_or_default());
            let made = match mode {
                Mode::Shared => return done(Mode::Shared, vec![], vec![], &path),
                Mode::Clone if path.is_file() => copy_file(&path, &own).map(|()| Mode::Clone),
                _ => empty_file(&own).map(|()| Mode::Empty),
            };
            match made {
                Ok(mode) => done(mode, point(&own), vec![], &own),
                Err(e) => done(Mode::Shared, vec![], vec![format!("The SQLite file could not be made ({e})")], &path),
            }
        }
    }
}

fn copy_file(from: &Path, to: &Path) -> Result<(), CoreError> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    }
    std::fs::copy(from, to).map(|_| ()).map_err(|e| CoreError::io(to, e))
}

fn empty_file(to: &Path) -> Result<(), CoreError> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    }
    std::fs::write(to, b"").map_err(|e| CoreError::io(to, e))
}

// Servers -----------------------------------------------------------------------------------------

/// Creates the database (empty, or a copy of the parent's) and returns its name.
fn server(paths: &HearthPaths, c: &Conn, mode: Mode, branch: &str) -> Result<String, CoreError> {
    let existing = match c.driver {
        Driver::Mysql => mysql(paths, c, None, "SHOW DATABASES")?,
        _ => psql(paths, c, "postgres", "SELECT datname FROM pg_database")?,
    };
    let taken: Vec<&str> = existing.lines().map(str::trim).collect();
    let name = names::database(&c.database, branch, |n| taken.contains(&n));
    match c.driver {
        Driver::Mysql => {
            let lit = c.database.replace('\'', "''");
            let charset = mysql(
                paths,
                c,
                None,
                &format!("SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = '{lit}'"),
            )?;
            let mut parts = charset.split_whitespace();
            let with = match (parts.next(), parts.next()) {
                (Some(cs), Some(col)) => format!(" CHARACTER SET {cs} COLLATE {col}"),
                _ => String::new(),
            };
            mysql(paths, c, None, &format!("CREATE DATABASE {}{with}", tick(&name)))?;
            if mode == Mode::Clone {
                if let Err(e) = mysql_copy(paths, c, &name) {
                    let _ = mysql(paths, c, None, &format!("DROP DATABASE IF EXISTS {}", tick(&name)));
                    return Err(e);
                }
            }
        }
        _ => {
            let new = quote_pg(&name);
            if mode == Mode::Clone {
                let template = format!("CREATE DATABASE {new} TEMPLATE {}", quote_pg(&c.database));
                // A template must have no other sessions: retry a few times, then copy by dump.
                let mut made = false;
                for attempt in 1..=5u64 {
                    match psql(paths, c, "postgres", &template) {
                        Ok(_) => {
                            made = true;
                            break;
                        }
                        Err(e) if e.to_string().contains("being accessed by other users") => {
                            std::thread::sleep(std::time::Duration::from_millis(400 * attempt));
                        }
                        Err(_) => break,
                    }
                }
                if !made {
                    psql(paths, c, "postgres", &format!("CREATE DATABASE {new}"))?;
                    if let Err(e) = pg_copy(paths, c, &name) {
                        let _ = psql(paths, c, "postgres", &format!("DROP DATABASE IF EXISTS {new}"));
                        return Err(e);
                    }
                }
            } else {
                psql(paths, c, "postgres", &format!("CREATE DATABASE {new}"))?;
            }
        }
    }
    Ok(name)
}

fn tick(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn quote_pg(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The client program `names` (first found): in the Hearth service listening on `port`, else on PATH.
fn client(paths: &HearthPaths, kinds: &[&str], port: u16, names: &[&str]) -> PathBuf {
    let exe = std::env::consts::EXE_SUFFIX;
    for inst in
        crate::services::store::load(paths).iter().filter(|i| i.port == port && kinds.contains(&i.kind.as_str()))
    {
        let Ok(versions) = std::fs::read_dir(paths.data.join("services").join("bin").join(&inst.kind)) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = versions.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
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

fn mysql_args(c: &Conn) -> Vec<String> {
    vec!["--protocol=TCP".into(), "-h".into(), c.host.clone(), format!("-P{}", c.port), "-u".into(), c.user.clone()]
}

fn finish(what: &str, out: std::io::Result<std::process::Output>) -> Result<String, CoreError> {
    let out = out.map_err(|e| CoreError::Php(format!("{what} could not be started: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(CoreError::Php(format!("{what}: {}", String::from_utf8_lossy(&out.stderr).trim())))
    }
}

/// Runs one statement with the `mysql` client; tab-separated rows without a header.
fn mysql(paths: &HearthPaths, c: &Conn, database: Option<&str>, sql: &str) -> Result<String, CoreError> {
    let program = client(paths, &["mysql", "mariadb"], c.port, &["mysql", "mariadb"]);
    let mut cmd = Command::new(&program);
    cmd.args(mysql_args(c)).args(["-N", "-B", "-e", sql]).env("MYSQL_PWD", &c.password);
    if let Some(db) = database {
        cmd.arg(db);
    }
    finish("mysql", cmd.stdin(Stdio::null()).output())
}

/// `mysqldump <parent> | mysql <name>`.
fn mysql_copy(paths: &HearthPaths, c: &Conn, name: &str) -> Result<(), CoreError> {
    let dump = client(paths, &["mysql", "mariadb"], c.port, &["mysqldump", "mariadb-dump"]);
    let load = client(paths, &["mysql", "mariadb"], c.port, &["mysql", "mariadb"]);
    let mut dumping = Command::new(&dump)
        .args(mysql_args(c))
        .args([
            "--single-transaction",
            "--routines",
            "--triggers",
            "--events",
            "--no-tablespaces",
            "--skip-comments",
            "--skip-lock-tables",
            &c.database,
        ])
        .env("MYSQL_PWD", &c.password)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::Php(format!("mysqldump could not be started: {e}")))?;
    let piped = dumping.stdout.take().map(Stdio::from).unwrap_or_else(Stdio::null);
    let loaded = Command::new(&load).args(mysql_args(c)).arg(name).env("MYSQL_PWD", &c.password).stdin(piped).output();
    let dumped = dumping.wait_with_output();
    let dumped = dumped.map_err(|e| CoreError::Php(format!("mysqldump: {e}")))?;
    if !dumped.status.success() {
        return Err(CoreError::Php(format!("mysqldump: {}", String::from_utf8_lossy(&dumped.stderr).trim())));
    }
    finish("mysql", loaded).map(|_| ())
}

/// Runs one statement with `psql` against `database`; unaligned rows without a header.
fn psql(paths: &HearthPaths, c: &Conn, database: &str, sql: &str) -> Result<String, CoreError> {
    let program = client(paths, &["postgresql"], c.port, &["psql"]);
    let out = Command::new(&program)
        .args(["-h", &c.host, "-p", &c.port.to_string(), "-U", &c.user, "-d", database])
        .args(["-v", "ON_ERROR_STOP=1", "-At", "-c", sql])
        .env("PGPASSWORD", &c.password)
        .stdin(Stdio::null())
        .output();
    finish("psql", out)
}

/// `pg_dump <parent> | psql <name>`.
fn pg_copy(paths: &HearthPaths, c: &Conn, name: &str) -> Result<(), CoreError> {
    let dump = client(paths, &["postgresql"], c.port, &["pg_dump"]);
    let load = client(paths, &["postgresql"], c.port, &["psql"]);
    let port = c.port.to_string();
    let mut dumping = Command::new(&dump)
        .args(["-h", &c.host, "-p", &port, "-U", &c.user, "--no-owner", "--no-privileges", &c.database])
        .env("PGPASSWORD", &c.password)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::Php(format!("pg_dump could not be started: {e}")))?;
    let piped = dumping.stdout.take().map(Stdio::from).unwrap_or_else(Stdio::null);
    let loaded = Command::new(&load)
        .args(["-h", &c.host, "-p", &port, "-U", &c.user, "-d", name, "--quiet", "-v", "ON_ERROR_STOP=1"])
        .env("PGPASSWORD", &c.password)
        .stdin(piped)
        .output();
    let dumped = dumping.wait_with_output().map_err(|e| CoreError::Php(format!("pg_dump: {e}")))?;
    if !dumped.status.success() {
        return Err(CoreError::Php(format!("pg_dump: {}", String::from_utf8_lossy(&dumped.stderr).trim())));
    }
    finish("psql", loaded).map(|_| ())
}
