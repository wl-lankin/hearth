use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::workspaces::database::{self, Mode};
use crate::HearthPaths;

fn home() -> (tempfile::TempDir, HearthPaths, PathBuf, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    let parent = d.path().join("shop");
    let worktree = d.path().join("ws");
    std::fs::create_dir_all(parent.join("database")).unwrap();
    std::fs::create_dir_all(worktree.join("database")).unwrap();
    (d, paths, parent, worktree)
}

fn env_db(prepared: &database::Prepared) -> Option<String> {
    prepared.env.iter().find(|(k, _)| k == "DB_DATABASE").map(|(_, v)| v.clone())
}

#[test]
fn modes_take_herds_names() {
    assert_eq!(Mode::parse("clone").unwrap(), Mode::Clone);
    assert_eq!(Mode::parse("copy").unwrap(), Mode::Clone);
    assert_eq!(Mode::parse("cloned").unwrap(), Mode::Clone);
    assert_eq!(Mode::parse("empty").unwrap(), Mode::Empty);
    assert_eq!(Mode::parse("fresh").unwrap(), Mode::Empty);
    assert_eq!(Mode::parse("new").unwrap(), Mode::Empty);
    assert_eq!(Mode::parse("shared").unwrap(), Mode::Shared);
    assert_eq!(Mode::parse("none").unwrap(), Mode::Shared);
    assert_eq!(Mode::parse("").unwrap(), Mode::Shared);
    assert_eq!(Mode::parse("other").unwrap_err().to_string(), "Use shared, empty or clone for the database");
}

#[test]
fn sqlite_in_the_project_is_copied_emptied_or_shared() {
    let (_d, paths, parent, worktree) = home();
    std::fs::write(parent.join("database").join("database.sqlite"), b"parent data").unwrap();
    let env = "DB_CONNECTION=sqlite\n";
    let copy = database::prepare(&paths, &worktree, &parent, env, Mode::Clone, "feature");
    assert_eq!(copy.mode, Mode::Clone);
    assert_eq!(std::fs::read(worktree.join("database").join("database.sqlite")).unwrap(), b"parent data");
    assert_eq!(env_db(&copy), None, "the default path is already the worktree's own");
    let empty = database::prepare(&paths, &worktree, &parent, env, Mode::Empty, "feature");
    assert_eq!(std::fs::read(worktree.join("database").join("database.sqlite")).unwrap(), b"");
    assert_eq!(empty.mode, Mode::Empty);
    std::fs::remove_file(worktree.join("database").join("database.sqlite")).unwrap();
    let shared = database::prepare(&paths, &worktree, &parent, env, Mode::Shared, "feature");
    let pointed = PathBuf::from(env_db(&shared).unwrap());
    assert_eq!(pointed, parent.join("database").join("database.sqlite"), "shared means the parent's file");
}

#[test]
fn a_sqlite_file_outside_the_project_is_copied_into_the_worktree() {
    let (d, paths, parent, worktree) = home();
    let outside = d.path().join("data").join("shop.db");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, b"rows").unwrap();
    let env = format!("DB_CONNECTION=sqlite\nDB_DATABASE=\"{}\"\n", outside.display());
    let copy = database::prepare(&paths, &worktree, &parent, &env, Mode::Clone, "feature");
    let target = worktree.join("database").join("shop.db");
    assert_eq!(std::fs::read(&target).unwrap(), b"rows");
    assert_eq!(env_db(&copy).map(PathBuf::from), Some(target));
    assert_eq!(std::fs::read(&outside).unwrap(), b"rows", "the parent's file is untouched");
}

#[test]
fn an_unsupported_or_unnamed_database_stays_shared_with_a_note() {
    let (_d, paths, parent, worktree) = home();
    let odd = database::prepare(&paths, &worktree, &parent, "DB_CONNECTION=sqlsrv\nDB_DATABASE=x\n", Mode::Clone, "f");
    assert_eq!(odd.mode, Mode::Shared);
    assert!(odd.notes[0].contains("sqlsrv"), "{:?}", odd.notes);
    let unnamed = database::prepare(&paths, &worktree, &parent, "DB_CONNECTION=mysql\n", Mode::Empty, "f");
    assert_eq!(unnamed.mode, Mode::Shared);
    assert!(unnamed.notes[0].contains("DB_DATABASE"), "{:?}", unnamed.notes);
    let shared =
        database::prepare(&paths, &worktree, &parent, "DB_CONNECTION=mysql\nDB_DATABASE=shop\n", Mode::Shared, "f");
    assert_eq!((shared.mode, shared.notes.len(), shared.env.len()), (Mode::Shared, 0, 0));
}

/// A throwaway MySQL or MariaDB server: `HEARTH_TEST_MYSQLD` names its `mysqld`/`mariadbd`, the
/// install tool and `mysql` client sit next to it. Free port, temp data folder, stopped at the end.
struct TestServer {
    child: std::process::Child,
    port: u16,
    bin: PathBuf,
    _data: tempfile::TempDir,
}

impl TestServer {
    fn start() -> Option<TestServer> {
        let server = PathBuf::from(std::env::var_os("HEARTH_TEST_MYSQLD")?);
        let bin = server.parent()?.to_path_buf();
        let data = tempfile::tempdir().ok()?;
        let exe = std::env::consts::EXE_SUFFIX;
        let install = ["mariadb-install-db", "mysql_install_db"]
            .iter()
            .map(|n| bin.join(format!("{n}{exe}")))
            .find(|p| p.is_file())?;
        let ok = std::process::Command::new(install)
            .arg(format!("--datadir={}", data.path().join("d").display()))
            .output()
            .ok()?
            .status
            .success();
        assert!(ok, "the test server's data folder could not be set up");
        let port = std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok()?.port();
        let child = std::process::Command::new(&server)
            .args(["--no-defaults", "--console", "--bind-address=127.0.0.1"])
            .arg(format!("--datadir={}", data.path().join("d").display()))
            .arg(format!("--port={port}"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "the test server did not start");
            std::thread::sleep(Duration::from_millis(200));
        }
        Some(TestServer { child, port, bin, _data: data })
    }

    fn sql(&self, sql: &str) -> String {
        let out = std::process::Command::new(self.bin.join(format!("mysql{}", std::env::consts::EXE_SUFFIX)))
            .args(["-h", "127.0.0.1", "-u", "root", "-N", "-B"])
            .arg(format!("-P{}", self.port))
            .args(["-e", sql])
            .output()
            .unwrap();
        assert!(out.status.success(), "{sql}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mysql_gets_an_empty_or_copied_database_and_drop_removes_it() {
    let Some(server) = TestServer::start() else {
        eprintln!("skipped: set HEARTH_TEST_MYSQLD to a mysqld or mariadbd");
        return;
    };
    let (_d, paths, parent, worktree) = home();
    server.sql(
        "CREATE DATABASE shop; CREATE TABLE shop.orders (id INT PRIMARY KEY); INSERT INTO shop.orders VALUES (1),(2);",
    );
    let env = format!(
        "DB_CONNECTION=mariadb\nDB_HOST=127.0.0.1\nDB_PORT={}\nDB_DATABASE=shop\nDB_USERNAME=root\nDB_PASSWORD=\n",
        server.port
    );
    let copy = database::prepare(&paths, &worktree, &parent, &env, Mode::Clone, "feature/X");
    assert_eq!((copy.mode, copy.name.as_deref()), (Mode::Clone, Some("shop_feature_x")), "{:?}", copy.notes);
    assert_eq!(env_db(&copy).as_deref(), Some("shop_feature_x"));
    assert_eq!(server.sql("SELECT COUNT(*) FROM shop_feature_x.orders"), "2");
    let empty = database::prepare(&paths, &worktree, &parent, &env, Mode::Empty, "feature/X");
    assert_eq!(empty.name.as_deref(), Some("shop_feature_x_2"), "the first name is taken");
    assert_eq!(server.sql("SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA='shop_feature_x_2'"), "0");
    let worktree_env = env.replace("DB_DATABASE=shop", "DB_DATABASE=shop_feature_x");
    database::drop(&paths, &worktree_env, "shop").unwrap();
    assert!(!server.sql("SHOW DATABASES").lines().any(|l| l == "shop_feature_x"));
    assert!(server.sql("SHOW DATABASES").lines().any(|l| l == "shop"), "the parent's database stays");
    // Dropping the parent's own database is refused.
    assert!(database::drop(&paths, &env, "shop").is_err());
}
