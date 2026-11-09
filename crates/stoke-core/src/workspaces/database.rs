//! A workspace's database: the parent's (shared), a new empty one, or a copy. MySQL/MariaDB and
//! Postgres go through their own client programs (a Stoke service's on the same port first, else
//! the ones on PATH); SQLite is a file. Nothing here fails a workspace: a database that cannot be
//! made leaves the workspace on the parent's, with a note.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};

use crate::db::{client, finish, mysql, mysql_args, psql, quote_pg, tick, Conn, Driver};
use crate::workspaces::{env_file, names};
use crate::{CoreError, StokePaths};

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

/// The driver of a `.env`, or why the workspace keeps the parent's database.
fn driver(env: &str) -> Result<Driver, String> {
    crate::db::driver(env)
        .map_err(|other| format!("Stoke cannot make a {other} database: the workspace uses the parent's"))
}

fn conn(env: &str, driver: Driver) -> Result<Conn, String> {
    let c = crate::db::conn(env, driver);
    if c.database.is_empty() {
        return Err("DB_DATABASE is empty in .env: the workspace uses the parent's database".into());
    }
    Ok(c)
}

/// Makes the workspace's database as `mode` asks, from the workspace's `.env` (a copy of the
/// parent's). `worktree` and `parent` are the two project folders.
pub fn prepare(paths: &StokePaths, worktree: &Path, parent: &Path, env: &str, mode: Mode, branch: &str) -> Prepared {
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

/// Drops the database Stoke made for a workspace: `name` and `driver` as recorded, never what a
/// `.env` names now. The connection (host, port, user) comes from the workspace's `.env`, else the
/// parent's. Never the parent's own database.
pub fn drop(
    paths: &StokePaths,
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
    // Rebuilt from its parts: `database/database.sqlite` gets the platform's separator.
    let path: PathBuf = path.components().collect();
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
fn server(paths: &StokePaths, c: &Conn, mode: Mode, branch: &str) -> Result<String, CoreError> {
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

/// `mysqldump <parent> | mysql <name>`.
fn mysql_copy(paths: &StokePaths, c: &Conn, name: &str) -> Result<(), CoreError> {
    let mut dump =
        crate::workspaces::command(client(paths, &["mysql", "mariadb"], c.port, &["mysqldump", "mariadb-dump"]));
    dump.args(mysql_args(c))
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
        .env("MYSQL_PWD", &c.password);
    let mut load = crate::workspaces::command(client(paths, &["mysql", "mariadb"], c.port, &["mysql", "mariadb"]));
    load.args(mysql_args(c)).arg(name).env("MYSQL_PWD", &c.password);
    pipe(dump, load, "mysqldump", "mysql")
}

/// `dump | load`. The dump's messages go to a temporary file (a full stderr pipe would stall both
/// programs); when both fail, the loader's error is the real one and comes first.
fn pipe(
    mut dump: std::process::Command,
    mut load: std::process::Command,
    dump_name: &str,
    load_name: &str,
) -> Result<(), CoreError> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let log = std::env::temp_dir().join(format!("stoke-dump-{}-{nanos}.log", std::process::id()));
    let errors = std::fs::File::create(&log).map_err(|e| CoreError::io(&log, e))?;
    let mut dumping = dump
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(errors))
        .spawn()
        .map_err(|e| CoreError::Php(format!("{dump_name} could not be started: {e}")))?;
    let piped = dumping.stdout.take().map(Stdio::from).unwrap_or_else(Stdio::null);
    let loaded = load.stdin(piped).output();
    let dumped = dumping.wait();
    let dump_errors = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    finish(load_name, loaded)?;
    match dumped {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(CoreError::Php(format!("{dump_name}: {}", dump_errors.trim()))),
        Err(e) => Err(CoreError::Php(format!("{dump_name}: {e}"))),
    }
}

/// `pg_dump <parent> | psql <name>`.
fn pg_copy(paths: &StokePaths, c: &Conn, name: &str) -> Result<(), CoreError> {
    let port = c.port.to_string();
    let mut dump = crate::workspaces::command(client(paths, &["postgresql"], c.port, &["pg_dump"]));
    dump.args(["-h", &c.host, "-p", &port, "-U", &c.user, "--no-owner", "--no-privileges", &c.database])
        .env("PGPASSWORD", &c.password);
    let mut load = crate::workspaces::command(client(paths, &["postgresql"], c.port, &["psql"]));
    load.args(["-h", &c.host, "-p", &port, "-U", &c.user, "-d", name, "--quiet", "-v", "ON_ERROR_STOP=1"])
        .env("PGPASSWORD", &c.password);
    pipe(dump, load, "pg_dump", "psql")
}
