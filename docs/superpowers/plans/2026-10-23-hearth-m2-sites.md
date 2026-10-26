# Hearth M2 (Sites) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve local PHP sites at `http://<name>.test:<port>` from parked folders, linked folders and proxies, each with its own PHP version, through a Caddy server and PHP FastCGI pools that Hearth runs and supervises, managed from the app and the CLI.

**Architecture:** The signed catalog (hearth-php) gains a `tools.caddy` list. Hearth core gets a `sites` module (registry in `config/sites.json`, docroot detection, Caddyfile rendering) and a `supervisor` module that runs Caddy and one FastCGI pool per PHP version in use (Windows: several `php-cgi.exe -b`; macOS: `php-fpm` with a generated pool config), restarts them on crash and stops them with Hearth. A `web` service ties both together: any site change re-renders the Caddyfile, starts or stops pools and restarts Caddy. DNS and HTTPS stay for M3: names resolve through existing hosts entries (Herd's today) or `curl --resolve`.

**Tech Stack:** Existing M1 stack. Caddy 2.11 (official builds). Windows Job Objects (`windows-sys` `Win32_System_JobObjects`) so children die with Hearth. tauri-plugin-dialog for folder pickers.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (milestone M2, decisions 5 and 6).

## Global Constraints

- Everything from M0 and M1 Global Constraints applies (public repos, no machine/company details, no long dashes, `M2: <description>` commits; Hearth branch `HEARTH-4` cut from `HEARTH-2`, hearth-php branch `HEARTH-5`; push only on the owner's go).
- Side by side with Herd: Hearth never binds 80/443 in M2. Default HTTP port `8080` (setting `sites.httpPort`); if taken, the Sites view says so and offers the next free port. FastCGI ports `19000 + major*100 + minor*10 + i` (8.4 -> 19840..19843), never Herd's 90xx/91xx.
- Caddy's admin API is off (`admin off`): config changes restart Caddy (local, ~100 ms). No other listening sockets besides the HTTP port and the FastCGI ports, all on 127.0.0.1.
- Site names: lowercase `[a-z0-9-]` labels joined by dots (`api.shop`), max 63 per label; the URL is `<name>.test`. A linked name wins over a parked folder of the same name.
- Child processes run without a console window (Windows `CREATE_NO_WINDOW`), with `PHPRC` set, and are killed when Hearth exits (Job Object on Windows; process group + kill on macOS).
- Generated Caddyfile lives at `<config>/caddy/Caddyfile`; logs at `<logs>/caddy.log`, `<logs>/php-<minor>.log`.

## Review Focus

1. **A pool or Caddy crashes or its port is taken**: restart with backoff (1 s, 2 s, 4 s, max 30 s), the Sites view shows the error ("Port 8080 is used by another program"), never a silent dead site. Tests in Tasks 3 and 7.
2. **Folders with spaces, non-ASCII, or a `{` / `}` / `"`**: Caddyfile quoting must hold (Caddy treats braces specially). Test in Task 5.
3. **Removing the PHP version a site uses**: the site falls back to the default version and the view shows "uses the default PHP 8.4 (8.3 was removed)". Test in Task 6.
4. **A parked folder that disappears or a linked folder that is deleted**: the site is listed as "Folder missing", no Caddy error for the whole server. Test in Task 4.
5. **Hearth quits or crashes**: no orphan `php-cgi.exe` / `caddy.exe` keeps the ports. Test in Task 3 (Windows job object) and the end-to-end check.

---

## Part A: hearth-php

### Task 1: Caddy in the catalog

**Files:** `lib/manifest.mjs` (validate optional `tools.caddy[]`: `os`, `arch`, `version`, `url`, `sha256`, `format`, `binary` = path inside the archive), `scripts/caddy.mjs`, `scripts/generate.mjs`, `test/caddy.test.mjs`.

**Interfaces:** `caddyEntries({ release, checksums, hash })`: from the GitHub release JSON and `caddy_<v>_checksums.txt` (SHA-512 lines `<hash>  <file>`), entries for windows/x64 (`windows_amd64.zip`, binary `caddy.exe`), macos/arm64 (`mac_arm64.tar.gz`), macos/x86_64 (`mac_amd64.tar.gz`, binary `caddy`). `hash(url)` downloads, checks the SHA-512 against the checksums file (mismatch throws) and returns the SHA-256 for the catalog.

- [ ] **Step 1:** Failing tests: three entries with the right file names; a SHA-512 mismatch throws `caddy_2.11.7_mac_arm64.tar.gz: checksum mismatch`; `validate` accepts a manifest with `tools.caddy` and reports a missing `sha256`.
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M2: Caddy in the catalog`.

## Part B: Hearth

### Task 2: Caddy from the catalog (core)

**Files:** `crates/hearth-core/src/php/manifest.rs` (`Tool { os, arch, version, url, sha256, format, binary }`, `Manifest.tools: Tools { caddy: Vec<Tool> }` with `#[serde(default)]`), `crates/hearth-core/src/web/caddy_install.rs`.

**Interfaces:** `ensure_caddy(core) -> Result<PathBuf, CoreError>`: picks the build for this OS/arch, downloads + verifies like PHP (Task 6/9 code of M1: `download`, `unpack_with`), installs to `<data>/caddy/<version>/`, returns the binary path; reuses an installed matching version.

- [ ] Failing tests against the M1 test server: installs once and returns the path; second call does not download again (server hit count); a catalog without `tools.caddy` gives "This catalog has no web server for this computer".
- [ ] Implement, GREEN, commit `M2: Install Caddy from the catalog`.

### Task 3: Process supervisor (core)

**Files:** `crates/hearth-core/src/supervisor.rs` (+ `supervisor/windows.rs` Job Object).

**Interfaces:**
- `pub struct Supervisor` (in `CoreState`, `Arc`): `ensure(spec: ProcessSpec) -> Result<(), CoreError>` starts or keeps a process by `spec.key`; `stop(key)`; `stop_all()`; `status() -> Vec<ProcessStatus { key, running, pid, restarts, last_error }>`.
- `ProcessSpec { key: String, program: PathBuf, args: Vec<OsString>, env: Vec<(String, OsString)>, cwd: Option<PathBuf>, log: PathBuf, ready_port: Option<u16> }`. Start waits up to 5 s for `ready_port` to accept connections; a port already in use before start fails with "Port <n> is used by another program".
- Restarts a crashed process with backoff 1, 2, 4, 8, 16, 30 s; resets the backoff after 60 s of uptime. Spec changes (`ensure` with the same key, different args) restart it.
- Windows: every child is assigned to one Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (owned by the supervisor, so children die with Hearth), started with `CREATE_NO_WINDOW`. macOS: own process group, killed on `stop_all` and on drop.

- [ ] Failing tests using a tiny test helper binary (`crates/hearth-core/tests/bin/listen.rs` built as an example: listens on a port, exits on a flag file): starts and reports running; port already taken fails with the message; a crash restarts it (restarts = 1); `stop_all` leaves no process; `#[cfg(windows)]` dropping the supervisor kills the child (job object).
- [ ] Implement, GREEN, commit `M2: Supervise child processes`.

### Task 4: Sites registry and detection (core)

**Files:** `crates/hearth-core/src/sites/{mod.rs, registry.rs, detect.rs}`.

**Interfaces:**
- `config/sites.json`: `{ "parked": ["C:\\Projects"], "links": { "shop": "C:\\x\\shop" }, "proxies": { "app.shop": "http://localhost:5173" }, "isolated": { "shop": "8.3" }, "httpPort": 8080 }` (atomic writes, corrupt file kept aside, one writer via `core.php_lock`'s sibling `sites_lock`).
- `pub fn list(paths) -> Vec<Site { name, kind: Parked|Linked|Proxy, path: Option<PathBuf>, upstream: Option<String>, docroot: Option<PathBuf>, php: Option<String>, problem: Option<String> }>`: parked = every subfolder of every parked path (name = folder name, lowercased, invalid characters replaced by `-`); links override parked; proxies have no PHP.
- `detect::docroot(path) -> Option<PathBuf>`: `public/index.php` (Laravel and most frameworks) -> `public/`; `index.php` -> root; `index.html` -> root (static); else None ("No index.php or index.html found").
- `park(dir)`, `unpark(dir)`, `link(name, dir)`, `unlink(name)`, `proxy(name, upstream)` (`http(s)://host:port` only), `unproxy(name)`, `isolate(name, minor|None)`, `set_http_port(port)`. Name validation per Global Constraints ("api.shop" ok, "Shop!" -> "Use lowercase letters, digits and dashes").

- [ ] Failing tests: parked folder with three subfolders lists three sites with detected docroots; a link overrides a parked name; a deleted link folder lists `problem: "Folder missing"`; invalid names refused with the message; proxy upstream `ftp://x` refused; isolation persists and is dropped by `unlink`.
- [ ] Implement, GREEN, commit `M2: Sites registry and docroot detection`.

### Task 5: Caddyfile rendering (core, pure)

**Files:** `crates/hearth-core/src/sites/caddyfile.rs`.

**Interfaces:** `render(ctx: CaddyContext) -> String` with `CaddyContext { http_port, sites: Vec<Site>, php_upstreams: BTreeMap<minor, Vec<u16>>, default_php: Option<String>, log: PathBuf }`. Global block: `{ admin off\n auto_https off\n http_port <p>\n log { output file "<log>" } }`. PHP site: `http://<name>.test:<p> { root * "<docroot>"\n php_fastcgi 127.0.0.1:<port> 127.0.0.1:<port> { lb_policy least_conn }\n file_server\n encode gzip }`. Proxy: `http://<name>.test:<p> { reverse_proxy <upstream> }`. Sites with `problem` or without PHP for their version are left out with a `# <name>: <problem>` comment. All paths double-quoted with `\` and `"` escaped; braces inside quotes are safe.

- [ ] Failing tests: a Laravel site block exactly as above; a proxy block; a docroot `C:\Projects\Jörg {old} "x"` renders a quoted, escaped path; a site isolated to 8.3 uses the 8.3 ports; a site without an installed PHP is a comment, not a block.
- [ ] Implement, GREEN, commit `M2: Render the Caddyfile`.

### Task 6: FastCGI pools (core)

**Files:** `crates/hearth-core/src/web/pools.rs`.

**Interfaces:** `pool_ports(minor) -> Vec<u16>` (Global Constraints formula, 4 per minor on Windows, 1 on macOS); `pool_specs(paths, minor) -> Vec<ProcessSpec>`: Windows `php-cgi.exe -b 127.0.0.1:<port> -c <ini>` with env `PHPRC`, `PHP_FCGI_MAX_REQUESTS=500`; macOS `sbin/php-fpm -F -y <config>/php/<minor>/fpm.conf` with a rendered pool conf (`listen = 127.0.0.1:<port>`, `pm = dynamic`, `pm.max_children = 8`, `catch_workers_output = yes`, `php_admin_value[error_log]`). `effective_php(site, state) -> Option<minor>`: isolation if installed, else default; reports "uses the default PHP 8.4 (8.3 was removed)".

- [ ] Failing tests: ports for 8.4 are 19840..19843 on Windows; the fpm conf for 8.4; a site isolated to a removed 8.3 falls back with that note.
- [ ] Implement, GREEN, commit `M2: PHP FastCGI pools`.

### Task 7: Web service (core)

**Files:** `crates/hearth-core/src/web/mod.rs`.

**Interfaces:** `pub async fn sync(core) -> Result<WebStatus, CoreError>`: lists sites, resolves PHP per site, `ensure`s pools for used minors and stops unused ones, ensures Caddy, renders the Caddyfile, validates it with `caddy validate --config` (a failure keeps the last good config running and reports the error), writes it and restarts Caddy only when it changed. Called at core start, after every site change, PHP install/uninstall/use, and from a 30 s health loop. `WebStatus { http_port, running, processes: Vec<ProcessStatus>, error: Option<String> }`.

- [ ] Failing tests with a fake Caddy (the Task 3 helper accepting `validate`/`run`): first sync installs nothing new when nothing is parked; parking a folder with one Laravel-shaped site starts one pool and Caddy; unparking stops the pool; an invalid config keeps the old one and reports the error.
- [ ] Implement, GREEN, commit `M2: Keep the web server in step with the sites`.

### Task 8: IPC and CLI

**Interfaces:** IPC `sites.list`, `sites.park {dir}`, `sites.unpark {dir}`, `sites.link {name, dir}`, `sites.unlink {name}`, `sites.proxy {name, upstream}`, `sites.unproxy {name}`, `sites.isolate {name, minor|null}`, `sites.port {port}`, `web.status`. CLI (Herd-shaped): `hearth park [dir]`, `hearth unpark [dir]`, `hearth link [name]` (current folder), `hearth unlink [name]`, `hearth links` / `hearth sites [--json]`, `hearth proxy <name> <upstream>`, `hearth unproxy <name>`, `hearth isolate <minor> [--site name]`, `hearth unisolate [--site name]`, `hearth open [name]` (opens the URL in the browser).

- [ ] Failing tests: router happy paths and `-32602`s; CLI `link` from a temp cwd uses the folder name; `sites --json` against a served core.
- [ ] Implement, GREEN, commit `M2: Sites commands in the CLI`.

### Task 9: Sites view

**Files:** `ui/src/sites.svelte.ts` (+ spec), `ui/src/views/Sites.svelte`, nav item "Sites" (between Dashboard and PHP), `src-tauri` dialog permission for folder picking.

**View:** header with the web server state (Running on port 8080 / error text with "Use port 8081"), buttons "Park a folder", "Link a folder", "Add a proxy". Table: name (`shop.test`, link opens in the browser), kind chip (Parked / Linked / Proxy), folder or upstream, PHP select (Default (8.4) / installed minors), problem text; row menu: Open folder, Unlink / Remove proxy. Parked folders listed under the table with "Stop parking". Filled-tile selection only (owner taste).

- [ ] Failing store tests: load merges sites + status; isolate rolls back on error; park adds the folder's sites after reload; port conflict shows the suggestion.
- [ ] Implement, GREEN, screenshots light/dark at 1100x720 and 760x520, commit `M2: Sites view`.

### Task 10: End-to-end check (owner go needed)

- [ ] With a test home and the real catalog: install PHP 8.4 and Caddy, link a fresh `laravel new` project, `curl -H "Host: <name>.test" http://127.0.0.1:8080` returns the Laravel page; `php artisan serve` from that project also works (M1 PHPRC fix); a proxy to a local Vite server works; quit Hearth and confirm no `php-cgi.exe` / `caddy.exe` remain.

## Done when

- A linked Laravel project answers on `http://<name>.test:8080` through Hearth on Windows (and in CI smoke on macOS), sites are managed from the UI and CLI, and nothing Hearth started survives Hearth.
