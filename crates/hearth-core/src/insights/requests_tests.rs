use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::insights::collector::Collector;
use crate::insights::requests::{analyse, normalize, Query, KEEP_REQUESTS};

fn q(sql: &str, bindings: &[&str], ms: f64, line: u32) -> Query {
    Query {
        sql: sql.into(),
        bindings: bindings.iter().map(|b| b.to_string()).collect(),
        ms,
        start_ms: 0.0,
        connection: "sqlite".into(),
        file: Some("/srv/shop/app/Http/Controllers/CartController.php".into()),
        line: Some(line),
    }
}

#[test]
fn normalize_folds_lists_and_literals() {
    assert_eq!(
        normalize("select *  from \"items\" where \"id\" in (?, ?, ?) and price > 10.5 and name = 'tea'"),
        "select * from \"items\" where \"id\" in (?) and price > ? and name = ?"
    );
    assert_eq!(normalize("select * from t where id in (1, 2)"), normalize("select * from t where id in (7)"));
    assert_eq!(normalize("select * from t2"), "select * from t2", "digits inside names stay");
}

#[test]
fn n_plus_one_needs_five_from_one_line() {
    let sql = "select * from \"items\" where \"cart_id\" = ?";
    let five: Vec<Query> = (0..5).map(|i| q(sql, &[&i.to_string()], 1.0, 40)).collect();
    let found = analyse(&five);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, "n+1");
    assert_eq!(found[0].queries, vec![0, 1, 2, 3, 4]);
    assert!(found[0].text.contains("5 times") && found[0].text.contains("CartController.php:40"), "{}", found[0].text);

    let four: Vec<Query> = (0..4).map(|i| q(sql, &[&i.to_string()], 1.0, 40)).collect();
    assert!(analyse(&four).is_empty());
    let split: Vec<Query> = (0..6).map(|i| q(sql, &[&i.to_string()], 1.0, 40 + i % 2)).collect();
    assert!(analyse(&split).is_empty(), "three from each of two lines is not N+1");
}

#[test]
fn duplicates_need_equal_bindings() {
    let sql = "select * from \"users\" where \"id\" = ?";
    let found = analyse(&[q(sql, &["3"], 1.0, 10), q(sql, &["4"], 1.0, 11), q(sql, &["3"], 1.0, 12)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].kind.as_str(), found[0].queries.clone()), ("duplicate", vec![0, 2]));
    assert!(found[0].text.contains("2 times"), "{}", found[0].text);
}

#[test]
fn slow_is_100_ms_or_more() {
    let found = analyse(&[q("select 1", &[], 99.9, 1), q("select 2", &[], 100.0, 2)]);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].kind.as_str(), found[0].queries.clone()), ("slow", vec![1]));
    assert!(found[0].text.contains("100 ms"), "{}", found[0].text);
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn request_line(uri: &str, queries: usize) -> String {
    let qs: Vec<_> = (0..queries)
        .map(|i| {
            json!({"sql": "select * from \"items\" where \"cart_id\" = ?", "bindings": [i.to_string()], "ms": 1.5,
            "start": i as f64 * 2.0, "connection": "sqlite", "file": "/srv/shop/app/X.php", "line": 7})
        })
        .collect();
    let v = json!({"type": "request", "site": "shop.test", "sapi": "cli-server", "method": "GET", "uri": uri,
        "command": null, "status": 200, "ms": 42.5, "memory": 4194304, "dropped": 3, "queries": qs});
    format!("{v}\n")
}

async fn send(port: u16, text: &str) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(text.as_bytes()).await.unwrap();
    s.shutdown().await.unwrap();
}

async fn wait_for(c: &Collector, n: usize) {
    for _ in 0..200 {
        if c.requests_after(0).len() >= n {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("only {} requests arrived", c.requests_after(0).len());
}

#[tokio::test]
async fn a_request_line_is_kept_with_findings() {
    let c = Collector::default();
    let port = free_port();
    c.start(port).await.unwrap();
    send(port, &request_line("/cart", 6)).await;
    wait_for(&c, 1).await;
    let summary = &c.requests_after(0)[0];
    assert_eq!(
        (summary.uri.as_deref(), summary.status, summary.query_count, summary.dropped),
        (Some("/cart"), Some(200), 6, 3)
    );
    assert_eq!(summary.query_ms, 9.0);
    assert_eq!(summary.findings, vec!["n+1".to_string()]);
    let full = c.request(summary.id).unwrap();
    assert_eq!((full.queries.len(), full.queries[2].start_ms), (6, 4.0));
    assert_eq!(full.findings[0].kind, "n+1");
    assert!(c.request(summary.id + 1).is_none());
    c.clear_requests();
    assert!(c.requests_after(0).is_empty());
    c.stop();
}

#[tokio::test]
async fn only_the_newest_requests_are_kept() {
    let c = Collector::default();
    let port = free_port();
    c.start(port).await.unwrap();
    let body: String = (0..KEEP_REQUESTS + 2).map(|i| request_line(&format!("/{i}"), 0)).collect();
    send(port, &body).await;
    wait_for(&c, KEEP_REQUESTS).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let all = c.requests_after(0);
    assert_eq!(all.len(), KEEP_REQUESTS);
    assert_eq!(all[0].uri.as_deref(), Some(format!("/{}", KEEP_REQUESTS + 1).as_str()));
    let newest = all[0].id;
    assert!(c.requests_after(newest).is_empty());
    c.stop();
}
