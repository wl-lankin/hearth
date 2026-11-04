# Hearth M9b: SPX Profiler (macOS) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Profile a site with SPX in one click on macOS and see its profiles in Hearth. On Windows, Hearth says SPX is not available.

**Architecture:** hearth-php builds SPX as a shared extension next to Xdebug in the macOS builds. The catalog marks it as a new kind, `so`, but only for archives that really contain it. Hearth gets a per-site Profile switch, modeled on Debug. Caddy passes SPX's switches for that site as `HTTP_SPX_*` FastCGI params, which SPX reads from `$_SERVER` like request headers. The PHP running the site writes reports to Hearth's data folder. A Profiles page lists those reports and opens each one in SPX's own web UI, served through the site.

**Tech Stack:** static-php-cli 2.8.5, GitHub Actions, Node 24 (`node --test`), Rust (hearth-core), Svelte 5.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (M9: "SPX profiler view"). Owner decision 2026-11-04: SPX on macOS only, because upstream SPX has no Windows build (php-spx PR #263 has been stalled since Dec 2025).

## Global Constraints

- No em or en dashes anywhere. Everything in English. No attribution footers.
- Branches: `HEARTH-20` in hearth-php (from `origin/main`) and `HEARTH-21` in Hearth (stacked on `HEARTH-19`, because PR #15 is not merged yet). PRs go to `main`.
- hearth-php: no CI run may publish a release from a pull request.
- Windows behavior must not change, apart from the new page and its "not available" text.
- Ports 2525, 2306 and 9003 are never bound in tests.

## Design calls (mine, the owner can overrule)

- **One switch per site.** Profiling records every PHP request of that site (`SPX_AUTO_START=1`) until it is switched off. This is the same model as Debug.
- **Settings:** `spx.http_enabled=1`, `spx.http_key=<random 32 hex, made once and kept in config/spx.json>`, `spx.http_ip_whitelist=127.0.0.1`, `spx.data_dir=<data>/spx`. Only localhost with the key can reach the SPX UI.
- **Report view:** reuse SPX's own UI (flame graph, timeline). The Profiles page opens `<site url>/?SPX_KEY=<key>&SPX_UI_URI=/report.html&key=<report key>` in the browser. A Hearth-native flame graph is out of scope.
- **Housekeeping:** the Profiles page lists the newest 200 reports, and "Clear" deletes them all. There is no automatic deletion.

## Review Focus

1. A catalog regenerated from older releases must not claim `spx` for archives that lack `lib/spx.so`.
2. The PR-time macOS build check must never upload a release or touch the `manifest` release.
3. A site on a PHP whose build has no spx (Windows, or an old macOS archive) must fail clearly when profiling is switched on, and the UI must not offer the switch.
4. Profile metadata files that are half written or malformed are skipped, never an error for the whole list.
5. Unlinking a site removes it from the profile set (as with debug).

---

### Task 1 (hearth-php, HEARTH-20): Catalog knows `so` and finds spx in the archive

**Files:** `lib/manifest.mjs`, `scripts/macos-entry.mjs`, `test/manifest.test.mjs`, `test/macos.test.mjs`, `README.md`

**Interfaces:**
- Produces: `macosEntry({version, arch, url, sha256, size, spx})`. With `spx: true` it adds `extensions.spx = {kind: 'so', file: 'lib/spx.so'}`; with `spx` false or missing, the extensions stay as today.
- Produces: `tarHas(gzBuffer, path) -> boolean`. Reads a tar.gz and reports whether it holds a regular file at `path`.

- [ ] Write tests: `validate` accepts `{kind:'so', file:'lib/spx.so'}` and rejects `so` without `file`. `macosEntry` without `spx` has no spx key. `tarHas` finds `php-8.4.1/lib/spx.so` in a tar.gz built in the test (write it with `tar` from a temp folder, or hand-build the 512-byte headers) and returns false for a missing path. `entriesFromReleases` with a `fetch` that returns that tar.gz sets spx exactly when the file is there (the hash function now returns `{sha256, spx}`).
- [ ] Run `npm test` and expect FAIL.
- [ ] Implement. `entry` takes the archive path and sets `spx` from `tarHas`. `releases` gets it from the downloaded buffer it already hashes. Add the README line for `so`.
- [ ] Run `npm test` and expect PASS. Commit "HEARTH-20: Catalog marks spx only in archives that hold it".

### Task 2 (hearth-php, HEARTH-20): Build and smoke-test SPX, checked on pull requests without releasing

**Files:** `.github/workflows/macos.yml`, `.github/workflows/ci.yml`, `README.md`

- [ ] `macos.yml`:
  - Download with `--for-extensions="$EXT,xdebug,spx"` and build with `--build-shared=xdebug,spx`.
  - Package `lib/spx.so` (taken with `find`, like `xdebug.so`).
  - Smoke test: `php -d extension=$PWD/$DIR/lib/spx.so -m | grep -qi '^spx$'`, then profile a CLI script with `SPX_ENABLED=1 SPX_REPORT=full` and `-d spx.data_dir=$PWD/spxdata`, and check that a `.json` file and a `.txt.gz` file appear.
  - New input `release` (boolean, default `true`). The `release` job runs only when it is true.
- [ ] `ci.yml`: on pull requests that change `.github/workflows/macos.yml`, `build/extensions.txt` or `scripts/macos-entry.mjs`, call `macos.yml` with `minors: '["8.1","8.5"]'` and `release: false`. Use `permissions: contents: read` (the build jobs need nothing more).
- [ ] Check: `npm test` passes, and both YAML files parse with `node -e` (js-yaml is not a dependency, so use `npx --yes yaml@2.8.1 valid < file`).
- [ ] Commit "HEARTH-20: Build SPX for macOS and check the build on pull requests". Pushing and running the PR's CI is part of the finishing step (option 2). The build check passes there, or the task is not done.

### Task 3 (Hearth, HEARTH-21): `so` extensions and the SPX ini

**Files:** `crates/hearth-core/src/php/manifest.rs`, `php/ini.rs` (+ its tests), `php/service.rs`, new `crates/hearth-core/src/profile/mod.rs` (`spx_key(paths) -> Result<String>` and `data_dir(paths) -> PathBuf`)

- [ ] Write tests:
  - `ExtensionSource::So { file }` parses from `{"kind":"so","file":"lib/spx.so"}`.
  - `render_ini` with spx on writes `extension="<install>/lib/spx.so"` plus the four `spx.` lines (key, data dir quoted, whitelist, http_enabled).
  - `spx_key` returns the same 32-hex key on a second call.
- [ ] Run and expect FAIL. Implement: `IniContext` gets `spx: Option<(String, PathBuf)>`, which `generated_ini` fills when spx is on. Run and expect PASS.
- [ ] Commit "HEARTH-21: Load SPX with Hearth's key and folder".

### Task 4 (Hearth): Profile switch per site

**Files:** `sites/registry.rs` (`profile: BTreeSet<String>`, `Site.profile`, `set_profile`, unlink cleanup), `sites/caddyfile.rs` (`Route::Php { .., spx: Option<String> }`, holding the key), `web/service.rs`, `sites/api.rs` (`sites.profile {name,on}`), tests next to each.

- [ ] Write tests:
  - Caddyfile: a profiled site gets `env HTTP_SPX_ENABLED 1`, `env HTTP_SPX_KEY <key>` and `env HTTP_SPX_AUTO_START 1` inside `php_fastcgi`, and other sites get none.
  - `sites.profile` on a PHP whose build has spx switches spx on and records the site. On a build without spx the error says "SPX is not available for PHP 8.4", and nothing changes. A proxy is refused.
  - Unlinking removes the name from the set.
  - The web sync note: "SPX is off for PHP X: switch it on on the PHP page".
- [ ] Run and expect FAIL. Implement by mirroring `debug` (effective PHP, `set_extension(.., "spx", true)`, `change(set_profile)`). Run and expect PASS. Commit "HEARTH-21: Profile one site at a time".

### Task 5 (Hearth): Profiles methods

**Files:** `profile/reports.rs` (+ `reports_tests.rs`), `ipc` routing for `profile.list`, `profile.clear` and `profile.status`.

- [ ] Write tests on a temp data dir with SPX-shaped `*.json` metadata. Each item reads `key, exec_ts, http_host, http_method, http_request_uri, cli, cli_command_line, wall_time_ms, peak_memory_usage, called_function_count`.
  - The list comes newest first and is capped at 200.
  - A malformed or partly written json is skipped.
  - `clear` deletes `.json` and `.txt.gz` files only.
  - `profile.status` returns `{available, key}`. `available` is true when any installed PHP build has spx.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-21: List and clear SPX profiles".

### Task 6 (Hearth UI): Profile switch and Profiles page

**Files:** `ui/src/api.ts` (types, demo data with two profiles), `ui/src/sites.svelte.ts` + `sites.spec.ts`, new `ui/src/profiles.svelte.ts` + `profiles.spec.ts`, new `ui/src/views/Profiles.svelte`, `ui/src/views/Sites.svelte`, `ui/src/App.svelte` (nav item "Profiles" after "Requests").

- [ ] Write vitest tests:
  - The Sites store offers the Profile switch only when the site's effective PHP lists spx, and rolls back a refusal.
  - The profiles store builds the report URL `http://shop.test:8080/?SPX_KEY=k&SPX_UI_URI=/report.html&key=r` (https and the https port for secured sites, as `url()` does today).
  - When `available` is false, the store shows "SPX profiling runs on macOS only." and lists nothing.
- [ ] Run and expect FAIL. Implement. The page shows a table (time, method, URL or command, wall time, memory, functions) with Open (opens the browser via the existing opener) and Clear. Run `npx vitest run` and `npm run check` and expect PASS.
- [ ] Screenshots of light and dark in demo mode. Commit "HEARTH-21: Profiles page and the Profile switch".

### Task 7: Verification

- [ ] `cargo test --workspace` (with `HEARTH_TEST_PHP`), clippy, `npx vitest run` and `npm run check` all pass.
- [ ] No hands-on run on macOS is possible here. Ledger a ruling naming what only a Mac can confirm: SPX loads in php-fpm, reports land in the data dir, and the UI opens through the site.
