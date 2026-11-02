use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::insights::collector::{self, Collector, CollectorConfig, KEEP, TEXT_MAX};
use crate::HearthPaths;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

async fn started() -> (Collector, u16) {
    let c = Collector::default();
    let port = free_port();
    c.start(port).await.unwrap();
    (c, port)
}

async fn send(port: u16, bytes: &[u8]) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(bytes).await.unwrap();
    s.shutdown().await.unwrap();
}

fn line(text: &str) -> String {
    let v = json!({"type": "dump", "site": "shop", "file": "/srv/shop/app/X.php", "line": 12, "sapi": "fpm-fcgi", "uri": "/cart", "text": text});
    format!("{v}\n")
}

/// Waits until the collector holds `n` dumps (they arrive on another task).
async fn wait_for(c: &Collector, n: usize) {
    for _ in 0..100 {
        if c.dumps().len() >= n {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("only {} dumps arrived", c.dumps().len());
}

#[tokio::test]
async fn a_dump_line_is_kept_newest_first() {
    let (c, port) = started().await;
    send(port, line("first").as_bytes()).await;
    wait_for(&c, 1).await;
    send(port, line("second").as_bytes()).await;
    wait_for(&c, 2).await;
    let dumps = c.dumps();
    assert_eq!(dumps.iter().map(|d| d.text.as_str()).collect::<Vec<_>>(), ["second", "first"]);
    let d = &dumps[1];
    assert_eq!((d.site.as_deref(), d.file.as_deref(), d.line), (Some("shop"), Some("/srv/shop/app/X.php"), Some(12)));
    assert_eq!((d.sapi.as_str(), d.uri.as_deref(), d.truncated), ("fpm-fcgi", Some("/cart"), false));
    assert!(dumps[0].id > dumps[1].id && d.time > 0);
    c.clear();
    assert!(c.dumps().is_empty());
}

#[tokio::test]
async fn many_lines_on_one_connection_and_garbage_between() {
    let (c, port) = started().await;
    let body = format!(
        "{}not json\n{{\"type\":\"other\",\"text\":\"x\"}}\n{{\"type\":\"dump\"}}\n{}",
        line("one"),
        line("two")
    );
    send(port, body.as_bytes()).await;
    wait_for(&c, 2).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(c.dumps().iter().map(|d| d.text.as_str()).collect::<Vec<_>>(), ["two", "one"]);
}

#[tokio::test]
async fn text_over_256_kb_is_cut_and_marked() {
    let (c, port) = started().await;
    send(port, line(&"é".repeat(TEXT_MAX / 2 + 1000)).as_bytes()).await;
    wait_for(&c, 1).await;
    let d = &c.dumps()[0];
    assert!(d.truncated);
    assert!(d.text.len() <= TEXT_MAX, "{}", d.text.len());
    let huge = format!("{}\n{}", "x".repeat(400 * 1024), line("after"));
    send(port, huge.as_bytes()).await;
    wait_for(&c, 2).await;
    assert_eq!(c.dumps()[0].text, "after", "an oversized line is skipped whole");
}

#[tokio::test]
async fn only_the_newest_are_kept() {
    let (c, port) = started().await;
    let body: String = (0..KEEP + 3).map(|i| line(&i.to_string())).collect();
    send(port, body.as_bytes()).await;
    wait_for(&c, KEEP).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let dumps = c.dumps();
    assert_eq!(dumps.len(), KEEP);
    assert_eq!(dumps[0].text, (KEEP + 2).to_string());
    assert_eq!(dumps[KEEP - 1].text, "3");
}

#[tokio::test]
async fn an_http_request_is_dropped() {
    let (c, port) = started().await;
    let body = format!("POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 300\r\n\r\n{}", line("forged"));
    send(port, body.as_bytes()).await;
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(c.dumps().is_empty());
    send(port, line("real").as_bytes()).await;
    wait_for(&c, 1).await;
}

#[tokio::test]
async fn a_taken_port_keeps_the_old_one() {
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = blocker.local_addr().unwrap().port();
    let c = Collector::default();
    let err = c.start(taken).await.unwrap_err().to_string();
    assert!(err.contains(&format!("Port {taken} is taken")), "{err}");
    assert_eq!((c.status().listening, c.status().error.is_some()), (false, true));
    let (c, port) = (c, free_port());
    c.start(port).await.unwrap();
    assert!(c.start(taken).await.is_err());
    let s = c.status();
    assert_eq!((s.port, s.listening, s.error), (port, true, None));
    c.stop();
    assert!(!c.status().listening);
}

#[test]
fn config_defaults_and_roundtrip() {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    assert_eq!(collector::config(&paths), CollectorConfig { port: 2306, dumps: true });
    collector::set_config(&paths, &CollectorConfig { port: 2400, dumps: false }).unwrap();
    assert_eq!(collector::config(&paths), CollectorConfig { port: 2400, dumps: false });
    std::fs::write(paths.config.join("collector.json"), "nope").unwrap();
    assert_eq!(collector::config(&paths), CollectorConfig { port: 2306, dumps: true });
}
