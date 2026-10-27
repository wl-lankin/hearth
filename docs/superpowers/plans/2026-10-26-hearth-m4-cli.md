# Hearth M4 (CLI + Herd compatibility) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** the `hearth` CLI ships with the app and is on PATH, accepts the Herd command shapes for everything Hearth already does (with `--json` on every listing), `php`/`composer`/`laravel` pick the PHP of the project you are in like Herd does, and a `herd` command forwards to Hearth so existing scripts keep working.

**Architecture:** one CLI binary (`hearth-cli`) that also answers to the names `php`, `composer`, `laravel` and `herd` (it looks at its own file name). The app copies it into `<home>/bin` on start (Windows: `hearth.exe` plus `php.exe`, `composer.exe`, `laravel.exe`, `herd.exe`; macOS: `hearth` plus symlinks), next to the existing `.bat`/sh shims, which now forward to it. Per-project PHP comes from a pure lookup in the sites registry, so `php` works without the app running.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (M4; the Shims and PHP lines of "Compatibility with an existing Herd install").

## Global Constraints

- Everything from M0 to M3 applies (public repo, no machine or company details, no long dashes, commits `M4: <description>`, branch `HEARTH-8` from `main`, push only on the owner's go).
- Command names, arguments and flags follow Herd's (`herd park [path]`, `herd isolate [phpVersion] [--site=]`, `herd secure [domain]`, `--json`); a domain may be given as `shop` or `shop.test`.
- Commands that belong to later milestones (services, mail, logs, insights, debug, processes, workspaces, node, mcp, db, share, studio) answer with exit code 1 and one line naming what is missing, e.g. "Hearth has no mail catcher yet".
- `php`, `composer`, `laravel` pass every argument through untouched (no shell in between on Windows), forward stdin/stdout/stderr, and return PHP's exit code.
- Herd's own `~/.config/herd/bin` is never touched (replacing it with forwarders is M13).

## Review Focus

1. **Arguments cmd would mangle** (`composer require "pkg:^1.2"`, `artisan test --filter="A|B"`, a 10 000-character argument list, quotes and spaces): reach PHP unchanged through `php.exe`/`composer.exe`. Tests in Tasks 2 and 3.
2. **Where am I**: the project root, a nested folder (`app/Models`), a folder inside a parked path, a linked folder, a folder that is in no site: the site's isolated PHP, else the default. Tests in Task 1.
3. **No default PHP and no isolation**: a clear "Install PHP first: hearth php:install 8.4", exit 1, not a crash. Test in Task 2.
4. **The app updates the CLI while a terminal runs `php`**: copying over a running `php.exe` fails on Windows; the copy goes to a new name and is swapped in on the next start, never leaving a half-written file. Test in Task 3.
5. **A Herd command for a later milestone** run by an old script: exit 1 with the "not yet" line, never a silent success. Test in Task 5.

---

### Task 1: The PHP of the folder you are in (core)

**Files:** `crates/hearth-core/src/sites/locate.rs`.

**Interfaces:** `site_for_dir(paths, dir) -> Option<Site>` (the longest site path containing `dir`, case-insensitive on Windows); `php_for_dir(paths, dir) -> Option<String>` (that site's isolation if installed, else the default minor).

- [ ] Failing tests: root, nested folder, parked subfolder, linked folder outside any parked path, a folder in no site (default), an isolation to a removed version (default), a link inside a parked folder wins over the parked name.
- [ ] Implement, GREEN, commit `M4: Find the site and PHP of a folder`.

### Task 2: Site-aware `php`, `composer`, `laravel`, `tinker` (CLI)

**Files:** `crates/hearth-cli/src/main.rs`, `crates/hearth-cli/src/run.rs`.

**Interfaces:** `hearth php [--site=<name>] [args...]`, `hearth composer [--site=] [args...]`, `hearth laravel [args...]`, `hearth tinker [--site=]` (runs `php artisan tinker` in the site's folder). The PHP comes from `--site`, else `php_for_dir(cwd)`. Child gets `PHPRC`, inherits stdio; the CLI exits with the child's code.

- [ ] Failing tests (a fake `php` that prints its args and version folder): an isolated project runs its version; `--site` overrides the folder; `"pkg:^1.2"` and `"A|B"` arrive intact; exit code 7 comes back as 7; no PHP at all gives the install hint.
- [ ] Implement, GREEN, commit `M4: Run the project's PHP`.

### Task 3: One binary, many names; shipped with the app

**Files:** `crates/hearth-cli/src/main.rs` (dispatch on the file stem), `crates/hearth-core/src/cli_install.rs`, `crates/hearth-core/src/php/shims.rs`, `src-tauri/src/lib.rs`, `src-tauri/tauri.release.conf.json` (externalBin for the CLI, used only by the release workflow), `.github/workflows/release.yml`.

**Interfaces:**
- Started as `php`, `composer`, `laravel` or `herd` (any case, with or without `.exe`), the CLI behaves as `hearth php ...`, `hearth composer ...`, `hearth laravel ...`, `hearth ...`.
- `install_cli(paths, source_dir) -> Result<bool, CoreError>`: copies the CLI from next to the running app into `<home>/bin` as `hearth(.exe)`; on Windows also as `php.exe`, `composer.exe`, `laravel.exe`, `herd.exe`; on macOS symlinks. Skips identical files; a locked target is written as `<name>.new` and swapped in on the next start.
- `.bat`/sh shims `php`, `composer`, `laravel`, `herd` forward to `hearth` when it is there (else today's direct PHP call); `php84` and friends stay direct.

- [ ] Failing tests: started under the name `php` it runs PHP; install copies all names and a second run copies nothing; a locked `php.exe` leads to `php.exe.new` and the next run swaps it; shims forward and fall back.
- [ ] Implement, GREEN, release workflow builds the CLI before `tauri build`, commit `M4: Ship the CLI and answer to php, composer and herd`.

### Task 4: The Herd command set Hearth can serve today (CLI)

**Interfaces (new or aliased):** `parked`, `paths`, `forget [path]` (= unpark), `proxies`, `isolated`, `which`, `which-php [site]`, `use [phpVersion]` (= php:use), `php:update [phpVersion]` (= php:install), `site:info [path]` / `site-information [path]`, `link [name] [--secure] [--isolate=]`, `proxy domain host [--secure]`, `unsecure [domain] [--all]`, `restart|start|stop [service]` (web server and PHP pools), `tld` (prints `test`), `loopback` (prints `127.0.0.1`). Domains accept `shop` and `shop.test`.

- [ ] Failing CLI tests for each new command and flag against a served test core, including `--json` output shapes.
- [ ] Implement, GREEN, commit `M4: Herd's command set`.

### Task 5: JSON everywhere and honest "not yet"

**Interfaces:** every listing and info command takes `--json` (stable field names, camelCase). Herd commands of later milestones print "Hearth has no <feature> yet" and exit 1.

- [ ] Failing tests: `--json` parses for every listing; `herd mail:list` and `herd services:list` exit 1 with their lines.
- [ ] Implement, GREEN, commit `M4: JSON output and later-milestone commands`.

### Task 6: End-to-end check (test home, no admin rights needed)

- [ ] With a test home and the app running: `herd link`, `herd secure`, `herd proxy ... --secure`, `herd isolate 8.3` through the `herd` command in `<home>/bin`; `php -v` in an isolated project prints 8.3 and the default elsewhere; `composer require "psr/log:^3.0"` in a scratch project keeps the caret in composer.json; Ctrl+C in `php artisan serve` stops it.

## Done when

- A shell with `<home>/bin` on PATH runs `herd ...`, `php`, `composer`, `laravel` the Herd way for everything Hearth already does, without cmd mangling arguments, and every listing has `--json`.
