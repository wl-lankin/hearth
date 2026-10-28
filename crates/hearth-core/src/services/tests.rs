use std::path::{Path, PathBuf};

use super::kinds::{self, Run};
use super::store::{self, Instance};
use crate::HearthPaths;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, paths)
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// An install folder with the given program files present.
fn install(dir: &Path, programs: &[&str]) -> PathBuf {
    for p in programs {
        let file = dir.join(exe(p));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"x").unwrap();
    }
    dir.to_path_buf()
}

fn args(run: &Run) -> Vec<String> {
    run.args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

#[test]
fn instances_get_their_own_ids_folders_and_secrets() {
    let (_d, paths) = home();
    let first = store::create(&paths, "mysql", "8.4", 3306, None).unwrap();
    let second = store::create(&paths, "mysql", "9.7", 3307, Some("Newer")).unwrap();
    assert_eq!((first.id.as_str(), second.id.as_str()), ("mysql", "mysql-2"));
    assert_eq!((first.name.as_str(), second.name.as_str()), ("MySQL 8.4", "Newer"));
    assert_ne!(store::data_dir(&paths, &first), store::data_dir(&paths, &second));
    let meili = store::create(&paths, "meilisearch", "1", 7700, None).unwrap();
    assert_eq!(meili.secret.len(), 32, "a generated key");
    assert_eq!(store::load(&paths).len(), 3);
    store::delete(&paths, "mysql-2").unwrap();
    assert_eq!(store::load(&paths).iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["mysql", "meilisearch"]);
    assert_eq!(store::create(&paths, "oracle", "1", 1521, None).unwrap_err().to_string(), "Hearth cannot run oracle");
}

#[test]
fn mysql_initializes_once_and_runs_on_its_port() {
    let (d, paths) = home();
    let inst = store::create(&paths, "mysql", "8.4", 3310, None).unwrap();
    let dir = install(&d.path().join("mysql"), &["bin/mysqld", "bin/mysqladmin"]);
    let data = store::data_dir(&paths, &inst);
    let init = kinds::init(&inst, &dir, &data).expect("an empty data folder needs init");
    assert!(args(&init).contains(&"--initialize-insecure".to_string()), "{:?}", args(&init));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("ibdata1"), b"x").unwrap();
    assert!(kinds::init(&inst, &dir, &data).is_none(), "a data folder with files is never initialized again");
    let spec = kinds::spec(&paths, &inst, &dir).unwrap();
    let a: Vec<String> = spec.args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(a[0], "--no-defaults", "my.cnf files elsewhere on the computer are ignored");
    assert!(a.contains(&"--port=3310".to_string()) && a.contains(&"--bind-address=127.0.0.1".to_string()), "{a:?}");
    assert_eq!(spec.ready_port, Some(3310));
    assert_eq!(spec.key, "service-mysql");
    let stop = kinds::stop(&inst, &dir, &data).unwrap();
    assert!(stop.program.ends_with(exe("mysqladmin")) && args(&stop).contains(&"shutdown".to_string()));
}

#[test]
fn postgresql_uses_trust_auth_and_clears_a_stale_lock() {
    let (d, paths) = home();
    let inst = store::create(&paths, "postgresql", "18", 5433, None).unwrap();
    let dir = install(&d.path().join("pg"), &["bin/postgres", "bin/initdb", "bin/pg_ctl"]);
    let data = store::data_dir(&paths, &inst);
    let init = kinds::init(&inst, &dir, &data).unwrap();
    let a = args(&init);
    assert!(
        a.contains(&"-A".to_string()) && a.contains(&"trust".to_string()) && a.contains(&"postgres".to_string()),
        "{a:?}"
    );
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("PG_VERSION"), b"18").unwrap();
    // A lock left by a crash, naming a process that no longer exists.
    std::fs::write(data.join("postmaster.pid"), "4000000000\n/data\n").unwrap();
    kinds::clean_stale(&inst, &data);
    assert!(!data.join("postmaster.pid").exists());
    // A lock naming a live process: on Windows PostgreSQL judges that itself; on macOS the number
    // is often reused by another program after a reboot, so only a live postgres keeps it.
    std::fs::write(data.join("postmaster.pid"), format!("{}\n", std::process::id())).unwrap();
    kinds::clean_stale(&inst, &data);
    assert_eq!(data.join("postmaster.pid").exists(), cfg!(windows), "this test process is alive but is not postgres");
    let spec = kinds::spec(&paths, &inst, &dir).unwrap();
    let a: Vec<String> = spec.args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(a.windows(2).any(|w| w == ["-p", "5433"]), "{a:?}");
    let stop = args(&kinds::stop(&inst, &dir, &data).unwrap());
    assert_eq!(
        stop.last().map(String::as_str),
        Some(data.to_string_lossy().as_ref()),
        "pg_ctl stops this data folder: {stop:?}"
    );
}

#[test]
fn every_kind_finds_its_program_and_binds_to_loopback() {
    let (d, paths) = home();
    let layouts: [(&str, &str, &[&str]); 7] = [
        ("mysql", "8.4", &["bin/mysqld"]),
        ("mariadb", "11.8", &["bin/mariadbd"]),
        ("postgresql", "18", &["bin/postgres"]),
        ("redis", "8", &["redis-server"]),
        ("meilisearch", "1", &["meilisearch"]),
        ("typesense", "30", &["typesense-server"]),
        ("silo", "current", &["silo"]),
    ];
    for (i, (kind, line, programs)) in layouts.iter().enumerate() {
        let inst = store::create(&paths, kind, line, 20000 + i as u16 * 10, None).unwrap();
        let dir = install(&d.path().join(kind), programs);
        let spec = kinds::spec(&paths, &inst, &dir).unwrap_or_else(|e| panic!("{kind}: {e}"));
        let joined = spec.args.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        assert!(joined.contains("127.0.0.1"), "{kind} must bind to loopback: {joined}");
        assert_eq!(spec.ready_port, Some(inst.port), "{kind}");
    }
}

#[test]
fn typesense_uses_its_real_flags_and_keeps_peering_on_loopback() {
    let (d, paths) = home();
    let inst = store::create(&paths, "typesense", "30", 8108, None).unwrap();
    let dir = install(&d.path().join("ts"), &["typesense-server"]);
    let a: Vec<String> =
        kinds::spec(&paths, &inst, &dir).unwrap().args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    for pair in [
        ["--api-address", "127.0.0.1"],
        ["--api-port", "8108"],
        ["--peering-address", "127.0.0.1"],
        ["--peering-port", "8109"],
    ] {
        assert!(a.windows(2).any(|w| w == pair), "{pair:?} missing: {a:?}");
    }
    assert!(!a.iter().any(|x| x == "--listen-address"), "Typesense has no --listen-address: {a:?}");
    assert_eq!(kinds::ports(&inst), [8108, 8109], "the peering port is reserved too");
}

#[test]
fn keys_travel_in_the_environment_not_the_command_line() {
    let (d, paths) = home();
    for (kind, line, program, var) in [
        ("meilisearch", "1", "meilisearch", "MEILI_MASTER_KEY"),
        ("typesense", "30", "typesense-server", "TYPESENSE_API_KEY"),
    ] {
        let inst = store::create(&paths, kind, line, 7700, None).unwrap();
        let dir = install(&d.path().join(kind), &[program]);
        let spec = kinds::spec(&paths, &inst, &dir).unwrap();
        let joined = spec.args.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        assert!(!joined.contains(&inst.secret), "{kind}: the key is visible in the process list: {joined}");
        assert!(spec.env.iter().any(|(k, v)| k == var && v.to_string_lossy() == inst.secret), "{kind}");
    }
}

#[test]
fn redis_saves_its_data_when_it_is_stopped() {
    let (d, paths) = home();
    let inst = store::create(&paths, "redis", "8", 6380, None).unwrap();
    let dir = install(&d.path().join("redis"), &["redis-server", "redis-cli"]);
    let stop = kinds::stop(&inst, &dir, &store::data_dir(&paths, &inst)).expect("Redis is stopped by redis-cli");
    assert!(stop.program.ends_with(exe("redis-cli")), "{:?}", stop.program);
    assert_eq!(args(&stop), ["-h", "127.0.0.1", "-p", "6380", "shutdown", "save"]);
}

#[test]
fn a_missing_program_is_named() {
    let (d, paths) = home();
    let inst = store::create(&paths, "redis", "8", 6380, None).unwrap();
    let err = kinds::spec(&paths, &inst, d.path()).unwrap_err();
    assert_eq!(err.to_string(), "Redis is not installed completely (redis-server is missing)");
}

#[test]
fn env_lines_for_laravel() {
    let (_d, paths) = home();
    let pg = store::create(&paths, "postgresql", "18", 5433, None).unwrap();
    assert_eq!(
        kinds::env(&pg),
        [
            ("DB_CONNECTION", "pgsql"),
            ("DB_HOST", "127.0.0.1"),
            ("DB_PORT", "5433"),
            ("DB_USERNAME", "postgres"),
            ("DB_PASSWORD", "")
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
    );
    let redis = store::create(&paths, "redis", "8", 6380, None).unwrap();
    assert!(kinds::env(&redis).contains(&("REDIS_PORT".into(), "6380".into())));
    let silo = store::create(&paths, "silo", "current", 9000, None).unwrap();
    let env = kinds::env(&silo);
    assert!(env.contains(&("AWS_ENDPOINT".into(), "http://127.0.0.1:9000".into())));
    assert!(env.contains(&("AWS_SECRET_ACCESS_KEY".into(), silo.secret.clone())));
}

#[cfg(unix)]
#[test]
fn the_file_with_the_keys_is_readable_by_the_user_only() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, paths) = home();
    store::create(&paths, "silo", "current", 9000, None).unwrap();
    let mode = std::fs::metadata(paths.config.join("services.json")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn instances_survive_a_reload() {
    let (_d, paths) = home();
    let inst = store::create(&paths, "redis", "8", 6380, None).unwrap();
    store::set_autostart(&paths, &inst.id, true).unwrap();
    store::set_port(&paths, &inst.id, 6390).unwrap();
    let back: Instance = store::load(&paths).into_iter().next().unwrap();
    assert_eq!((back.port, back.autostart), (6390, true));
}
