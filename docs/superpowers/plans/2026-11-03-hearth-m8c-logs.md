# M8c Laravel Logs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Logs page shows the Laravel log files of every Hearth site, split into entries (time, level, message, stack trace), newest first, with a level filter, search and live updates, and lets the owner empty a log file.

**Architecture:** A `logs` module in the core finds `<site folder>/storage/logs/*.log` for each site of the sites registry, reads the last 2 MB of a chosen file and parses Monolog's line format into entries (a line starting with `[date] env.LEVEL:` opens an entry, every other line belongs to the one before). `logs.*` IPC methods feed a Logs page that polls while open and re-reads only when the file's size changed.

**Tech Stack:** Rust (hearth-core), Svelte 5 UI.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`, M8: "Laravel log viewer". Third M8 part after M8a dumps and M8b requests. The owner said "continue with the milestones" on 2026-11-03; choices below are the builder's.

## Global Constraints

- Never use em or en dashes, in code, comments, messages or docs.
- Only files directly in `<site folder>/storage/logs` whose name ends in `.log` and holds no path separator or `..` are ever read or emptied. Proxies have no folder and no logs.
- At most the last 2 MB of a file are read; a first entry cut by that is dropped; at most 500 entries, newest first; one entry's text is cut at 64 KB.
- Emptying a file truncates it to 0 bytes (the app may still hold it open); the page asks first.
- Laravel's daily logs (`laravel-2026-11-03.log`) list newest first by modified time.

## Review Focus

1. A huge log (hundreds of MB): reading stays fast and bounded (seek to the end, read 2 MB).
2. Entries with multi-line messages, JSON context and long stack traces (`#0 ... #57 {main}`, `[previous exception]`): one entry each, not one per line.
3. A log written while it is read (Laravel appending): no crash, a half-written last line is just part of the newest entry.
4. Non-UTF-8 bytes in a log: shown with replacement characters, nothing fails.
5. A file name from the UI like `..\..\.env` or `laravel.log/../x`: refused.

---

### Task 1: reading logs in the core

**Files:**
- Create: `crates/hearth-core/src/insights/logs.rs`, `insights/logs_tests.rs`
- Modify: `crates/hearth-core/src/insights/mod.rs`

**Interfaces:**
- Produces: `LogFile { name, size: u64, modified: u64 (unix secs) }`; `SiteLogs { site, files: Vec<LogFile> }`; `Entry { time: String, env: String, level: String (lowercase), message: String (first line), detail: String (the rest), truncated: bool }`; `pub fn sites(paths) -> Vec<SiteLogs>` (sites with at least one log file, sorted by name; files newest first); `pub fn parse(text: &str) -> Vec<Entry>` (newest first, at most 500); `pub fn read(paths, site, file) -> Result<(Vec<Entry>, u64 size), CoreError>`; `pub fn clear(paths, site, file) -> Result<(), CoreError>`.

- [ ] **Step 1: failing tests:** `parse_splits_entries_with_their_stack_traces`; `parse_handles_iso_dates_and_lowercase_levels` (`[2026-11-03T10:00:00.123456+00:00] production.ERROR:`); `a_cut_first_entry_is_dropped`; `sites_lists_log_files_newest_first_and_skips_proxies`; `read_takes_only_the_last_2_mb` (a 5 MB file); `bad_file_names_are_refused` (`..\x.log`, `a/b.log`, `x.txt`, `..`); `clear_empties_the_file`; `non_utf8_bytes_do_not_fail`.
- [ ] **Step 2:** `cargo test -p hearth-core insights::logs` fails.
- [ ] **Step 3:** implement (seek to `len - 2 MB`, `String::from_utf8_lossy`, a line opens an entry when it matches `^\[\d{4}-\d{2}-\d{2}[ T][^\]]*\] [A-Za-z0-9_-]+\.[A-Z]+: ` checked by hand, no regex crate).
- [ ] **Step 4:** tests pass, clippy clean.
- [ ] **Step 5:** commit "Read Laravel logs".

### Task 2: `logs.*` methods

**Files:**
- Create: `crates/hearth-core/src/insights/logs_api.rs` (or extend `insights/api.rs`), tests in `insights/api_tests.rs`
- Modify: `crates/hearth-core/src/ipc/mod.rs` if a new handler is added

**Interfaces:**
- Produces: `logs.sites` -> `SiteLogs[]`; `logs.read {site, file, size?}` -> `{size, entries}` or `{size, unchanged: true}` when `size` equals the file's size; `logs.clear {site, file}`.

- [ ] **Step 1: failing tests:** `sites_read_and_clear_through_the_core`; `an_unchanged_file_is_not_parsed_again`.
- [ ] **Step 2-4:** implement, tests pass.
- [ ] **Step 5:** commit "Log methods in the core".

### Task 3: the Logs page

**Files:**
- Create: `ui/src/logs.svelte.ts`, `ui/src/logs.spec.ts`, `ui/src/views/Logs.svelte`
- Modify: `ui/src/api.ts` (types, calls, demo data), `ui/src/App.svelte` (nav "Logs" after Requests)

Layout (builder's call): a bar with the site and file pickers, level chips (All, Errors, Warnings, Info, Debug: each shows that level and worse), a search field and "Empty file" (with a confirm row); then the entries, each a row with the level badge, time and first line, opening to show the rest (context and stack trace) in monospace. Polls `logs.read` with the known size every 2 s.

- [ ] **Step 1: failing tests:** level filter keeps that level and worse; search matches message and detail, case-insensitive; the poll sends the known size and keeps entries when unchanged; empty asks first and then calls `logs.clear`.
- [ ] **Step 2-4:** implement; `npx vitest run`, `npm run check`; screenshots light and dark at 1100 and 760 (`?view=logs`).
- [ ] **Step 5:** commit "Logs page".

### Task 4: hands-on check on Windows

- [ ] With a test home that links a scratch Laravel folder holding `storage/logs/laravel.log` (real Monolog output: an info line, a warning with context, an exception with a stack trace) and a 50 MB filler log: the page lists both files, shows the entries, filters and searches; appending a line shows up live; emptying works. Stop the app by its PID and delete everything.
