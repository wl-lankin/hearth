use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use crate::insights::collector::{Collector, CollectorConfig};
use crate::insights::prepend::{script_path, write_prepend};
use crate::StokePaths;

macro_rules! need_php {
    () => {
        match php() {
            Some(p) => p,
            None => {
                eprintln!("skipped: no PHP 8.1+ (set STOKE_TEST_PHP or put php on PATH)");
                return;
            }
        }
    };
}

/// Dumps and requests on, sending to `port`.
fn on(port: u16) -> CollectorConfig {
    CollectorConfig { port, dumps: true, requests: true }
}

/// Just enough of Laravel for the hook, in the project's `vendor/` as in a real app: the container
/// with its instance, an events dispatcher, the query event and `Env`.
const FAKE_LARAVEL: &str = r#"<?php
namespace Illuminate\Container {
    class Container {
        protected static $instance;
        public static function getInstance() { return static::$instance ??= new static; }
        public static function setInstance($c = null) { return static::$instance = $c; }
        public function bound($key) { return false; }
        public function make($key, array $parameters = []) { return null; }
    }
}
namespace Illuminate\Support {
    class Env { public static function get($key, $default = null) { return $default; } }
}
namespace Illuminate\Database\Events {
    class QueryExecuted {
        public function __construct(public $sql, public $bindings, public $time, public $connectionName = 'sqlite') {}
    }
}
namespace Fake {
    class Events {
        public array $listeners = [];
        public function listen($event, $listener) { $this->listeners[$event][] = $listener; }
        public function query(string $sql, array $bindings, float $ms) {
            $event = new \Illuminate\Database\Events\QueryExecuted($sql, $bindings, $ms);
            foreach ($this->listeners[\Illuminate\Database\Events\QueryExecuted::class] ?? [] as $l) {
                // As Laravel's Dispatcher::makeListener: a string is a class to resolve, never a function.
                if (is_string($l)) { throw new \RuntimeException("Target class [$l] does not exist."); }
                $l($event);
            }
        }
    }
    class App extends \Illuminate\Container\Container {
        public function __construct(public Events $events) { static::setInstance($this); }
        public function bound($key) { return $key === 'events'; }
        public function make($key, array $parameters = []) { return $key === 'events' ? $this->events : null; }
    }
}
"#;

/// Runs `body` as `project/s.php <args>` in a project with an `artisan` file and the fake Laravel
/// in `vendor/fake.php`.
fn run_laravel(php: &Path, d: &Path, paths: &StokePaths, body: &str, args: &[&str]) -> Output {
    let project = d.join("project");
    std::fs::create_dir_all(project.join("vendor")).unwrap();
    std::fs::write(project.join("artisan"), "<?php\n").unwrap();
    std::fs::write(project.join("vendor").join("fake.php"), FAKE_LARAVEL).unwrap();
    std::fs::write(project.join("s.php"), body).unwrap();
    let prepend = format!("auto_prepend_file=\"{}\"", script_path(paths).display());
    Command::new(php).args(["-n", "-d", &prepend, "s.php"]).args(args).current_dir(&project).output().unwrap()
}

async fn wait_for_requests(c: &Collector, n: usize) {
    for _ in 0..200 {
        if c.requests_after(0).len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("only {} requests arrived", c.requests_after(0).len());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_laravel_request_is_sent_with_its_queries_and_callers() {
    let php = need_php!();
    let (d, paths) = home();
    let (collector, port) = (Collector::default(), free_port());
    collector.start(port).await.unwrap();
    write_prepend(&paths, &on(port)).unwrap();
    let body = r#"<?php
require __DIR__ . '/vendor/fake.php';
$events = new Fake\Events();
new Fake\App($events);
echo env('APP_NAME', 'shop'), "\n";
$events->query('select * from "items" where "id" = ?', [1], 1.5);
$events->query('select * from "items" where "id" = ?', [2], 2.5);
$events->query('select * from "users" where "secret" = ?', [str_repeat('x', 500)], 120.0);
echo config() === null ? "no config\n" : "config\n";
"#;
    let (p, dir) = (paths.clone(), d.path().to_path_buf());
    let out =
        tokio::task::spawn_blocking(move || run_laravel(&php, &dir, &p, body, &["migrate", "--seed"])).await.unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.starts_with("shop\n"), "Laravel's env() body: {stdout}");
    wait_for_requests(&collector, 1).await;
    let summary = &collector.requests_after(0)[0];
    assert_eq!(
        (summary.command.as_deref(), summary.sapi.as_str(), summary.query_count),
        (Some("migrate --seed"), "cli", 3)
    );
    assert_eq!(summary.site.as_deref(), Some("project"));
    let full = collector.request(summary.id).unwrap();
    let lines: Vec<Option<u32>> = full.queries.iter().map(|q| q.line).collect();
    assert_eq!(lines, [Some(6), Some(7), Some(8)], "the app's lines, not the dispatcher's in vendor/");
    assert!(full.queries[0].file.as_deref().unwrap().ends_with("s.php"));
    assert_eq!(full.queries[2].bindings[0].len(), 100, "bindings are cut");
    assert!(full.queries[1].start_ms >= 0.0 && full.queries[1].ms == 2.5);
    assert_eq!(summary.findings, vec!["slow".to_string()]);
    collector.stop();
}

/// Runs a fake-Laravel script and returns the one request the collector got.
async fn one_request(body: &'static str) -> (crate::insights::requests::Request, String) {
    let php = php().unwrap();
    let (d, paths) = home();
    let (collector, port) = (Collector::default(), free_port());
    collector.start(port).await.unwrap();
    write_prepend(&paths, &on(port)).unwrap();
    let (p, dir) = (paths.clone(), d.path().to_path_buf());
    let out = tokio::task::spawn_blocking(move || run_laravel(&php, &dir, &p, body, &["db:seed"])).await.unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    wait_for_requests(&collector, 1).await;
    let id = collector.requests_after(0)[0].id;
    let request = collector.request(id).unwrap();
    collector.stop();
    (request, said)
}

/// A seeder's big inserts: the line stays under the collector's limit, the rest is counted.
#[tokio::test(flavor = "multi_thread")]
async fn a_seeder_stays_under_the_line_limit() {
    if php().is_none() {
        return eprintln!("skipped: no PHP 8.1+");
    }
    let body = r#"<?php
require __DIR__ . '/vendor/fake.php';
$events = new Fake\Events();
new Fake\App($events);
env('X');
for ($i = 0; $i < 600; $i++) {
    $events->query('insert into "posts" ("a", "b") values ' . str_repeat('(?, ?), ', 300), array_fill(0, 50, str_repeat('y', 100)), 0.5);
}
"#;
    let (request, _) = one_request(body).await;
    assert!(request.dropped > 0, "{}", request.dropped);
    assert_eq!(request.queries.len() + request.dropped as usize, 600);
}

/// Laravel registers its own shutdown handler after Stoke's hook ran (error pages render there):
/// Stoke's request goes out after it, with its queries and the status it set.
#[tokio::test(flavor = "multi_thread")]
async fn the_request_is_sent_after_laravels_shutdown_handler() {
    if php().is_none() {
        return eprintln!("skipped: no PHP 8.1+");
    }
    let body = r#"<?php
require __DIR__ . '/vendor/fake.php';
$events = new Fake\Events();
new Fake\App($events);
env('X');
register_shutdown_function(function () use ($events) {
    $events->query('select * from "errors"', [], 1.0);
});
"#;
    let (request, _) = one_request(body).await;
    assert_eq!(request.queries.last().map(|q| q.sql.as_str()), Some("select * from \"errors\""));
}

/// Out of memory: Stoke sends what it can without its queries and never adds a fatal of its own.
#[tokio::test(flavor = "multi_thread")]
async fn out_of_memory_sends_without_queries_and_no_second_fatal() {
    if php().is_none() {
        return eprintln!("skipped: no PHP 8.1+");
    }
    let body = r#"<?php
require __DIR__ . '/vendor/fake.php';
ini_set('memory_limit', '32M');
$events = new Fake\Events();
new Fake\App($events);
env('X');
for ($i = 0; $i < 400; $i++) {
    $events->query('select * from "items" where "id" = ?', [$i], 0.1);
}
$x = str_repeat('x', 64 * 1024 * 1024);
"#;
    let (request, said) = one_request(body).await;
    assert!(said.contains("Allowed memory size"), "{said}");
    assert!(!said.contains("dump.php"), "a second fatal blames Stoke: {said}");
    assert!(request.queries.is_empty());
    assert_eq!(request.dropped, 400);
}

#[test]
fn without_artisan_nothing_is_defined() {
    let php = need_php!();
    let (d, paths) = home();
    write_prepend(&paths, &on(free_port())).unwrap();
    let out = run(&php, d.path(), &paths, "<?php\necho function_exists('env') ? 'env' : 'none';\n");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "none");
}

#[test]
fn a_long_running_command_is_not_recorded() {
    let php = need_php!();
    let (d, paths) = home();
    write_prepend(&paths, &on(free_port())).unwrap();
    let body = "<?php\necho function_exists('env') ? 'env' : 'none';\n";
    for args in [&["queue:work"][..], &["octane:start", "--watch"], &["serve"]] {
        let out = run_laravel(&php, d.path(), &paths, body, args);
        assert_eq!(String::from_utf8_lossy(&out.stdout), "none", "{args:?}");
    }
    let out = run_laravel(&php, d.path(), &paths, body, &["migrate"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "env");
}

#[test]
fn dumps_off_requests_on_has_no_dump_function() {
    let php = need_php!();
    let (d, paths) = home();
    write_prepend(&paths, &CollectorConfig { port: free_port(), dumps: false, requests: true }).unwrap();
    let body =
        "<?php\necho function_exists('dump') ? 'dump' : 'no dump', ' ', function_exists('env') ? 'env' : 'none';\n";
    let out = run_laravel(&php, d.path(), &paths, body, &["migrate"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "no dump env");
    write_prepend(&paths, &CollectorConfig { port: free_port(), dumps: true, requests: false }).unwrap();
    let out = run_laravel(&php, d.path(), &paths, body, &["migrate"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "dump none");
}

fn home() -> (tempfile::TempDir, StokePaths) {
    let d = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(d.path().join("stoke"));
    paths.ensure().unwrap();
    (d, paths)
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A PHP 8.1+ to run the script with: `STOKE_TEST_PHP`, else `php` on PATH.
fn php() -> Option<PathBuf> {
    let candidate = std::env::var_os("STOKE_TEST_PHP").map(PathBuf::from).unwrap_or_else(|| "php".into());
    let out = Command::new(&candidate).args(["-n", "-r", "echo PHP_VERSION_ID;"]).output().ok()?;
    let id: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    (id >= 80100).then_some(candidate)
}

/// Runs `body` as `project/s.php` with only the prepend script loaded (`-n`: no php.ini).
fn run(php: &Path, d: &Path, paths: &StokePaths, body: &str) -> Output {
    let project = d.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("s.php"), body).unwrap();
    // Quoted as in php.ini: unquoted, a `~` in a short Windows path (RUNNER~1) is an ini operator.
    let prepend = format!("auto_prepend_file=\"{}\"", script_path(paths).display());
    Command::new(php).args(["-n", "-d", &prepend, "s.php"]).current_dir(&project).output().unwrap()
}

async fn wait_for(c: &Collector, n: usize) {
    for _ in 0..200 {
        if c.dumps().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("only {} dumps arrived", c.dumps().len());
}

/// Stands in for Xdebug: its two functions, then the real script.
const FAKE_XDEBUG: &str = r#"<?php
function xdebug_is_debugger_active(): bool { return getenv('ACTIVE') === '1'; }
function xdebug_connect_to_client(): bool { echo "connect "; return true; }
require getenv('PREPEND');
"#;

/// A site's trigger arrives as a FastCGI param, in `$_SERVER` only; Xdebug looks in `$_ENV` (empty
/// under GPCS), so the script starts the session. One that already runs is left alone.
#[test]
fn the_trigger_starts_an_xdebug_session() {
    let php = need_php!();
    let (d, paths) = home();
    write_prepend(&paths, &CollectorConfig { port: free_port(), dumps: false, requests: false }).unwrap();
    let stub = d.path().join("stub.php");
    std::fs::write(&stub, FAKE_XDEBUG).unwrap();
    let project = d.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("s.php"), "<?php echo 'app';").unwrap();
    let go = |trigger: bool, active: bool| {
        let mut cmd = Command::new(&php);
        cmd.args(["-n", "-d", "variables_order=GPCS", "-d", &format!("auto_prepend_file=\"{}\"", stub.display())]);
        cmd.arg("s.php").current_dir(&project).env("PREPEND", script_path(&paths));
        cmd.env_remove("XDEBUG_TRIGGER");
        if trigger {
            cmd.env("XDEBUG_TRIGGER", "stoke");
        }
        cmd.env("ACTIVE", if active { "1" } else { "0" });
        let out = cmd.output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
    };
    assert_eq!(go(true, false), "connect app");
    assert_eq!(go(false, false), "app");
    assert_eq!(go(true, true), "app");
}

#[test]
fn the_script_has_the_port_and_is_rewritten_only_on_change() {
    let (_d, paths) = home();
    let path = write_prepend(&paths, &on(2400)).unwrap();
    assert_eq!(path, script_path(&paths));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("tcp://127.0.0.1:2400") && !text.contains("__STOKE_PORT__"));
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(30));
    write_prepend(&paths, &on(2400)).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before, "same content, no write");
    write_prepend(&paths, &on(2401)).unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("tcp://127.0.0.1:2401"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_script_dump_reaches_the_collector_and_still_prints() {
    let php = need_php!();
    let (d, paths) = home();
    let (collector, port) = (Collector::default(), free_port());
    collector.start(port).await.unwrap();
    write_prepend(&paths, &on(port)).unwrap();
    let (p, dir) = (paths.clone(), d.path().to_path_buf());
    let body = "<?php\n$x = dump(['a' => 1]);\necho 'after ', $x['a'];\n";
    let out = tokio::task::spawn_blocking(move || run(&php, &dir, &p, body)).await.unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("[a] => 1") && stdout.ends_with("after 1"), "{stdout}");
    wait_for(&collector, 1).await;
    let dump = &collector.dumps()[0];
    assert_eq!((dump.site.as_deref(), dump.line, dump.sapi.as_str()), (Some("project"), Some(2), "cli"));
    assert!(dump.file.as_deref().unwrap().ends_with("s.php"), "{:?}", dump.file);
    assert!(dump.text.contains("[a] => 1"), "{}", dump.text);
    collector.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn dd_exits_with_1_after_sending() {
    let php = need_php!();
    let (d, paths) = home();
    let (collector, port) = (Collector::default(), free_port());
    collector.start(port).await.unwrap();
    write_prepend(&paths, &on(port)).unwrap();
    let (p, dir) = (paths.clone(), d.path().to_path_buf());
    let body = "<?php\ndd('stop', 2);\necho 'never';\n";
    let out = tokio::task::spawn_blocking(move || run(&php, &dir, &p, body)).await.unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("stop") && !stdout.contains("never"), "{stdout}");
    wait_for(&collector, 1).await;
    assert_eq!(collector.dumps()[0].text, "stop\n2");
    collector.stop();
}

/// Windows waits out the whole connect timeout on a closed loopback port: only the first dump of a
/// run may pay it.
#[test]
fn a_closed_port_costs_nothing() {
    let php = need_php!();
    let (d, paths) = home();
    write_prepend(&paths, &on(free_port())).unwrap();
    let started = Instant::now();
    let out = run(&php, d.path(), &paths, "<?php\nfor ($i = 0; $i < 50; $i++) { dump($i); }\necho 'done';\n");
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
    assert!(String::from_utf8_lossy(&out.stdout).ends_with("done"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_huge_dump_arrives_cut() {
    let php = need_php!();
    let (d, paths) = home();
    let (collector, port) = (Collector::default(), free_port());
    collector.start(port).await.unwrap();
    write_prepend(&paths, &on(port)).unwrap();
    let (p, dir) = (paths.clone(), d.path().to_path_buf());
    let body = r"<?php
dump(str_repeat('C:\\path\\x.php ', 30000));
";
    tokio::task::spawn_blocking(move || run(&php, &dir, &p, body)).await.unwrap();
    wait_for(&collector, 1).await;
    assert!(collector.dumps()[0].truncated);
    collector.stop();
}

/// Symfony's source link takes the first frame without a class after `VarDumper::dump`; Laravel
/// looks for the frame in `symfony/var-dumper/Resources/functions/dump.php` and takes the next.
/// Both must land on the app's own line, as without Stoke.
#[test]
fn the_source_is_the_apps_line_for_symfony_and_laravel() {
    let php = need_php!();
    let (d, paths) = home();
    write_prepend(&paths, &on(free_port())).unwrap();
    let body = r#"<?php
namespace Symfony\Component\VarDumper;
class VarDumper {
    public static function dump($var, $label = null) {
        $trace = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS);
        for ($i = 1; $i < count($trace); $i++) {
            if (isset($trace[$i]['file']) && empty($trace[$i]['class'])) {
                echo 'symfony=', basename($trace[$i]['file']), ':', $trace[$i]['line'], "\n";
                break;
            }
        }
        foreach ($trace as $k => $frame) {
            if (isset($frame['file']) && str_ends_with(str_replace('\\', '/', $frame['file']), 'symfony/var-dumper/Resources/functions/dump.php')) {
                $source = $trace[$k + 1];
                echo 'laravel=', basename($source['file']), ':', $source['line'], "\n";
                break;
            }
        }
    }
}
namespace App;
dump(1);
"#;
    let out = run(&php, d.path(), &paths, body);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("symfony=s.php:22\n") && stdout.contains("laravel=s.php:22\n"), "{stdout}");
}
