# Hearth M0 (Brand + Skeleton) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An installable-from-source Hearth app for Windows and macOS: Rust core with a local IPC server, a `hearth` CLI that talks to it, a Tauri tray app with a Svelte main window (Dashboard + Settings), the chosen logo and icons, and CI on both OSes.

**Architecture:** A Cargo workspace with three crates. `hearth-core` (library) owns paths, settings and the IPC server/client; the Tauri app (`src-tauri`) hosts the core server in-process; `hearth-cli` is a thin client. Every later milestone adds IPC methods to the core, so UI, CLI and MCP share one implementation.

**Tech Stack:** Rust (stable, edition 2021), Tauri 2 (`tray-icon`), tauri-plugin-single-instance, tauri-plugin-autostart, tokio, interprocess (local sockets), serde/serde_json, clap 4, thiserror; Svelte 5 + TypeScript + Vite 8 + Vitest 5 (same versions as Kern: svelte ^5.57, @sveltejs/vite-plugin-svelte ^7.3, vite ^8.3, vitest ^5.0, typescript ^5.9, @tauri-apps/cli ^2.12, @tauri-apps/api ^2.12), @resvg/resvg-js for icons.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`

## Global Constraints

- Repo: `C:\Projects\private\Hearth`, branch `main`, PUBLIC GitHub repo `wl-lankin/hearth` (code + releases). Repo-local git identity `Wolfgang Linz <27097016+wl-lankin@users.noreply.github.com>` set before the first commit.
- Nothing machine- or company-specific in the repo: no local user paths, no `.test` hostnames of real projects, no names of the owner's work projects or employer (machine details go to the git-ignored `docs/local/`).
- Tauri identifier `de.wolfganglinz.hearth`, productName `Hearth`, CLI binary `hearth`. Publisher "Wolfgang Linz", homepage https://wolfgang-linz.de, license MIT.
- Hearth home: `~/.config/hearth` on both OSes, overridable with env `HEARTH_HOME` (used by every test). Subdirs: `bin`, `config`, `data`, `logs`, `run`.
- M0 opens no TCP ports at all (Herd keeps running side by side): IPC is a named pipe (Windows) / Unix socket (macOS) only.
- No em/en dashes in any UI string, doc, comment or commit message. English only.
- Commit messages: `M0: <description>`. Commit after every task; never push unless the owner asks.
- Windows shell gotchas: files use LF (`.gitattributes` `* text=auto eol=lf`); use Write/Edit tools for files, not Bash heredocs.

## Review Focus

1. **Core already running**: launching the app twice, or the CLI while the app is closed, must give a clear result (focus existing window / "Hearth core is not running", exit 1), never a hang. Tests in Tasks 4, 5, 6.
2. **Corrupt or hand-edited settings.json**: the app must start with defaults and keep the broken file as `settings.json.corrupt-<unix-ts>`. Test in Task 3.
3. **Home path with spaces and non-ASCII** (Windows user names like `Jörg Müller`): paths, settings and the IPC endpoint must work. Tests in Tasks 2 and 4.
4. **Stale Unix socket after a crash (macOS)**: a leftover `hearth.sock` with no listener must be replaced, while a live one makes the second server fail with `AlreadyRunning`. Test in Task 4.
5. **Two Hearth homes on one machine** (tests or a dev build next to the installed app): each home gets its own IPC endpoint so they never talk to each other. Test in Task 4.

---

## File Structure

```
Hearth/
  Cargo.toml                    workspace: crates/*, src-tauri
  package.json, vite.config.ts, svelte.config.js, tsconfig.json
  LICENSE, README.md, .gitignore, .gitattributes
  assets/logo-a.svg, logo-b.svg, logo.svg (chosen), logo-preview.html
  scripts/make-icons.mjs
  crates/hearth-core/src/{lib.rs, paths.rs, settings.rs, atomic.rs, error.rs}
  crates/hearth-core/src/ipc/{mod.rs, protocol.rs, endpoint.rs, server.rs, client.rs}
  crates/hearth-cli/src/main.rs, tests/status.rs
  src-tauri/{Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/}
  src-tauri/src/{main.rs, lib.rs, commands.rs, tray.rs, startup.rs}
  ui/index.html
  ui/src/{main.ts, App.svelte, api.ts, theme.ts, settings.svelte.ts, styles.css}
  ui/src/views/{Dashboard.svelte, Settings.svelte}
  ui/src/*.spec.ts
  .github/workflows/ci.yml
```

---

### Task 1: Repository scaffold

**Files:**
- Create: everything at the top level listed above, `crates/hearth-core/{Cargo.toml,src/lib.rs}`, `crates/hearth-cli/{Cargo.toml,src/main.rs}`, `ui/index.html`, `ui/src/main.ts`, `ui/src/App.svelte`, `ui/src/smoke.spec.ts`

**Interfaces:**
- Produces: workspace that builds; npm scripts `dev`, `build`, `check` (`svelte-check --tsconfig ./tsconfig.json`), `test` (`vitest run`), `tauri`, `icons`. Vite root `ui`, outDir `../dist`, dev port 1440 strict. Vitest config copied from Kern (`resolve.conditions: ['browser']` under VITEST, include `src/**/*.spec.ts`).

- [ ] **Step 1:** `git init -b main`, set the repo-local noreply identity (Global Constraints).
- [ ] **Step 2:** Write `.gitignore` (node_modules, dist, target, `src-tauri/gen`, `.superpowers/`), `.gitattributes` (`* text=auto eol=lf`, `*.png binary`, `*.ico binary`, `*.icns binary`), MIT `LICENSE` (Copyright 2026 Wolfgang Linz), `README.md` (one paragraph: what Hearth is, "work in progress", MIT).
- [ ] **Step 3:** Workspace `Cargo.toml` with `members = ["crates/hearth-core", "crates/hearth-cli"]` (Task 6 adds `"src-tauri"`), `resolver = "2"`, shared `[workspace.package] version = "0.1.0"`, and the release profile from Kern (`opt-level 3, lto, codegen-units 1, panic abort, strip`). `hearth-core` lib with one `#[test] fn it_builds() {}`; `hearth-cli` bin `hearth` printing nothing yet.
- [ ] **Step 4:** `package.json` (`"name": "hearth"`, `"version": "0.1.0"`, private, type module, author/license as Glint) + Svelte/Vite/Vitest config; `ui/src/smoke.spec.ts` with `expect(1 + 1).toBe(2)`.
- [ ] **Step 5:** Run `npm install`, `npm run check`, `npm test`, `cargo test --workspace`. Expected: all green.
- [ ] **Step 6:** Commit `M0: Scaffold workspace and frontend`.

### Task 2: Hearth paths

**Files:**
- Create: `crates/hearth-core/src/paths.rs`, `crates/hearth-core/src/error.rs`
- Test: inline `#[cfg(test)] mod tests` in `paths.rs` (dev-dep `tempfile = "3"`)

**Interfaces:**
- Produces:
  - `pub struct HearthPaths { pub root: PathBuf, pub bin: PathBuf, pub config: PathBuf, pub data: PathBuf, pub logs: PathBuf, pub run: PathBuf }`
  - `HearthPaths::from_env() -> Result<HearthPaths, CoreError>`: `HEARTH_HOME` if set and non-empty, else `<home_dir>/.config/hearth` (use `dirs::home_dir`).
  - `HearthPaths::at(root: impl Into<PathBuf>) -> HearthPaths`
  - `HearthPaths::ensure(&self) -> Result<(), CoreError>` creates all dirs.
  - `pub enum CoreError` (thiserror): `NoHomeDir`, `Io { path: PathBuf, source: io::Error }`, `AlreadyRunning`, `NotRunning`, `Protocol(String)`, `Rpc { code: i64, message: String }`.

- [ ] **Step 1: Write the failing tests**
  - `at_builds_the_five_subdirs`: `HearthPaths::at("/x")` has `bin == /x/bin`, `config == /x/config`, `data`, `logs`, `run` likewise.
  - `from_env_prefers_hearth_home`: with `HEARTH_HOME` set to a temp dir, `root` equals it (serialize env tests with a static `Mutex`).
  - `ensure_creates_dirs_under_unicode_path_with_spaces`: root = `tempdir/Jörg Müller/hearth`; after `ensure()` all five dirs exist.
- [ ] **Step 2:** Run `cargo test -p hearth-core paths`. Expected: FAIL (module missing).
- [ ] **Step 3:** Implement as specified.
- [ ] **Step 4:** Run again. Expected: PASS.
- [ ] **Step 5:** Commit `M0: Add Hearth home paths`.

### Task 3: Settings store

**Files:**
- Create: `crates/hearth-core/src/settings.rs`, `crates/hearth-core/src/atomic.rs`

**Interfaces:**
- Consumes: `HearthPaths` (Task 2).
- Produces:
  - `#[serde(rename_all = "camelCase")] pub struct Settings { pub theme: Theme, pub launch_at_login: bool, pub start_hidden: bool }`, `#[serde(default)]` on the struct so missing keys take defaults.
  - `#[serde(rename_all = "lowercase")] pub enum Theme { System, Light, Dark }`. Default `Settings { theme: System, launch_at_login: false, start_hidden: false }`.
  - `pub fn load(paths: &HearthPaths) -> Settings` (never fails: missing file = default; unparseable = rename to `settings.json.corrupt-<unix-ts>` then default).
  - `pub fn save(paths: &HearthPaths, s: &Settings) -> Result<(), CoreError>` via `atomic::write(path, bytes)` (write `<file>.tmp`, flush + sync, rename over the target), pretty JSON with a trailing newline.
  - File: `<config>/settings.json`.

- [ ] **Step 1: Write the failing tests**
  - `missing_file_gives_defaults`
  - `save_then_load_roundtrips` (theme Dark, launchAtLogin true)
  - `json_uses_camel_case_keys`: saved text contains `"launchAtLogin"` and `"theme": "dark"`.
  - `unknown_and_missing_keys_are_tolerated`: file `{"theme":"light","future":1}` loads as theme Light, other fields default.
  - `corrupt_file_is_kept_aside`: file `{not json` -> `load` returns defaults, `settings.json` is gone, exactly one `settings.json.corrupt-*` exists with the original bytes.
  - `atomic_write_leaves_no_tmp_file`.
- [ ] **Step 2:** Run `cargo test -p hearth-core settings`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit `M0: Add settings store with atomic writes`.

### Task 4: IPC protocol, endpoint, server and client

**Files:**
- Create: `crates/hearth-core/src/ipc/{mod.rs, protocol.rs, endpoint.rs, server.rs, client.rs}`
- Deps: `tokio` (rt-multi-thread, macros, io-util, sync, time), `interprocess` (feature `tokio`), `sha2`, `whoami`.

**Interfaces:**
- Consumes: `HearthPaths`, `CoreError`.
- Produces:
  - Protocol: JSON-RPC 2.0, one JSON object per line (`\n`-delimited, UTF-8). `pub struct Request { jsonrpc, id: u64, method: String, params: serde_json::Value }`, `pub struct Response { jsonrpc, id: u64, result: Option<Value>, error: Option<RpcError> }`, `pub struct RpcError { code: i64, message: String }`. Codes: `-32700` parse error, `-32601` method not found, `-32602` invalid params, `-32603` internal.
  - `pub fn endpoint_name(paths: &HearthPaths) -> String`: `hearth-<user>-<first 10 hex of sha256(canonical root as UTF-8)>`. Windows: used as pipe name `\\.\pipe\<name>`; macOS: socket file `<run>/hearth.sock` (the hash keeps names unique on Windows; on macOS the socket lives in the home).
  - `#[async_trait]`-free handler: `pub type Handler = Arc<dyn Fn(String, Value) -> BoxFuture<'static, Result<Value, RpcError>> + Send + Sync>`.
  - `pub async fn serve(paths: &HearthPaths, handler: Handler) -> Result<ServerHandle, CoreError>`: binds, returns `AlreadyRunning` if a live server answers `core.ping` on the endpoint; on macOS removes a stale socket file that refuses connections. `ServerHandle::shutdown(self)` stops accepting and removes the socket file.
  - `pub async fn call(paths: &HearthPaths, method: &str, params: Value) -> Result<Value, CoreError>`: connect timeout 2 s, response timeout 10 s; `NotRunning` when nothing listens; `Rpc{code,message}` for error responses.
  - Built-in methods answered by `core::router(state)` in `ipc/mod.rs`: `core.ping` -> `"pong"`; `core.status` -> `{ "version": env!("CARGO_PKG_VERSION"), "pid": u32, "uptimeSecs": u64, "home": String }`. `router(state: CoreState) -> Handler` where `pub struct CoreState { pub paths: HearthPaths, pub started: Instant }`.

- [ ] **Step 1: Write the failing tests** (`#[tokio::test]`, each with its own temp `HearthPaths`)
  - `ping_roundtrip`: serve router, `call("core.ping")` == `"pong"`.
  - `status_reports_version_pid_home`.
  - `unknown_method_is_32601`.
  - `malformed_line_gets_parse_error_and_connection_survives`: raw client writes `{oops\n`, reads `-32700`, then a valid ping on the same connection succeeds.
  - `second_server_same_home_is_already_running`.
  - `two_homes_do_not_collide`: two servers in two temp homes both start; each `core.status` returns its own home.
  - `call_without_server_is_not_running` (returns within 3 s).
  - `unicode_home_with_spaces_works` (`Jörg Müller`).
  - `#[cfg(unix)] stale_socket_is_replaced`: create a plain file at `<run>/hearth.sock`, `serve` succeeds.
- [ ] **Step 2:** Run `cargo test -p hearth-core ipc`. Expected: FAIL.
- [ ] **Step 3:** Implement. One tokio task per connection; read lines with a 1 MiB max line length (longer = parse error and close).
- [ ] **Step 4:** Run. Expected: PASS on Windows locally; macOS covered by CI (Task 9).
- [ ] **Step 5:** Commit `M0: Add local IPC server and client`.

### Task 5: `hearth` CLI

**Files:**
- Modify: `crates/hearth-cli/src/main.rs`, `crates/hearth-cli/Cargo.toml` (clap derive, tokio, serde_json, hearth-core; dev: `assert_cmd`, `predicates`, `tempfile`)
- Test: `crates/hearth-cli/tests/status.rs`

**Interfaces:**
- Consumes: `HearthPaths::from_env`, `ipc::call`, `ipc::serve`, `ipc::router` (Task 4).
- Produces: `hearth --version` (prints `hearth 0.1.0`), `hearth status [--json]`.
  - Running: `Hearth core is running (v0.1.0, pid 1234, up 42s)`, exit 0. `--json`: the raw `core.status` object.
  - Not running: stderr `Hearth core is not running. Start the Hearth app.`, exit 1. `--json`: `{"running":false}` on stdout, exit 1.

- [ ] **Step 1: Write the failing tests** (set `HEARTH_HOME` to a temp dir on the command)
  - `status_when_not_running_exits_1_with_message`
  - `status_json_when_not_running`
  - `status_when_running`: start `ipc::serve(router)` in the test's tokio runtime for the same home, then run the binary; stdout matches `^Hearth core is running \(v0\.1\.0, pid \d+, up \d+s\)$`.
  - `version_flag`.
- [ ] **Step 2:** Run `cargo test -p hearth-cli`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit `M0: Add hearth CLI with status command`.

### Task 6: Tauri app shell (core host, tray, single instance, settings commands)

**Files:**
- Create: `src-tauri/{Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json}`, `src-tauri/src/{main.rs, lib.rs, commands.rs, tray.rs, startup.rs}`
- Modify: workspace `Cargo.toml` members, add `"src-tauri"`.
- Package name `hearth-app`, lib `hearth_app_lib`. Deps: `tauri` (`tray-icon`), `tauri-plugin-single-instance`, `tauri-plugin-autostart`, `hearth-core`, tokio, serde_json.

**Interfaces:**
- Consumes: `HearthPaths`, `settings::{load, save, Settings}`, `ipc::{serve, router, CoreState}`.
- Produces (Tauri commands, used by Task 7):
  - `core_status() -> Result<CoreStatus, String>` (same shape as `core.status`, served from the in-process `CoreState`, no IPC round trip)
  - `settings_get() -> Settings`
  - `settings_set(settings: Settings) -> Result<Settings, String>`: saves, then applies `launch_at_login` through the autostart plugin; on autostart failure returns `Err("Could not change Start at login: <reason>")` and does not save.
- `startup.rs`: `pub fn start_hidden(settings: &Settings, args: &[String]) -> bool` = `settings.start_hidden || args.contains("--hidden")`. Autostart entry is registered with arg `--hidden`.
- Behaviour:
  - On setup: `paths.ensure()`, load settings, spawn `ipc::serve` on the Tauri async runtime. If it returns `AlreadyRunning` (another Hearth with the same home), show a native message "Hearth is already running." and exit 0 (the single-instance plugin normally catches this first).
  - Single instance: a second launch shows, unminimizes and focuses the main window.
  - Window: label `main`, title "Hearth", 1100x720, min 760x520, centered, `visible: false` in the config; shown after setup unless `start_hidden`. Close button hides to tray (prevent close).
  - Tray: tooltip "Hearth", left click toggles the window, menu "Open Hearth", separator, "Quit Hearth". Quit shuts the IPC server down, then exits.
  - `tauri.conf.json`: productName/identifier/publisher/license per Global Constraints, CSP like Glint (`default-src 'self'`, `script-src 'self'`, `style-src 'self' 'unsafe-inline'`, `connect-src 'self' ipc: http://ipc.localhost`) with `dangerousDisableAssetCspModification: ["style-src"]`, bundle targets `nsis`, `app`, `dmg`, NSIS `installMode: currentUser`, macOS minimum 11.0. Updater is NOT configured in M0 (M14).

- [ ] **Step 1: Write the failing tests** in `startup.rs`: `start_hidden_from_setting`, `start_hidden_from_flag`, `visible_by_default`.
- [ ] **Step 2:** Run `cargo test -p hearth-app`. Expected: FAIL.
- [ ] **Step 3:** Implement the app as described. Generate placeholder icons with `npx tauri icon` from a plain 1024 px square until Task 8 replaces them.
- [ ] **Step 4:** Run `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Manual smoke (ask the owner before running, the window appears on their screen):** `HEARTH_HOME=<scratch dir> npx tauri dev`. Check: window appears; tray icon present; close hides; tray click shows; second `npx tauri dev`-built exe launch focuses the first; `HEARTH_HOME=<same> cargo run -p hearth-cli -- status` prints running; Quit removes the tray icon and `hearth status` then exits 1.
- [ ] **Step 6:** Commit `M0: Add Tauri app hosting the core with tray and single instance`.

### Task 7: UI shell (Dashboard + Settings, themes)

**Files:**
- Create: `ui/src/{api.ts, theme.ts, settings.svelte.ts, styles.css}`, `ui/src/views/{Dashboard.svelte, Settings.svelte}`
- Modify: `ui/src/App.svelte`, `ui/index.html`
- Test: `ui/src/theme.spec.ts`, `ui/src/settings.spec.ts` (`// @vitest-environment jsdom`)

**Interfaces:**
- Consumes: Tauri commands from Task 6.
- Produces:
  - `api.ts`: `coreStatus(): Promise<CoreStatus>`, `getSettings(): Promise<Settings>`, `setSettings(s: Settings): Promise<Settings>`; TS types `Settings { theme: 'system' | 'light' | 'dark'; launchAtLogin: boolean; startHidden: boolean }`, `CoreStatus { version: string; pid: number; uptimeSecs: number; home: string }`.
  - `theme.ts`: `resolveTheme(setting: Settings['theme'], prefersDark: boolean): 'light' | 'dark'`; `applyTheme(t)` sets `document.documentElement.dataset.theme`.
  - `settings.svelte.ts`: `createSettingsStore(api: Pick<typeof import('./api'), 'getSettings' | 'setSettings'>)` returning `{ value: Settings, error: string | null, load(): Promise<void>, update(patch: Partial<Settings>): Promise<void> }`. `update` applies optimistically, rolls back and sets `error` when `setSettings` rejects.
- Layout: left sidebar (logo + "Hearth", nav: Dashboard, Settings; app version at the bottom), content area. Dashboard: a "Core" card (Running, version, uptime refreshed every 5 s, home path) and an empty-state card "Sites, PHP and services arrive in the next milestones." Settings: Appearance (System / Light / Dark segmented control), Startup ("Start at login", "Start hidden in the tray"). Errors show as an inline message under the control.
- Colors as CSS custom properties on `:root[data-theme=light|dark]`, warm neutrals with an ember accent taken from the chosen logo (Task 8 sets the final accent value; use `#e8642c` until then). System font stack, no web fonts.

- [ ] **Step 1: Write the failing tests**
  - `resolveTheme('system', true) === 'dark'`, `('system', false) === 'light'`, `('light', true) === 'light'`, `('dark', false) === 'dark'`.
  - Store: `load` fills `value` from the fake api; `update({theme:'dark'})` calls `setSettings` with the merged object and keeps it; when `setSettings` rejects with `"Could not change Start at login: denied"`, `value.launchAtLogin` returns to `false` and `error` equals that message.
- [ ] **Step 2:** Run `npm test`. Expected: FAIL.
- [ ] **Step 3:** Implement the modules and views. `App.svelte` listens to `matchMedia('(prefers-color-scheme: dark)')` changes while theme is `system`.
- [ ] **Step 4:** Run `npm test && npm run check`. Expected: PASS, 0 svelte-check errors.
- [ ] **Step 5: Screenshot check:** `npm run dev` with a `?demo` mode in `api.ts` that returns canned data when `window.__TAURI_INTERNALS__` is missing; screenshot Dashboard and Settings in light and dark at 1100x720 and 760x520 into the scratchpad; fix overflow/contrast issues.
- [ ] **Step 6:** Commit `M0: Add Dashboard and Settings UI with themes`.

### Task 8: Logo and icons (owner decision gate)

**Files:**
- Create: `assets/logo-a.svg`, `assets/logo-b.svg`, `assets/logo-preview.html`, `scripts/make-icons.mjs`; then `assets/logo.svg`, `src-tauri/icons/*`, `docs/logo.png`, `src-tauri/icons/tray.png` + `tray@2x.png` (macOS template variants: `tray-template.png`)

**Interfaces:**
- Produces: final `assets/logo.svg`; accent color constant written into `ui/src/styles.css` `--accent`.

- [ ] **Step 1:** Draw two 1024x1024 SVG drafts (rounded square tile, works at 16 px):
  - **A, "Arch":** a rounded arch (fireplace opening / house silhouette) in warm off-white on a charcoal tile, one soft flame inside in an ember gradient (amber `#f6a33b` to deep red `#c2361f`).
  - **B, "Ember H":** two pillars forming an H whose crossbar is a glowing ember bar, same palette.
  - No sparkles, no stars, nothing that reads as an AI icon.
- [ ] **Step 2:** `logo-preview.html` shows both at 512, 64, 32, 16 px on light and dark backgrounds plus a mock Windows taskbar and macOS menu bar strip. Publish it as an Artifact for the owner and ask: A, B, or changes. **Stop until the owner picks.**
- [ ] **Step 3:** Copy the pick to `assets/logo.svg`; adapt Glint's `scripts/make-icons.mjs` (app icons via `tauri icon`, `docs/logo.png` at 512, tray PNG 32/64 px; macOS tray as a monochrome template image).
- [ ] **Step 4:** Run `npm run icons`; check `src-tauri/icons/icon.ico` and `icon.icns` exist; update `--accent`; run the Task 6 smoke (owner asked first) to see the tray icon.
- [ ] **Step 5:** Commit `M0: Add Hearth logo and icons`.

### Task 9: CI on Windows and macOS

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: workflow `ci` on push to `main` and pull requests; matrix `windows-latest`, `macos-latest`; `permissions: contents: read`.

- [ ] **Step 1:** Copy Glint's `ci.yml` (same SHA-pinned actions) and extend the steps: `npm ci`, `npm run check`, `npm test`, `npx vite build`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- [ ] **Step 2:** Run the same commands locally on Windows. Expected: all green, no clippy warnings.
- [ ] **Step 3:** Commit `M0: Add CI for Windows and macOS`.

### Task 11: Start with the OS by default (owner request 2026-10-23)

**Files:**
- Modify: `src-tauri/src/startup.rs`, `src-tauri/src/lib.rs`
- Create: `src-tauri/windows/hooks.nsh` (uninstall removes the login entry), `tauri.conf.json` `bundle.windows.nsis.installerHooks`

**Interfaces:**
- Produces: `startup::first_run_settings(first_run: bool, release_build: bool, current: &Settings) -> Option<Settings>`: `Some(Settings { launch_at_login: true, ..current })` only when both flags are true, else `None`. First run = `settings.json` did not exist when the app started (`hearth_core::settings::exists(&paths) -> bool`, new).
- Behaviour: on a first run of a release build, enable autostart (entry passes `--hidden`) and save the settings; failure is logged to stderr and leaves the setting off. The Settings switch keeps working both ways. Debug builds and test homes never register themselves.

- [ ] **Step 1: Failing tests:** `first_run_of_release_enables_login_start`, `debug_build_never_enables_it`, `later_runs_keep_the_users_choice` (first_run false + launch_at_login false gives None); core: `exists_is_false_before_the_first_save`.
- [ ] **Step 2:** Run, expect FAIL. **Step 3:** Implement + NSIS uninstall hook deleting the autostart value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. **Step 4:** Run, expect PASS. **Step 5:** Commit `M0: Start Hearth with the OS by default`.

### Task 12: Auto-update (owner request 2026-10-23)

**Files:**
- Create: `src-tauri/src/updates.rs`, `ui/src/views/Updates.svelte` (section inside Settings), `ui/src/updates.spec.ts`
- Modify: `src-tauri/Cargo.toml` (`tauri-plugin-updater = "2"`, `tauri-plugin-process = "2"`), `tauri.conf.json` (`plugins.updater.pubkey`, endpoint `https://github.com/wl-lankin/hearth/releases/latest/download/latest.json`, `windows.installMode: "passive"`, `bundle.createUpdaterArtifacts: true`), `src-tauri/src/{lib.rs, tray.rs, commands.rs}`, `ui/src/api.ts`, `ui/src/views/Settings.svelte`
- Key: `npx tauri signer generate -w %USERPROFILE%\.tauri\hearth-updater.key` with a generated password stored in `hearth-updater.key.password` next to it (same layout as Glint/Kern). The private key never enters the repo.

**Interfaces:**
- `updates.rs`: `#[serde(tag = "phase", rename_all = "camelCase")] pub enum UpdateState { Idle, Disabled, Checking, Current, Downloading { version: String, progress: f64 }, Ready { version: String }, Installing { version: String }, Error { message: String } }`; a pure `fn next_check_allowed(state: &UpdateState) -> bool` (false while Checking/Downloading/Ready/Installing); `pub fn start(app: &AppHandle)` (release builds only, else state `Disabled`): first check 10 s after start, then every 6 h; downloads in the background; emits `update-state` events and refreshes the tray.
- Commands: `update_state() -> UpdateState`, `update_check()`, `update_install_now()` (installs, then restarts Hearth).
- Tray: a "Restart to update Hearth (v<x>)" item appears when the state is Ready. Quit Hearth with a ready update installs it on the way out (IPC server shut down first).
- UI: Settings gets an "Updates" card: current version, state line ("Hearth is up to date", "Downloading 0.2.0 (45 %)", "0.2.0 is ready", "Updates are off in development builds", error text), buttons "Check now" and "Restart to update".

- [ ] **Step 1: Failing tests:** Rust `no_new_check_while_busy_or_ready`, `state_serializes_with_phase_tag` (`{"phase":"downloading","version":"0.2.0","progress":0.5}`); UI `describeUpdate(state)` gives the exact lines above.
- [ ] **Step 2:** Run, expect FAIL. **Step 3:** Implement. **Step 4:** Run, expect PASS; `cargo clippy` clean. **Step 5:** Commit `M0: Add auto-update from GitHub releases`.

### Task 13: Release workflow

**Files:**
- Create: `.github/workflows/release.yml` (adapted from Glint: tag `v*` drafts a release from `docs/releases/<tag>.md`, checks the tag against package.json, tauri.conf.json and the workspace Cargo.toml version, builds Windows NSIS + macOS universal dmg one after another with `includeUpdaterJson: true`, publishes), `docs/releases/v0.1.0.md`, `scripts/check-version.mjs` + `npm run version:check` (all three versions equal).
- Signing: updater signature via secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` (set by the owner). Azure Trusted Signing / Apple notarization steps run only when their secrets exist (added once the owner has them; M14 finishes this).

- [ ] **Step 1: Failing test:** `ui/src/version.spec.ts` imports the pure `versionsMatch({pkg, tauri, cargo})` from `scripts/version-lib.mjs`: true when equal, false and naming the odd one out otherwise.
- [ ] **Step 2-4:** Implement, run `npm test` + `npm run version:check`. **Step 5:** Commit `M0: Add release workflow`.

### Task 10: Publish (only on the owner's go)

- [ ] **Step 1:** Ask the owner. On yes: `gh repo create wl-lankin/hearth --public --source . --description "Hearth: a local PHP development environment for Windows and macOS"`, `git push -u origin main`.
- [ ] **Step 2:** Watch the first CI run with `gh run watch`; fix macOS-only failures (expected area: Unix socket path length, `run` dir permissions) in follow-up commits.

---

## Done when

- `cargo test --workspace`, `npm test`, `npm run check`, clippy and fmt are green on Windows locally and on both CI runners.
- The owner has picked the logo and seen the app run: tray, window, Dashboard shows the core running, Settings persist across restarts, `hearth status` works against the running app.
