# M9a Xdebug per Site Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One click on a site's row turns step debugging on for that site only, and Hearth shows whether an IDE is listening for Xdebug (port 9003) and which program it is.

**Architecture:** `sites.json` gains a `debug` set of site names. For a site in it, the Caddyfile's `php_fastcgi` block passes `env XDEBUG_TRIGGER hearth`, so Xdebug (already set to `mode = debug,develop`, `start_with_request = trigger`) starts a session for that site's requests and no other site's. Switching a site on also switches the Xdebug extension on for the site's PHP version (isolated or default). A new `debug::ide` module finds the program listening on a TCP port from the OS's listener table (Windows `GetExtendedTcpTable`, macOS `lsof`), never by connecting to it (an IDE would take a connection for a debug session).

**Tech Stack:** Rust (hearth-core), Svelte 5 UI.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`, M9: "One-click Xdebug on/off per site, auto-detect a listening IDE, SPX profiler view." Owner decision 2026-11-03: split M9; M9a is Xdebug per site and IDE detection, SPX follows as M9b. Checked before planning: Xdebug 3.5.3 under `php-cgi` 8.5 starts a session when `XDEBUG_TRIGGER` arrives as a FastCGI parameter, and not without it.

## Global Constraints

- Never use em or en dashes, in code, comments, messages or docs.
- The IDE check never opens a connection to the port; it reads the OS listener table.
- Turning a site's debugging off does not switch the Xdebug extension off (another site or the browser extension may use it); the PHP page keeps that switch.
- A proxy site cannot be debugged (no PHP).
- Xdebug's port stays 9003, Xdebug's default.

## Review Focus

1. A site isolated to a PHP version without Xdebug in its build: a clear error, nothing half switched.
2. Two sites on one PHP version, one debugged: only that one gets the trigger.
3. An IDE listening on IPv6 only (`[::]:9003`) or on all addresses: still detected.
4. A listener whose process name cannot be read (another user's process): shown as listening, without a name.
5. A site removed (unlinked) while in the debug set: no stale trigger, no error.

---

### Task 1: the debug set and the Caddyfile

**Files:**
- Modify: `crates/hearth-core/src/sites/registry.rs` (`SitesConfig.debug: BTreeSet<String>`, `Site.debug: bool`, `pub fn set_debug(paths, name, on) -> Result<(), CoreError>`, refusing proxies and unknown names; unlink drops the name), `crates/hearth-core/src/sites/caddyfile.rs` (`Route::Php { .., xdebug: bool }`), `crates/hearth-core/src/web/service.rs` (fills it), tests in `sites/tests.rs` and `sites/caddyfile_tests.rs`

- [ ] **Step 1: failing tests:** `a_debugged_site_passes_the_xdebug_trigger` (two PHP routes on 8.4, one with `xdebug: true`: only its block holds `env XDEBUG_TRIGGER hearth`); `set_debug_marks_the_site_and_refuses_proxies_and_unknown_names`; `unlinking_drops_debugging`.
- [ ] **Step 2-4:** implement, `cargo test -p hearth-core sites` passes.
- [ ] **Step 5:** commit "Debug one site at a time".

### Task 2: finding a listening IDE

**Files:**
- Create: `crates/hearth-core/src/debug/mod.rs`, `debug/ide.rs`, `debug/ide_tests.rs`
- Modify: `crates/hearth-core/src/lib.rs`, `crates/hearth-core/Cargo.toml` (windows-sys features `Win32_NetworkManagement_IpHelper`, `Win32_Networking_WinSock`)

**Interfaces:**
- Produces: `pub struct Listener { pid: u32, program: Option<String> }`; `pub fn listener(port: u16) -> Option<Listener>` (IPv4 and IPv6 tables; program is the executable's file stem).

- [ ] **Step 1: failing tests:** `finds_this_test_listening` (a std listener on 127.0.0.1:0: pid is ours, program names this test binary); `finds_an_ipv6_listener` (`[::1]:0`, skipped when IPv6 is off); `nothing_listening_is_none`.
- [ ] **Step 2-4:** implement.
- [ ] **Step 5:** commit "Find the program listening on a port".

### Task 3: `sites.debug` and `debug.status`

**Files:**
- Modify: `crates/hearth-core/src/sites/api.rs` (`sites.debug {name, on}`: sets the flag; on: the site's PHP minor gets Xdebug switched on through `php::service::set_extension`, an error when the build has no Xdebug; then the web sync), `crates/hearth-core/src/debug/mod.rs` + routing for `debug.status` -> `{port: 9003, listening, program}`, tests in `sites/api_tests.rs` and a new `debug/api_tests.rs`

- [ ] **Step 1: failing tests:** `debugging_a_site_switches_xdebug_on_for_its_php` (fake installed 8.4 with Xdebug in its build: php.ini gets `zend_extension=xdebug`); `a_php_without_xdebug_is_a_clear_error`; `debug_status_reports_a_listener` (port taken by the test: listening with our program name; free: not listening).
- [ ] **Step 2-4:** implement.
- [ ] **Step 5:** commit "Debug methods".

### Task 4: the Sites page

**Files:**
- Modify: `ui/src/api.ts` (`Site.debug`, `sitesDebug`, `debugStatus`, demo), `ui/src/sites.svelte.ts`, `ui/src/sites.spec.ts`, `ui/src/views/Sites.svelte`

Layout (builder's call): a line above the site list: "PhpStorm is listening for Xdebug on port 9003" or "No IDE is listening on port 9003: start listening in your IDE", polled with the page's 2 s refresh; on each PHP site's row a "Debug" toggle next to the PHP picker (pressed when on).

- [ ] **Step 1: failing tests:** toggling calls `sites.debug` and shows the new state; an error shows on that row; the IDE line follows `debug.status`.
- [ ] **Step 2-4:** implement; `npx vitest run`, `npm run check`; screenshots light and dark at 1100 and 760 (`?view=sites`).
- [ ] **Step 5:** commit "Debug switch on the Sites page".

### Task 5: hands-on check on Windows

- [ ] Test home, PHP 8.4 from the catalog, a linked site: switch debugging on, check php.ini loads Xdebug and the Caddyfile has the trigger only for that site; with a stand-in listener on 9003 (only when the port is free), a request to the site connects to it and a request to another site does not; the page names the listening program. Stop everything by PID, delete the test home.
