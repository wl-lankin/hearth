use std::io::Write;

use crate::insights::logs::{self, parse, ENTRIES_MAX};
use crate::sites::registry;
use crate::HearthPaths;

const LOG: &str = "[2026-11-03 10:00:00] local.INFO: User 3 logged in
[2026-11-03 10:00:05] local.WARNING: Slow cart {\"user\":3,\"ms\":812}
[2026-11-03 10:00:09] local.ERROR: Division by zero {\"exception\":\"[object] (DivisionByZeroError(code: 0): Division by zero at /srv/shop/app/Pricing.php:18)
[stacktrace]
#0 /srv/shop/app/Http/Controllers/CartController.php(40): App\\Pricing->total()
#1 {main}
\"}
";

fn home() -> (tempfile::TempDir, HearthPaths) {
    let d = tempfile::tempdir().unwrap();
    let paths = HearthPaths::at(d.path().join("hearth"));
    paths.ensure().unwrap();
    (d, paths)
}

/// A linked site `shop` with `storage/logs`; returns the logs folder.
fn shop(d: &tempfile::TempDir, paths: &HearthPaths) -> std::path::PathBuf {
    let dir = d.path().join("shop");
    let logs = dir.join("storage").join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::create_dir_all(dir.join("public")).unwrap();
    registry::link(paths, "shop", &dir).unwrap();
    logs
}

#[test]
fn parse_splits_entries_with_their_stack_traces() {
    let entries = parse(LOG);
    assert_eq!(entries.len(), 3);
    let levels: Vec<&str> = entries.iter().map(|e| e.level.as_str()).collect();
    assert_eq!(levels, ["error", "warning", "info"], "newest first");
    let error = &entries[0];
    assert_eq!((error.time.as_str(), error.env.as_str()), ("2026-11-03 10:00:09", "local"));
    assert!(error.message.starts_with("Division by zero {"), "{}", error.message);
    assert!(error.detail.contains("#0 /srv/shop/app/Http/Controllers/CartController.php(40)"), "{}", error.detail);
    assert!(error.detail.ends_with("\"}"), "{:?}", error.detail);
    assert_eq!(entries[2].message, "User 3 logged in");
    assert_eq!(entries[2].detail, "");
}

#[test]
fn parse_handles_iso_dates_and_other_channels() {
    let text =
        "[2026-11-03T10:00:00.123456+00:00] production.CRITICAL: Down\n[2026-11-03 10:01:00] my-app_1.DEBUG: ok\n";
    let entries = parse(text);
    assert_eq!(entries.len(), 2);
    assert_eq!((entries[1].time.as_str(), entries[1].level.as_str()), ("2026-11-03T10:00:00.123456+00:00", "critical"));
    assert_eq!((entries[0].env.as_str(), entries[0].level.as_str()), ("my-app_1", "debug"));
}

#[test]
fn a_cut_first_entry_is_dropped_and_lines_without_a_head_are_kept_alone() {
    let entries = parse("#3 tail of a trace\n{main}\n[2026-11-03 10:00:00] local.INFO: whole\n");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message, "whole");
    let bare = parse("just text\nno heads at all\n");
    assert!(bare.is_empty());
}

#[test]
fn only_the_newest_entries_are_kept_and_long_ones_cut() {
    let many: String = (0..ENTRIES_MAX + 5).map(|i| format!("[2026-11-03 10:00:00] local.INFO: n{i}\n")).collect();
    let entries = parse(&many);
    assert_eq!(entries.len(), ENTRIES_MAX);
    assert_eq!(entries[0].message, format!("n{}", ENTRIES_MAX + 4));
    let long = format!("[2026-11-03 10:00:00] local.ERROR: big\n{}\n", "x".repeat(100_000));
    let e = &parse(&long)[0];
    assert!(e.truncated && e.detail.len() <= 64 * 1024);
}

#[test]
fn sites_lists_log_files_newest_first_and_skips_proxies() {
    let (d, paths) = home();
    let logs = shop(&d, &paths);
    std::fs::write(logs.join("laravel-2026-11-02.log"), "x").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(logs.join("laravel-2026-11-03.log"), "xy").unwrap();
    std::fs::write(logs.join(".gitignore"), "*").unwrap();
    std::fs::create_dir_all(logs.join("old.log")).unwrap();
    registry::proxy(&paths, "api", "http://localhost:3000").unwrap();
    let all = logs::sites(&paths);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].site, "shop");
    let names: Vec<&str> = all[0].files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["laravel-2026-11-03.log", "laravel-2026-11-02.log"]);
    assert_eq!(all[0].files[0].size, 2);
}

#[test]
fn read_takes_only_the_last_2_mb() {
    let (d, paths) = home();
    let logs = shop(&d, &paths);
    let mut f = std::fs::File::create(logs.join("laravel.log")).unwrap();
    for i in 0..60_000 {
        writeln!(f, "[2026-11-03 10:00:00] local.INFO: filler line number {i:06} {}", "z".repeat(40)).unwrap();
    }
    writeln!(f, "[2026-11-03 10:00:01] local.ERROR: the last one").unwrap();
    drop(f);
    assert!(std::fs::metadata(logs.join("laravel.log")).unwrap().len() > 5 * 1024 * 1024);
    let (entries, size) = logs::read(&paths, "shop", "laravel.log").unwrap();
    assert_eq!(size, std::fs::metadata(logs.join("laravel.log")).unwrap().len());
    assert_eq!(entries[0].message, "the last one");
    assert_eq!(entries.len(), ENTRIES_MAX);
}

#[test]
fn bad_names_are_refused() {
    let (d, paths) = home();
    let logs = shop(&d, &paths);
    std::fs::write(logs.join("laravel.log"), LOG).unwrap();
    std::fs::write(d.path().join("shop").join("secret.log"), "x").unwrap();
    for name in [r"..\secret.log", "../secret.log", "a/b.log", "laravel.txt", "..", "", "laravel.log/../x.log"] {
        assert!(logs::read(&paths, "shop", name).is_err(), "{name}");
        assert!(logs::clear(&paths, "shop", name).is_err(), "{name}");
    }
    assert!(logs::read(&paths, "nope", "laravel.log").is_err());
}

#[test]
fn clear_empties_the_file() {
    let (d, paths) = home();
    let logs = shop(&d, &paths);
    std::fs::write(logs.join("laravel.log"), LOG).unwrap();
    logs::clear(&paths, "shop", "laravel.log").unwrap();
    assert_eq!(std::fs::metadata(logs.join("laravel.log")).unwrap().len(), 0);
    assert!(logs::read(&paths, "shop", "laravel.log").unwrap().0.is_empty());
}

#[test]
fn non_utf8_bytes_do_not_fail() {
    let (d, paths) = home();
    let logs = shop(&d, &paths);
    std::fs::write(logs.join("laravel.log"), b"[2026-11-03 10:00:00] local.INFO: caf\xe9\n").unwrap();
    let (entries, _) = logs::read(&paths, "shop", "laravel.log").unwrap();
    assert_eq!(entries[0].message, "caf\u{fffd}");
}

/// A project could hold `storage/logs/x.log` as a link to a file elsewhere: Hearth never lists,
/// reads or empties it.
#[test]
fn a_linked_log_file_is_never_followed() {
    let (d, paths) = home();
    let logs = shop(&d, &paths);
    let outside = d.path().join("precious.txt");
    std::fs::write(&outside, "keep me").unwrap();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&outside, logs.join("evil.log"));
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outside, logs.join("evil.log"));
    if linked.is_err() {
        return eprintln!("skipped: this account cannot create symlinks");
    }
    std::fs::write(logs.join("laravel.log"), "[2026-11-03 10:00:00] local.INFO: real\n").unwrap();
    let names: Vec<String> = logs::sites(&paths)[0].files.iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, ["laravel.log"]);
    assert!(logs::read(&paths, "shop", "evil.log").is_err());
    assert!(logs::clear(&paths, "shop", "evil.log").is_err());
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep me");
}
