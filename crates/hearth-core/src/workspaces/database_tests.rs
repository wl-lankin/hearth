use std::path::PathBuf;

use crate::db::test_server::TestServer;
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
    // One kind of separator in what the .env gets (`database/database.sqlite` is Laravel's default).
    let written = env_db(&shared).unwrap();
    assert!(!(cfg!(windows) && written.contains('/')), "{written}");
}

#[test]
fn a_sqlite_file_outside_the_project_is_copied_into_the_worktree() {
    let (d, paths, parent, worktree) = home();
    let outside = d.path().join("data").join("shop.db");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, b"rows").unwrap();
    let env = format!("DB_CONNECTION=sqlite\nDB_DATABASE='{}'\n", outside.display());
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
    // The workspace's .env was later pointed elsewhere: delete drops the recorded database only.
    server.sql("CREATE DATABASE shop_staging");
    let worktree_env = env.replace("DB_DATABASE=shop", "DB_DATABASE=shop_staging");
    database::drop(&paths, &worktree_env, &env, "shop_feature_x", "mysql", "shop").unwrap();
    assert!(server.sql("SHOW DATABASES").lines().any(|l| l == "shop_staging"), "not the one the .env names now");
    // Without a usable .env the parent's connection is used.
    database::drop(&paths, "", &env, "shop_feature_x_2", "mysql", "shop").unwrap();
    assert!(!server.sql("SHOW DATABASES").lines().any(|l| l == "shop_feature_x_2"));
    assert!(!server.sql("SHOW DATABASES").lines().any(|l| l == "shop_feature_x"));
    assert!(server.sql("SHOW DATABASES").lines().any(|l| l == "shop"), "the parent's database stays");
    // Dropping the parent's own database is refused.
    assert!(database::drop(&paths, &env, &env, "shop", "mysql", "shop").is_err());
}
