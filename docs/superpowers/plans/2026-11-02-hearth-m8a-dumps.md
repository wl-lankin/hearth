# M8a Dumps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every `dump()` and `dd()` in a PHP app run by Hearth (web or CLI) also shows up on a Dumps page in Hearth, with where it was called, for which site, and when.

**Architecture:** Hearth's generated `php.ini` gets `auto_prepend_file` pointing at `<data>/collector/prepend.php`, a script Hearth writes. It defines `dump()` and `dd()` before Composer's autoload runs; Symfony's VarDumper and Laravel only define theirs when none exists, so Hearth's wins. Hearth's `dump()` renders each value as plain text (VarDumper's `CliDumper` when the app has it, `print_r` otherwise), sends one JSON line to the core's collector on `127.0.0.1:<port>`, then dumps the normal way (`VarDumper::dump`), so pages and terminals look as before. The core keeps the newest 500 entries in memory; `dumps.*` IPC methods feed a Dumps page.

**Tech Stack:** Rust (hearth-core, src-tauri), PHP 8.1+ for the prepend script, Svelte 5 UI.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`, M8: "Collector extension, dump viewer, Laravel log viewer, per-request timeline with N+1 / duplicate / slow query detection." M8 ships as three PRs: M8a dumps (this plan, which also builds the collector), M8b the request timeline, M8c the log viewer. The owner said "merge and continue with M8" on 2026-11-02 without design questions; choices below are the builder's.

## Global Constraints

- Never use em or en dashes, in code, comments, messages or docs.
- The collector listens on `127.0.0.1` only. Default port 2306, stored in `config/collector.json` as `{"port": 2306, "dumps": true}`.
- The prepend script never breaks an app: everything is guarded (`function_exists`, `class_exists`, try/catch), it sends with a 50 ms connect timeout and gives up silently, and with collection off there is no `auto_prepend_file` line at all.
- A dump still prints where it printed before (page or terminal); `dd()` still ends the script with status 1 and, outside the CLI, a 500 header, as Symfony's does.
- One dump entry holds at most 256 KB of text (longer is cut with a note); the core keeps the newest 500.
- Tests never bind 2306: they use port 0. The PHP test runs when `HEARTH_TEST_PHP` or `php` on PATH gives a PHP 8.1+; otherwise it prints that it was skipped.

## Review Focus

1. An app without `symfony/var-dumper` (a plain script): `dump()` still works, captured with `print_r`, and printed as before.
2. Hearth not running (port closed): a `dump()` costs no noticeable time and prints normally.
3. A dump of something huge or recursive (a model with relations, `$GLOBALS`): the PHP side stays bounded (VarDumper's cloner limits), and the entry is cut at 256 KB.
4. A user `auto_prepend_file` in `user.ini`: theirs wins (user.ini is appended later) and Hearth's Dumps page simply stays empty; nothing breaks.
5. Garbage or oversized lines sent to the collector port (a browser, a port scanner): ignored, the collector keeps serving.

---

### Task 1: the collector in the core

**Files:**
- Create: `crates/hearth-core/src/insights/mod.rs`, `insights/collector.rs`, `insights/collector_tests.rs`
- Modify: `crates/hearth-core/src/lib.rs`, `crates/hearth-core/src/ipc/mod.rs` (field `collector: Arc<insights::collector::Collector>`)

**Interfaces:**
- Produces: `pub struct Dump { id: u64, time: u64 (unix ms), site: Option<String>, file: Option<String>, line: Option<u32>, sapi: String, uri: Option<String>, text: String, truncated: bool }` (serde camelCase); `Collector` with `start(&self, port) -> Result<(), CoreError>` / `stop()` / `status() -> CollectorStatus { port, listening, error }` (same semantics as the mail catcher: a taken port keeps the old one), `dumps() -> Vec<Dump>` (newest first), `clear()`; `pub fn config(paths) -> CollectorConfig { port, dumps }`, `pub fn set_config(paths, &CollectorConfig)`.
- Wire format: one JSON object per line, `{"type":"dump","site":..,"file":..,"line":..,"sapi":..,"uri":..,"text":..}`; unknown `type`, bad JSON or a line over 300 KB is dropped. A connection may send many lines. An HTTP request line closes the connection.

- [ ] **Step 1: failing tests:** `a_dump_line_is_kept_newest_first`; `many_lines_on_one_connection_and_garbage_between`; `text_over_256_kb_is_cut_and_marked`; `only_the_newest_500_are_kept`; `an_http_request_is_dropped`; `a_taken_port_keeps_the_old_one`; `config_defaults_and_roundtrip`.
- [ ] **Step 2:** `cargo test -p hearth-core insights` fails.
- [ ] **Step 3:** implement (tokio listener, `read_until` with a byte cap, `Mutex<VecDeque<Dump>>`, ids from an `AtomicU64`).
- [ ] **Step 4:** tests pass, clippy clean.
- [ ] **Step 5:** commit "Collect dumps in the core".

### Task 2: the prepend script and php.ini

**Files:**
- Create: `crates/hearth-core/src/insights/prepend.php` (embedded with `include_str!`), `insights/prepend.rs`, `insights/prepend_tests.rs`
- Modify: `crates/hearth-core/src/php/ini.rs` (`IniContext.prepend: Option<PathBuf>`, rendered as `auto_prepend_file = "<path>"`), `crates/hearth-core/src/php/service.rs` (`generated_ini` fills it from the collector config), `crates/hearth-core/src/php/ini.rs` tests

**Interfaces:**
- Produces: `pub fn write_prepend(paths, port) -> Result<PathBuf, CoreError>` (writes `<data>/collector/prepend.php` with the port filled in, only when the content changed); `pub fn apply_all(paths) -> Result<(), CoreError>` (rewrites the prepend and every installed version's php.ini).
- The script: `if (!function_exists('dump')) { function dump(mixed ...$vars): mixed {...} }` and the same for `dd`; a private `__hearth_send(array $entry)` with `stream_socket_client("tcp://127.0.0.1:PORT", ..., 0.05)`; the caller's file and line from `debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 2)`; `site` from `$_SERVER['HTTP_HOST']` without the port, else the working folder's name; `uri` from `$_SERVER['REQUEST_URI']`.

- [ ] **Step 1: failing tests:** ini: `the_prepend_line_is_written_when_dumps_are_on` and absent when off; prepend: `the_script_has_the_port_and_is_rewritten_only_on_change`; PHP (real PHP, see Global Constraints): `a_plain_script_dump_reaches_the_collector_and_still_prints` (runs `php -d auto_prepend_file=<script> -r 'dump(["a" => 1]); echo "after";'` against a collector on a free port: stdout has the print_r output and `after`, the collector has one dump with `site`, `line` 1 and the text), `dd_exits_with_1_after_sending`, `a_closed_port_costs_nothing` (no collector: the script still prints, within 1 s).
- [ ] **Step 2:** run, expect failures.
- [ ] **Step 3:** implement; a dumps switch change calls `apply_all`, so the pools restart with the new php.ini on the next sync.
- [ ] **Step 4:** `cargo test -p hearth-core insights php::ini` passes.
- [ ] **Step 5:** commit "Send dumps from PHP to Hearth".

### Task 3: `dumps.*` methods and the app

**Files:**
- Create: `crates/hearth-core/src/insights/api.rs`, `insights/api_tests.rs`
- Modify: `crates/hearth-core/src/ipc/mod.rs` (route), `src-tauri/src/lib.rs` (start the collector in `start_core`, stop it in `stop_core`, write the prepend on start)

**Interfaces:**
- Produces: `dumps.list` -> `Dump[]`; `dumps.clear`; `dumps.status` -> `{port, listening, error, enabled}`; `dumps.enable {on}` (saves, rewrites the ini files); `dumps.port {port}` (moves the collector, rewrites the prepend; a taken port is an error and keeps the old one).

- [ ] **Step 1: failing tests:** `list_and_clear_through_the_core`; `enable_off_removes_the_prepend_line_from_every_installed_php` (a fake installed PHP 8.4 as in the PHP service tests); `port_change_rewrites_the_prepend`.
- [ ] **Step 2:** run, expect failures.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** `cargo test -p hearth-core insights`, clippy clean.
- [ ] **Step 5:** commit "Dump methods in the core and the app".

### Task 4: the Dumps page

**Files:**
- Create: `ui/src/dumps.svelte.ts`, `ui/src/dumps.spec.ts`, `ui/src/views/Dumps.svelte`
- Modify: `ui/src/api.ts` (types, calls, demo data), `ui/src/App.svelte` (nav "Dumps" after Mail)

Layout (builder's call): a top strip with the collector state, the on/off switch and the port; a site filter and Clear; then the dumps, newest first, each a card with the site, `file:line` (path shortened to the part after the site folder when it can), the time and the request URI or "CLI", and the text in a monospace block, long ones folded after 30 lines with "Show all". Polls `dumps.list` every second while open.

- [ ] **Step 1: failing tests** in `dumps.spec.ts`: sites filter; clear; switch off and on calls `dumps.enable`; a port error shows; folding of long text.
- [ ] **Step 2:** `npx vitest run src/dumps.spec.ts` fails.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** `npx vitest run`, `npm run check`; screenshots light and dark at 1100 and 760 (`?view=dumps`).
- [ ] **Step 5:** commit "Dumps page".

### Task 5: hands-on check on Windows

- [ ] With the app on a test home (collector port 2307), a PHP version installed from the catalog and a small Composer project with `symfony/var-dumper` in the scratchpad: `php` through Hearth's shim runs a script with `dump()` and `dd()`; check the page shows them with file and line and the terminal output is unchanged; switch dumps off and check php.ini has no prepend line. Stop the app by its PID and delete the test home.
