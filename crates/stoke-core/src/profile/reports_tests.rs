use std::path::Path;

use serde_json::json;

use crate::profile::reports::{clear, list};

/// Writes one SPX report (metadata json plus the report itself) as SPX names them.
fn report(dir: &Path, key: &str, ts: u64, uri: &str) {
    let meta = json!({
        "key": key, "exec_ts": ts, "host_name": "mac", "process_pid": 1, "process_tid": 1,
        "process_pwd": "/srv/shop/public", "cli": 0, "cli_command_line": "",
        "http_request_uri": uri, "http_method": "GET", "http_host": "shop.test:8080",
        "custom_metadata_str": null, "wall_time_ms": 42, "peak_memory_usage": 2097152,
        "called_function_count": 310, "call_count": 5000, "recorded_call_count": 5000,
    });
    std::fs::write(dir.join(format!("{key}.json")), meta.to_string()).unwrap();
    std::fs::write(dir.join(format!("{key}.txt.gz")), b"gz").unwrap();
}

#[test]
fn reports_come_newest_first_with_what_the_page_shows() {
    let d = tempfile::tempdir().unwrap();
    report(d.path(), "spx-full-1", 1_700_000_000, "/old");
    report(d.path(), "spx-full-2", 1_700_000_100, "/orders?page=2");
    let reports = list(d.path(), 200);
    assert_eq!(reports.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["spx-full-2", "spx-full-1"]);
    let r = &reports[0];
    assert_eq!(
        (r.time, r.method.as_str(), r.uri.as_str(), r.host.as_str()),
        (1_700_000_100, "GET", "/orders?page=2", "shop.test")
    );
    assert_eq!((r.wall_ms, r.peak_memory, r.functions, r.cli), (42, 2_097_152, 310, false));
}

#[test]
fn the_list_is_capped_and_broken_files_are_skipped() {
    let d = tempfile::tempdir().unwrap();
    for i in 0..5 {
        report(d.path(), &format!("spx-full-{i}"), 1_700_000_000 + i, "/");
    }
    std::fs::write(d.path().join("spx-full-half.json"), "{\"key\": \"spx-full-ha").unwrap();
    std::fs::write(d.path().join("spx-full-odd.json"), "[1, 2]").unwrap();
    std::fs::write(d.path().join("notes.txt"), "x").unwrap();
    let reports = list(d.path(), 3);
    assert_eq!(reports.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["spx-full-4", "spx-full-3", "spx-full-2"]);
    assert!(list(&d.path().join("missing"), 200).is_empty());
}

#[test]
fn a_command_line_profile_shows_its_command() {
    let d = tempfile::tempdir().unwrap();
    let meta = json!({ "key": "spx-full-cli", "exec_ts": 5, "cli": 1, "cli_command_line": "php artisan migrate",
        "http_request_uri": "", "http_method": "", "http_host": "", "wall_time_ms": 900,
        "peak_memory_usage": 1, "called_function_count": 2 });
    std::fs::write(d.path().join("spx-full-cli.json"), meta.to_string()).unwrap();
    let r = &list(d.path(), 200)[0];
    assert!(r.cli);
    assert_eq!(r.uri, "php artisan migrate");
}

#[test]
fn clear_deletes_reports_only() {
    let d = tempfile::tempdir().unwrap();
    report(d.path(), "spx-full-1", 1, "/");
    std::fs::write(d.path().join("keep.txt"), "x").unwrap();
    assert_eq!(clear(d.path()).unwrap(), 1);
    let left: Vec<String> =
        std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left, ["keep.txt"]);
    assert_eq!(clear(&d.path().join("missing")).unwrap(), 0);
}
