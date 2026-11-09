//! The db.* methods: the databases on the server a site's `.env` names. They run client programs:
//! off the async threads.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::{conn, driver, mysql, psql, quote_pg, tick, Conn, Driver};
use crate::ipc::{CoreState, RpcError, INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND};
use crate::{dotenv, CoreError, StokePaths};

fn error(message: impl Into<String>) -> CoreError {
    CoreError::Php(message.into())
}

/// A site's database setup, from its `.env`.
struct Target {
    site: String,
    env_file: PathBuf,
    text: String,
    driver: Driver,
}

impl Target {
    fn of(paths: &StokePaths, site: &str) -> Result<Target, CoreError> {
        let env_file = crate::env::file_of(paths, site)?;
        let text = std::fs::read_to_string(&env_file).map_err(|_| error(format!("{site}.test has no .env file")))?;
        let driver = driver(&text).map_err(|other| {
            error(format!(
                "{site}.test uses the {other} driver: Stoke manages MySQL, MariaDB, PostgreSQL and SQLite databases"
            ))
        })?;
        Ok(Target { site: site.to_string(), env_file, text, driver })
    }

    fn folder(&self) -> PathBuf {
        self.env_file.parent().map(|p| p.join("database")).unwrap_or_default()
    }

    fn conn(&self) -> Conn {
        conn(&self.text, self.driver)
    }

    /// The SQLite file the site uses: `DB_DATABASE` (relative to the project), else Laravel's default.
    fn sqlite_file(&self) -> PathBuf {
        match dotenv::get(&self.text, "DB_DATABASE").filter(|v| !v.is_empty()) {
            Some(v) => self.env_file.parent().map(|p| p.join(&v)).unwrap_or_else(|| PathBuf::from(v)),
            None => self.folder().join("database.sqlite"),
        }
    }

    /// The database the site uses, by the name the other methods take.
    fn current(&self) -> String {
        match self.driver {
            Driver::Sqlite => {
                let file = self.sqlite_file();
                match (file.parent() == Some(self.folder().as_path()), file.extension(), file.file_stem()) {
                    (true, Some(ext), Some(stem)) if ext == "sqlite" => stem.to_string_lossy().into_owned(),
                    _ => file.to_string_lossy().into_owned(),
                }
            }
            // Laravel's own default when `.env` names none.
            _ => Some(self.conn().database).filter(|d| !d.is_empty()).unwrap_or_else(|| "laravel".into()),
        }
    }

    fn sqlite_path(&self, name: &str) -> Result<PathBuf, CoreError> {
        let ok = !name.is_empty()
            && !name.starts_with('.')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
        if !ok {
            return Err(error(format!("[{name}] is not a database name")));
        }
        let name = name.strip_suffix(".sqlite").unwrap_or(name);
        Ok(self.folder().join(format!("{name}.sqlite")))
    }

    fn list(&self, paths: &StokePaths) -> Result<Vec<String>, CoreError> {
        let mut names: Vec<String> = match self.driver {
            Driver::Sqlite => std::fs::read_dir(self.folder())
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "sqlite"))
                        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                        .collect()
                })
                .unwrap_or_default(),
            Driver::Mysql => mysql(paths, &self.conn(), None, "SHOW DATABASES")?
                .lines()
                .map(str::trim)
                .filter(|n| !["information_schema", "mysql", "performance_schema", "sys"].contains(n))
                .map(str::to_string)
                .collect(),
            Driver::Pgsql => {
                psql(paths, &self.conn(), "postgres", "SELECT datname FROM pg_database WHERE NOT datistemplate")?
                    .lines()
                    .map(str::trim)
                    .filter(|n| *n != "postgres")
                    .map(str::to_string)
                    .collect()
            }
        };
        names.retain(|n| !n.is_empty());
        names.sort();
        Ok(names)
    }

    /// Creates `name` unless it exists; whether it was made.
    fn create(&self, paths: &StokePaths, name: &str) -> Result<bool, CoreError> {
        if self.driver == Driver::Sqlite {
            let file = self.sqlite_path(name)?;
            if file.exists() {
                return Ok(false);
            }
            std::fs::create_dir_all(self.folder()).map_err(|e| CoreError::io(self.folder(), e))?;
            std::fs::write(&file, b"").map_err(|e| CoreError::io(&file, e))?;
            return Ok(true);
        }
        if name.is_empty() {
            return Err(error("A database name is needed: DB_DATABASE is empty in .env"));
        }
        if self.list(paths)?.iter().any(|n| n == name) {
            return Ok(false);
        }
        match self.driver {
            Driver::Mysql => mysql(paths, &self.conn(), None, &format!("CREATE DATABASE {}", tick(name)))?,
            _ => psql(paths, &self.conn(), "postgres", &format!("CREATE DATABASE {}", quote_pg(name)))?,
        };
        Ok(true)
    }

    fn drop(&self, paths: &StokePaths, name: &str) -> Result<(), CoreError> {
        let current = self.current();
        let refused = || error(format!("{}.test uses `{current}`: it is not dropped", self.site));
        if name.eq_ignore_ascii_case(&current) || name.is_empty() {
            return Err(refused());
        }
        if self.driver == Driver::Sqlite {
            let file = self.sqlite_path(name)?;
            // The same file however its path is spelled (`..`, case, short names, links).
            if same_file(&file, &self.sqlite_file()) {
                return Err(refused());
            }
            if !file.is_file() {
                return Err(error(format!("No database [{name}] in {}", self.folder().display())));
            }
            return std::fs::remove_file(&file).map_err(|e| CoreError::io(&file, e));
        }
        if !self.list(paths)?.iter().any(|n| n == name) {
            return Err(error(format!("No database [{name}] on the server")));
        }
        let c = self.conn();
        match self.driver {
            Driver::Mysql => mysql(paths, &c, None, &format!("DROP DATABASE {}", tick(name))).map(|_| ()),
            _ => {
                let lit = name.replace('\'', "''");
                psql(
                    paths,
                    &c,
                    "postgres",
                    &format!("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{lit}'"),
                )?;
                psql(paths, &c, "postgres", &format!("DROP DATABASE {}", quote_pg(name))).map(|_| ())
            }
        }
    }

    /// Points `DB_DATABASE` at `name` (a SQLite database by its full path).
    fn switch(&self, name: &str) -> Result<(), CoreError> {
        let value = match self.driver {
            Driver::Sqlite => self.sqlite_path(name)?.to_string_lossy().into_owned(),
            _ => name.to_string(),
        };
        let next = dotenv::set(&self.text, &[("DB_DATABASE", &value)]);
        crate::atomic::write(&self.env_file, next.as_bytes())
    }
}

/// Whether two paths are one file: resolved when they exist, else compared without case (Windows
/// and macOS file systems ignore it).
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()),
    }
}

/// A name a server database can have: no control characters (a line break would reach `.env`).
fn server_name(name: &str) -> Result<(), CoreError> {
    if name.chars().any(char::is_control) {
        return Err(error(format!("[{}] is not a database name", name.escape_debug())));
    }
    Ok(())
}

/// A name as an agent may write it: trimmed, without surrounding backticks or quotes.
fn bare(name: &str) -> String {
    let name = name.trim();
    for q in ['`', '"', '\''] {
        if let Some(inner) = name.strip_prefix(q).and_then(|n| n.strip_suffix(q)) {
            return inner.trim().to_string();
        }
    }
    name.to_string()
}

fn exists(target: &Target, paths: &StokePaths, name: &str) -> Result<bool, CoreError> {
    Ok(match target.driver {
        Driver::Sqlite => target.sqlite_path(name)?.is_file(),
        _ => target.list(paths)?.iter().any(|n| n == name),
    })
}

fn answer(paths: &StokePaths, method: &str, params: &Value) -> Result<Value, CoreError> {
    let site = params.get("site").and_then(Value::as_str).ok_or_else(|| error("site is required"))?;
    let name = params.get("name").and_then(Value::as_str).map(bare);
    let target = Target::of(paths, site)?;
    if let (Some(n), false) = (&name, target.driver == Driver::Sqlite) {
        server_name(n)?;
    }
    match method {
        "db.connection" => Ok(match target.driver {
            Driver::Sqlite => json!({ "driver": "sqlite", "database": target.sqlite_file() }),
            driver => {
                let c = target.conn();
                json!({
                    "driver": driver.name(), "host": c.host, "port": c.port, "username": c.user,
                    "password": c.password, "database": c.database,
                })
            }
        }),
        "db.list" => {
            Ok(json!({ "driver": target.driver.name(), "current": target.current(), "databases": target.list(paths)? }))
        }
        "db.create" => {
            let name = name.unwrap_or_else(|| target.current());
            let created = target.create(paths, &name)?;
            Ok(json!({ "name": name, "created": created }))
        }
        "db.use" => {
            let name = name.filter(|n| !n.is_empty()).ok_or_else(|| error("name is required"))?;
            let create = params.get("create").and_then(Value::as_bool).unwrap_or(false);
            let created = if create {
                target.create(paths, &name)?
            } else if exists(&target, paths, &name)? {
                false
            } else {
                return Err(error(format!("No database [{name}]: pass create to make it")));
            };
            target.switch(&name)?;
            Ok(json!({ "name": name, "created": created }))
        }
        "db.drop" => {
            let name = name.ok_or_else(|| error("name is required"))?;
            target.drop(paths, &name)?;
            Ok(json!({ "name": name }))
        }
        _ => unreachable!("only db.* methods get here"),
    }
}

/// Answers a db.* method, or `None` when `method` is not one of them.
pub async fn handle(core: &CoreState, method: &str, params: &Value) -> Option<Result<Value, RpcError>> {
    if !method.starts_with("db.") {
        return None;
    }
    if !["db.connection", "db.list", "db.create", "db.use", "db.drop"].contains(&method) {
        return Some(Err(RpcError::new(METHOD_NOT_FOUND, format!("unknown method: {method}"))));
    }
    if params.get("site").and_then(Value::as_str).is_none() {
        return Some(Err(RpcError::new(INVALID_PARAMS, "site is required")));
    }
    let (paths, method, params) = (core.paths.clone(), method.to_string(), params.clone());
    let result = tokio::task::spawn_blocking(move || answer(&paths, &method, &params)).await;
    Some(match result {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(RpcError::new(INTERNAL_ERROR, e.to_string())),
        Err(e) => Err(RpcError::new(INTERNAL_ERROR, e.to_string())),
    })
}
