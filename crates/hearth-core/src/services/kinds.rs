//! What each kind of service needs: its program, first-run setup, command line (always bound to
//! 127.0.0.1), clean stop and `.env` lines for Laravel.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::services::install::display_name;
use crate::services::store::{data_dir, log_file, Instance};
use crate::supervisor::ProcessSpec;
use crate::{CoreError, HearthPaths};

const KINDS: [&str; 7] = ["mysql", "mariadb", "postgresql", "redis", "meilisearch", "typesense", "silo"];

pub fn known(kind: &str) -> bool {
    KINDS.contains(&kind)
}

/// Kinds that are protected by a generated key or password.
pub fn needs_secret(kind: &str) -> bool {
    matches!(kind, "meilisearch" | "typesense" | "silo")
}

pub fn default_port(kind: &str) -> u16 {
    match kind {
        "mysql" | "mariadb" => 3306,
        "postgresql" => 5432,
        "redis" => 6379,
        "meilisearch" => 7700,
        "typesense" => 8108,
        _ => 9000,
    }
}

/// Ports an instance takes: Silo also serves its console on the next one.
pub fn ports(inst: &Instance) -> Vec<u16> {
    match inst.kind.as_str() {
        "silo" => vec![inst.port, inst.port.saturating_add(1)],
        _ => vec![inst.port],
    }
}

/// A command to run once (setup) or to stop a service cleanly.
#[derive(Debug)]
pub struct Run {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// Where the main program may sit in an install folder, first match wins.
fn server_candidates(kind: &str) -> &'static [&'static str] {
    match kind {
        "mysql" => &["bin/mysqld"],
        "mariadb" => &["bin/mariadbd", "bin/mysqld"],
        "postgresql" => &["bin/postgres"],
        "redis" => &["redis-server", "bin/redis-server", "src/redis-server"],
        "meilisearch" => &["meilisearch"],
        "typesense" => &["typesense-server", "bin/typesense-server"],
        _ => &["silo", "minio", "bin/silo"],
    }
}

fn find(install: &Path, candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(|c| install.join(format!("{c}{}", std::env::consts::EXE_SUFFIX))).find(|p| p.is_file())
}

fn server(inst: &Instance, install: &Path) -> Result<PathBuf, CoreError> {
    let candidates = server_candidates(&inst.kind);
    find(install, candidates).ok_or_else(|| {
        let name = candidates[0].rsplit('/').next().unwrap_or(candidates[0]);
        CoreError::Php(format!("{} is not installed completely ({name} is missing)", display_name(&inst.kind)))
    })
}

/// A sibling program of the server (`mysqladmin` next to `mysqld`).
fn sibling(install: &Path, names: &[&str]) -> Option<PathBuf> {
    find(install, names)
}

fn os(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

/// Whether `data` has never been set up (missing or empty).
fn empty(data: &Path) -> bool {
    std::fs::read_dir(data).map(|mut d| d.next().is_none()).unwrap_or(true)
}

/// The one-time setup of an empty data folder, for the databases.
pub fn init(inst: &Instance, install: &Path, data: &Path) -> Option<Run> {
    if !empty(data) {
        return None;
    }
    let datadir = OsString::from(format!("--datadir={}", data.display()));
    match inst.kind.as_str() {
        "mysql" => {
            let mut args = os(&["--no-defaults", "--initialize-insecure"]);
            args.push(format!("--basedir={}", install.display()).into());
            args.push(datadir);
            Some(Run { program: server(inst, install).ok()?, args })
        }
        "mariadb" if cfg!(windows) => {
            // The Windows tool sets up the folder itself (root without a password).
            let program = sibling(install, &["bin/mariadb-install-db", "bin/mysql_install_db"])?;
            Some(Run { program, args: vec![datadir, format!("--port={}", inst.port).into()] })
        }
        "mariadb" => {
            let program = install.join("scripts").join("mariadb-install-db");
            let mut args = os(&["--no-defaults", "--auth-root-authentication-method=normal", "--skip-test-db"]);
            args.push(format!("--basedir={}", install.display()).into());
            args.push(datadir);
            Some(Run { program, args })
        }
        "postgresql" => {
            let program = sibling(install, &["bin/initdb"])?;
            let mut args = vec![OsString::from("-D"), data.as_os_str().to_owned()];
            args.extend(os(&["-U", "postgres", "-A", "trust", "-E", "UTF8", "--locale=C"]));
            Some(Run { program, args })
        }
        _ => None,
    }
}

/// How the service runs under the supervisor.
pub fn spec(paths: &HearthPaths, inst: &Instance, install: &Path) -> Result<ProcessSpec, CoreError> {
    let program = server(inst, install)?;
    let data = data_dir(paths, inst);
    let port = inst.port.to_string();
    let d = data.display().to_string();
    let mut env: Vec<(String, OsString)> = vec![];
    let args: Vec<OsString> = match inst.kind.as_str() {
        "mysql" | "mariadb" => {
            let mut a = os(&["--no-defaults"]);
            a.push(format!("--basedir={}", install.display()).into());
            a.push(format!("--datadir={d}").into());
            a.push(format!("--port={port}").into());
            a.push("--bind-address=127.0.0.1".into());
            if inst.kind == "mysql" {
                a.push("--mysqlx=OFF".into());
            }
            if cfg!(windows) {
                // Errors go to the console (Hearth's log), not to a file in the data folder.
                a.push("--console".into());
            } else {
                a.push(format!("--socket={}", data.join("server.sock").display()).into());
            }
            a
        }
        "postgresql" => {
            let mut a = vec![OsString::from("-D"), data.as_os_str().to_owned(), "-p".into(), port.clone().into()];
            a.extend(os(&["-c", "listen_addresses=127.0.0.1", "-c", "unix_socket_directories="]));
            a
        }
        "redis" => {
            let mut a = os(&["--port", &port, "--bind", "127.0.0.1", "--dir"]);
            a.push(data.as_os_str().to_owned());
            a.extend(os(&["--save", "60 1", "--appendonly", "no", "--protected-mode", "yes"]));
            a
        }
        "meilisearch" => {
            let mut a = os(&["--http-addr", &format!("127.0.0.1:{port}"), "--db-path"]);
            a.push(data.join("data.ms").into_os_string());
            a.extend(os(&["--master-key", &inst.secret, "--no-analytics", "--env", "development"]));
            a
        }
        "typesense" => {
            let mut a = os(&["--data-dir"]);
            a.push(data.as_os_str().to_owned());
            a.extend(os(&[
                "--api-key",
                &inst.secret,
                "--listen-address",
                "127.0.0.1",
                "--api-port",
                &port,
                "--enable-cors",
            ]));
            a
        }
        _ => {
            // Silo (MinIO): credentials come from the environment.
            env.push(("MINIO_ROOT_USER".into(), "hearth".into()));
            env.push(("MINIO_ROOT_PASSWORD".into(), inst.secret.clone().into()));
            env.push(("MINIO_BROWSER_REDIRECT".into(), "off".into()));
            let mut a = os(&["server"]);
            a.push(data.as_os_str().to_owned());
            a.extend(os(&[
                "--address",
                &format!("127.0.0.1:{port}"),
                "--console-address",
                &format!("127.0.0.1:{}", inst.port.saturating_add(1)),
            ]));
            a
        }
    };
    Ok(ProcessSpec {
        key: format!("service-{}", inst.id),
        program,
        args,
        env,
        cwd: Some(data),
        log: log_file(paths, inst),
        ready_port: Some(inst.port),
        ready_socket: None,
    })
}

/// A clean shutdown for the databases (the supervisor stops the rest).
pub fn stop(inst: &Instance, install: &Path, data: &Path) -> Option<Run> {
    let port = inst.port.to_string();
    match inst.kind.as_str() {
        "mysql" | "mariadb" => {
            let program = sibling(install, &["bin/mariadb-admin", "bin/mysqladmin"])?;
            Some(Run {
                program,
                args: os(&["--no-defaults", "-uroot", "-h127.0.0.1", &format!("-P{port}"), "shutdown"]),
            })
        }
        "postgresql" => {
            let program = sibling(install, &["bin/pg_ctl"])?;
            let mut args = os(&["stop", "-m", "fast", "-D"]);
            args.push(data.as_os_str().to_owned());
            Some(Run { program, args })
        }
        _ => None,
    }
}

/// Removes a lock a crash left behind (PostgreSQL refuses to start while `postmaster.pid` names a
/// process; after a crash that process is gone).
pub fn clean_stale(inst: &Instance, data: &Path) {
    if inst.kind != "postgresql" {
        return;
    }
    let lock = data.join("postmaster.pid");
    let pid =
        std::fs::read_to_string(&lock).ok().and_then(|t| t.lines().next().and_then(|l| l.trim().parse::<u32>().ok()));
    if let Some(pid) = pid {
        if !process_alive(pid) {
            let _ = std::fs::remove_file(&lock);
        }
    }
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else { return false };
        // SAFETY: signal 0 only checks that the process exists.
        let rc = unsafe { libc::kill(pid, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        const STILL_ACTIVE: u32 = 259;
        // SAFETY: a query-only handle, closed below.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code) != 0;
            CloseHandle(handle);
            ok && code == STILL_ACTIVE
        }
    }
}

/// The `.env` lines a Laravel app needs for this service.
pub fn env(inst: &Instance) -> Vec<(String, String)> {
    let port = inst.port.to_string();
    let pairs: Vec<(&str, String)> = match inst.kind.as_str() {
        "mysql" | "mariadb" => vec![
            ("DB_CONNECTION", inst.kind.clone()),
            ("DB_HOST", "127.0.0.1".into()),
            ("DB_PORT", port),
            ("DB_USERNAME", "root".into()),
            ("DB_PASSWORD", String::new()),
        ],
        "postgresql" => vec![
            ("DB_CONNECTION", "pgsql".into()),
            ("DB_HOST", "127.0.0.1".into()),
            ("DB_PORT", port),
            ("DB_USERNAME", "postgres".into()),
            ("DB_PASSWORD", String::new()),
        ],
        "redis" => vec![("REDIS_HOST", "127.0.0.1".into()), ("REDIS_PORT", port), ("REDIS_PASSWORD", "null".into())],
        "meilisearch" => vec![
            ("SCOUT_DRIVER", "meilisearch".into()),
            ("MEILISEARCH_HOST", format!("http://127.0.0.1:{port}")),
            ("MEILISEARCH_KEY", inst.secret.clone()),
        ],
        "typesense" => vec![
            ("SCOUT_DRIVER", "typesense".into()),
            ("TYPESENSE_HOST", "127.0.0.1".into()),
            ("TYPESENSE_PORT", port),
            ("TYPESENSE_PROTOCOL", "http".into()),
            ("TYPESENSE_API_KEY", inst.secret.clone()),
        ],
        _ => vec![
            ("FILESYSTEM_DISK", "s3".into()),
            ("AWS_ACCESS_KEY_ID", "hearth".into()),
            ("AWS_SECRET_ACCESS_KEY", inst.secret.clone()),
            ("AWS_DEFAULT_REGION", "us-east-1".into()),
            ("AWS_BUCKET", "local".into()),
            ("AWS_ENDPOINT", format!("http://127.0.0.1:{port}")),
            ("AWS_USE_PATH_STYLE_ENDPOINT", "true".into()),
        ],
    };
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}
