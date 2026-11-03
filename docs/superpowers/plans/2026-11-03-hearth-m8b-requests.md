# M8b Requests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every request (and short artisan command) of a Laravel app run by Hearth shows up on a Requests page with its duration, status and SQL queries on a timeline, and Hearth points out N+1 queries, duplicate queries and slow queries with the line of app code that ran them.

**Architecture:** The M8a prepend script grows a Laravel part. In a folder with an `artisan` file (the script's folder or its parent), it defines Laravel's `env()`, `app()` and `config()` helpers first, with Laravel's exact bodies plus a cheap one-time hook. The first call that finds the Laravel container registers a `QueryExecuted` listener on `$app['events']` and a shutdown function. Each query keeps its SQL, bindings (shortened), time, start offset, connection and the first caller outside `vendor/`. At shutdown one `{"type":"request"}` JSON line goes to the collector. The core analyses it (N+1, duplicates, slow), keeps the newest 200 in memory, and `requests.*` IPC methods feed a Requests page.

**Tech Stack:** Rust (hearth-core), PHP 8.1+, Svelte 5 UI.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`, M8: "per-request timeline with N+1 / duplicate / slow query detection." Owner decision 2026-11-03: collect through the prepend script by defining Laravel's helpers first ("Script trick, no app change"), not a Composer package or a C extension. Other choices are the builder's.

## Global Constraints

- Never use em or en dashes, in code, comments, messages or docs.
- The Laravel part is only defined when an `artisan` file is next to the running script or one folder up (`public/index.php`, `php artisan ...`). Tests under `vendor/bin` are never hooked.
- `env()`, `app()` and `config()` keep Laravel's exact bodies (identical from Laravel 8 to 12.59). The hook never throws, and when it cannot find the container or the events dispatcher the app runs as before and nothing is sent.
- Long-running commands are never recorded: `serve`, `queue:work`, `queue:listen`, `horizon`, `schedule:work`, `octane:*`, `reverb:start`, `pail`, `tinker`, `dev`.
- At most 500 queries per request are kept (the rest are counted); each SQL is cut at 2000 characters and each binding at 100.
- Findings: N+1 = the same normalized SQL from the same caller line 5 or more times; duplicate = the same SQL with the same bindings 2 or more times; slow = 100 ms or more.
- `config/collector.json` gains `"requests": true` (missing means true). The prepend line is in php.ini when dumps or requests are on; `dump()`/`dd()` are only defined when dumps are on.
- The core keeps the newest 200 requests.

## Review Focus

1. A non-Laravel app (Symfony, WordPress, a plain script) with its own `env()` or `app()`: never touched, since no `artisan` is next to it.
2. A Laravel app with a cached config (`config:cache`), where `env()` is not called while booting: still recorded through `app()`/`config()` calls; queries before the first call are missed, nothing breaks.
3. A request that ends with an exception, a `dd()`, `exit` or a fatal error: the shutdown function still sends what it has, with the status code PHP reports.
4. Bindings with secrets, binary data or huge strings: shortened to 100 characters and sent as text; nothing invalid breaks the JSON.
5. A command that runs thousands of queries (a seeder): capped at 500 kept, the total counted, the line stays under the collector's limit.

---

### Task 1: requests in the core

**Files:**
- Create: `crates/hearth-core/src/insights/requests.rs`, `insights/requests_tests.rs`
- Modify: `crates/hearth-core/src/insights/collector.rs` (accept `"type":"request"`, keep 200), `insights/mod.rs`

**Interfaces:**
- Produces: `Query { sql, bindings: Vec<String>, ms: f64, start_ms: f64, connection, file: Option<String>, line: Option<u32> }`; `Finding { kind: "n+1" | "duplicate" | "slow", text: String, queries: Vec<usize> }`; `Request { id, time, site, sapi, method: Option<String>, uri: Option<String>, command: Option<String>, status: Option<u16>, ms: f64, memory: u64, queries: Vec<Query>, dropped: u32, findings: Vec<Finding> }`; `RequestSummary` (the same without `queries`, plus `query_count`, `query_ms`); `pub fn normalize(sql) -> String` (numbers and quoted strings to `?`, `(?, ?, ?)` to `(?)`, whitespace collapsed, lowercase keywords not needed); `pub fn analyse(&[Query]) -> Vec<Finding>`; `Collector::requests_after(id) -> Vec<RequestSummary>`, `Collector::request(id) -> Option<Request>`, `Collector::clear_requests()`.

- [ ] **Step 1: failing tests:** `normalize_folds_lists_and_literals`; `n_plus_one_needs_five_from_one_line`; `duplicates_need_equal_bindings`; `slow_is_100_ms_or_more`; `a_request_line_is_kept_with_findings` (through the collector socket); `only_the_newest_200_requests_are_kept`.
- [ ] **Step 2:** `cargo test -p hearth-core insights` fails.
- [ ] **Step 3:** implement.
- [ ] **Step 4:** tests pass, clippy clean.
- [ ] **Step 5:** commit "Keep requests and find query problems".

### Task 2: the Laravel part of the prepend script

**Files:**
- Modify: `crates/hearth-core/src/insights/prepend.php`, `insights/prepend.rs` (placeholders `__HEARTH_DUMPS__` and `__HEARTH_REQUESTS__`), `insights/collector.rs` (`CollectorConfig.requests`, serde default true), `insights/prepend_tests.rs`

- [ ] **Step 1: failing tests** (real PHP, a fake Laravel in the test script: `Illuminate\Container\Container` with `getInstance()` and `offsetGet('events')`, a dispatcher with `listen()`, `Illuminate\Database\Events\QueryExecuted`, `Illuminate\Support\Env`): `a_laravel_request_is_sent_with_its_queries_and_callers` (an `artisan` file next to the script; `env('X')` hooks, three fake queries fired from two lines, the shutdown line arrives with 3 queries, their callers and `command`); `without_artisan_nothing_is_defined` (`function_exists('env')` stays false); `a_long_running_command_is_not_recorded` (`argv` `queue:work`); `dumps_off_requests_on_has_no_dump_function`; `the_config_keeps_requests_and_defaults_it_on`.
- [ ] **Step 2:** run with `HEARTH_TEST_PHP`, expect failures.
- [ ] **Step 3:** implement; `for_ini` emits the prepend when dumps or requests are on.
- [ ] **Step 4:** tests pass.
- [ ] **Step 5:** commit "Record Laravel requests and queries".

### Task 3: `requests.*` methods

**Files:**
- Create: `crates/hearth-core/src/insights/requests_api.rs` or extend `insights/api.rs`, with tests in `insights/api_tests.rs`

**Interfaces:**
- Produces: `requests.list {after?}` -> `RequestSummary[]` newest first; `requests.get {id}` -> `Request`; `requests.clear`; `requests.enable {on}` (saves, rewrites php.ini of every PHP, syncs); `dumps.status` gains `requests: bool`.

- [ ] **Step 1: failing tests:** `list_get_and_clear_requests`; `requests_off_and_dumps_off_removes_the_prepend_line`.
- [ ] **Step 2-4:** implement, tests pass.
- [ ] **Step 5:** commit "Request methods in the core".

### Task 4: the Requests page

**Files:**
- Create: `ui/src/requests.svelte.ts`, `ui/src/requests.spec.ts`, `ui/src/views/Requests.svelte`
- Modify: `ui/src/api.ts` (types, calls, demo data with an N+1 request), `ui/src/App.svelte` (nav "Requests" after Dumps)

Layout (builder's call): the list of requests (method, URI or command, status, duration, query count and time, a badge per finding kind, site, time) with a site filter, Clear and the On/Off switch; the open request shows a timeline (the request as a bar, each query as a mark at its start offset and width, findings colored), the findings with their caller, and the query list (SQL, bindings, time, caller), with N+1 groups folded. Polls `requests.list {after}` every second.

- [ ] **Step 1: failing tests:** filter and Clear; poll asks only for newer; opening loads the full request; the timeline maps start and duration to percentages.
- [ ] **Step 2-4:** implement; `npx vitest run`, `npm run check`; screenshots light and dark at 1100 and 760 (`?view=requests&request=<id>`).
- [ ] **Step 5:** commit "Requests page".

### Task 5: hands-on check on Windows

- [ ] A fresh Laravel app (`composer create-project laravel/laravel` in the scratchpad, SQLite), a route with an N+1 loop and a slow query (`sleep` via SQLite is not possible: use a big cross join or `usleep` around a query), run through Hearth's PHP with `php artisan serve` and one artisan command; check the page shows both with the right findings and callers. Stop everything by PID, delete the test home and the app.
