use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::run::{start, start_autostart, status, stop, stop_all};
use super::store;
use crate::ipc::CoreState;
use crate::HearthPaths;

/// Service ports are picked fresh per test; tests touching processes still run one at a time.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn fake_service() -> PathBuf {
    let deps = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    deps.parent().unwrap().join("examples").join(format!("fake_service{}", std::env::consts::EXE_SUFFIX))
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn answers(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok()
}

/// An installed MySQL 8.4 whose programs are the fake service.
fn installed_mysql(paths: &HearthPaths) -> PathBuf {
    let dir = paths.data.join("services").join("bin").join("mysql").join("8.4.11");
    for program in ["bin/mysqld", "bin/mysqladmin"] {
        let file = dir.join(format!("{program}{}", std::env::consts::EXE_SUFFIX));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::copy(fake_service(), &file).unwrap();
    }
    dir
}

fn home() -> (tempfile::TempDir, CoreState) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, CoreState::new(paths))
}

fn calls(install: &Path) -> String {
    std::fs::read_to_string(install.join("bin").join("calls.log")).unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_database_is_set_up_once_then_runs_on_its_port() {
    let _one = ONE_AT_A_TIME.lock().await;
    let (_d, core) = home();
    let install = installed_mysql(&core.paths);
    let inst = store::create(&core.paths, "mysql", "8.4", free_port(), None).unwrap();
    start(&core, &inst.id).await.unwrap();
    assert!(answers(inst.port));
    assert!(store::data_dir(&core.paths, &inst).join("initialized").exists());
    let row = status(&core).into_iter().find(|s| s.id == inst.id).unwrap();
    assert!(row.running && row.error.is_none(), "{row:?}");
    stop(&core, &inst.id).await.unwrap();
    start(&core, &inst.id).await.unwrap();
    assert_eq!(calls(&install).matches("--initialize-insecure").count(), 1, "set up only once");
    stop_all(&core).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_setup_that_dies_halfway_is_redone_on_the_next_start() {
    let _one = ONE_AT_A_TIME.lock().await;
    let (_d, core) = home();
    let install = installed_mysql(&core.paths);
    std::fs::write(install.join("bin").join("fail-init"), b"1").unwrap();
    let inst = store::create(&core.paths, "mysql", "8.4", free_port(), None).unwrap();
    let err = start(&core, &inst.id).await.unwrap_err().to_string();
    assert!(err.contains("setup failed"), "{err}");
    let data = store::data_dir(&core.paths, &inst);
    assert!(!data.join("ibdata1").exists(), "no half set-up data folder is left to block the next start");
    std::fs::remove_file(install.join("bin").join("fail-init")).unwrap();
    start(&core, &inst.id).await.unwrap();
    assert!(data.join("initialized").exists());
    stop_all(&core).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_while_a_start_runs_leaves_nothing_running() {
    let _one = ONE_AT_A_TIME.lock().await;
    let (_d, core) = home();
    installed_mysql(&core.paths);
    let inst = store::create(&core.paths, "mysql", "8.4", free_port(), None).unwrap();
    let starting = {
        let (core, id) = (core.clone(), inst.id.clone());
        tokio::spawn(async move { start(&core, &id).await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    super::run::delete(&core, &inst.id).await.unwrap();
    let _ = starting.await.unwrap();
    assert!(core.supervisor.status().is_empty(), "{:?}", core.supervisor.status());
    assert!(!answers(inst.port));
    assert!(store::load(&core.paths).is_empty());
}

#[test]
fn creates_at_the_same_time_get_their_own_ids() {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let paths = paths.clone();
            std::thread::spawn(move || store::create(&paths, "redis", "8", 7000 + i, None).unwrap().id)
        })
        .collect();
    let ids: std::collections::BTreeSet<String> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(ids.len(), 8, "{ids:?}");
    assert_eq!(store::load(&paths).len(), 8);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_taken_port_is_reported_without_starting() {
    let _one = ONE_AT_A_TIME.lock().await;
    let (_d, core) = home();
    installed_mysql(&core.paths);
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let inst = store::create(&core.paths, "mysql", "8.4", port, None).unwrap();
    let err = start(&core, &inst.id).await.unwrap_err().to_string();
    assert!(err.starts_with(&format!("Port {port} is used by another program")), "{err}");
    let row = status(&core).into_iter().find(|s| s.id == inst.id).unwrap();
    assert!(!row.running);
    assert_eq!(row.error.as_deref(), Some(err.as_str()), "the error stays visible");
    assert!(core.supervisor.status().is_empty(), "nothing was started, so nothing restarts");
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_a_database_asks_it_to_shut_down_first() {
    let _one = ONE_AT_A_TIME.lock().await;
    let (_d, core) = home();
    let install = installed_mysql(&core.paths);
    let inst = store::create(&core.paths, "mysql", "8.4", free_port(), None).unwrap();
    start(&core, &inst.id).await.unwrap();
    stop(&core, &inst.id).await.unwrap();
    assert!(install.join("bin").join("shutdown.called").exists(), "mysqladmin shutdown ran");
    assert!(!answers(inst.port));
    assert!(core.supervisor.status().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn autostart_instances_start_and_quitting_stops_them_all() {
    let _one = ONE_AT_A_TIME.lock().await;
    let (_d, core) = home();
    installed_mysql(&core.paths);
    let on = store::create(&core.paths, "mysql", "8.4", free_port(), None).unwrap();
    let off = store::create(&core.paths, "mysql", "8.4", free_port(), Some("Manual")).unwrap();
    store::set_autostart(&core.paths, &on.id, true).unwrap();
    start_autostart(&core).await;
    assert!(answers(on.port));
    assert!(!answers(off.port));
    stop_all(&core).await;
    assert!(!answers(on.port));
    assert!(core.supervisor.status().is_empty());
}
