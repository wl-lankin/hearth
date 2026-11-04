use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::plan::{plan, Area};
use super::probe::probe;
use super::scan::scan;
use super::scan_tests::fake_herd;
use crate::HearthPaths;

fn home() -> (tempfile::TempDir, HearthPaths) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, paths)
}

#[test]
fn the_plan_lists_every_item_with_what_happens() {
    let (d, paths) = home();
    let (herd, machine) = fake_herd(d.path());
    let inv = scan(&herd, &machine).unwrap();
    let plan = plan(&paths, &inv, &["8.4".to_string()]);
    let find = |area: Area, what: &str| {
        plan.items
            .iter()
            .find(|i| i.area == area && i.what.contains(what))
            .unwrap_or_else(|| panic!("no {area:?} item for {what}: {:#?}", plan.items))
    };

    assert!(find(Area::Sites, "vite.test").moved);
    assert!(find(Area::Php, "PHP 8.1").action.contains("install"), "8.1 is missing in Hearth");
    assert!(plan.items.iter().all(|i| !(i.area == Area::Php && i.what == "PHP 8.4")), "8.4 is there already");
    assert!(find(Area::Php, "default").action.contains("8.5"));
    assert!(find(Area::Php, "memory_limit").moved);
    assert!(find(Area::Php, "auto_prepend_file").action.contains("Hearth's dump loader"));
    let mongo = find(Area::Php, "mongodb");
    assert!(!mongo.moved);
    assert_eq!(mongo.reason.as_deref(), Some("Hearth's PHP builds have no mongodb extension"));
    assert!(find(Area::Php, "redis").moved);

    assert!(find(Area::Https, "certificate authority").moved);
    assert!(find(Area::Https, "ports").action.contains("80 and 443"));
    assert!(find(Area::Redis, "6379").action.contains("data"));
    assert!(find(Area::Mail, "2525").moved);
    let mariadb = find(Area::MariaDb, "MariaDB 10.11");
    assert!(!mariadb.moved);
    assert_eq!(mariadb.reason.as_deref(), Some("It is not running; your other databases are not touched"));
    assert!(find(Area::Node, "v21.7.0").moved);
    assert!(find(Area::Node, "default").action.contains("v22.9.0"));
    assert!(find(Area::Path, "herd").action.contains("your PATH"));
    assert!(find(Area::Path, "NVM_HOME").moved);
    assert!(find(Area::Mcp, "herd").action.contains("Hearth"));
    assert!(find(Area::Autostart, "Herd").action.contains("off"));
    let workspace = find(Area::Hosts, "bcd-1.api.shop.test");
    assert!(!workspace.moved);
    assert_eq!(workspace.reason.as_deref(), Some("Not a site (workspace or Herd's own)"));
    assert_eq!(plan.sites.proxy, [("vite".to_string(), "http://localhost:5173".to_string())]);
}

/// A server that answers every request with `reply` and hands back the Host header it saw.
async fn server(reply: &'static str) -> (u16, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        stream.write_all(reply.as_bytes()).await.unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
        request.lines().find_map(|l| l.strip_prefix("host: ").map(str::to_string)).unwrap_or_default()
    });
    (port, task)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_probe_records_status_and_redirect_without_following_it() {
    let (port, seen) =
        server("HTTP/1.1 302 Found\r\nLocation: /login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
    let answer = probe("shop.test", false, port, 1).await;
    assert_eq!(answer.status, Some(302));
    assert_eq!(answer.location.as_deref(), Some("/login"));
    assert_eq!(answer.error, None);
    assert!(seen.await.unwrap().starts_with("shop.test"));

    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let down = probe("shop.test", false, free, 1).await;
    assert_eq!(down.status, None);
    assert!(down.error.is_some());
}
