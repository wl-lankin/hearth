# Hearth M1 (PHP Runtime Manager) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Install, update and remove PHP versions on Windows and macOS from the Hearth UI and the `hearth` CLI, with a global default version, `php`/`php8X`/`composer`/`laravel` shims, a generated php.ini per version plus user overrides, and per-version toggles for the `redis` and `xdebug` extensions.

**Architecture:** Two repos. `wl-lankin/hearth-php` (new, public) builds macOS PHP with static-php-cli on GitHub Actions and publishes a minisign-signed `manifest.json` listing every downloadable artifact for both OSes with SHA-256. Hearth's core gets a `php` module that reads that manifest, downloads and verifies archives, unpacks them into `<home>/bin/php/<minor>`, writes ini files and shims, and exposes everything through IPC (CLI) and in-process calls (app). Long downloads run as core jobs that frontends poll.

**Tech Stack:** Rust: reqwest (rustls, stream), sha2, zip, flate2 + tar, minisign-verify, tiny_http (dev, local download server in tests). Node 24 + node:test for the hearth-php scripts. static-php-cli (`spc`) on macos-14 (arm64) and macos-13 (x86_64) runners. Existing Hearth stack from M0.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (decision 5 covers the PHP source).

## Global Constraints

- Everything from M0's Global Constraints still applies (public repos, no machine or company details, no em/en dashes, English, `M1: <description>` commits on branch `HEARTH-2`, never push without the owner's go).
- hearth-php repo: `C:\Projects\private\hearth-php`, same noreply identity, MIT, commits `M1: <description>` on branch `HEARTH-2` (its `main` gets only an initial empty commit, like Hearth's).
- Supported PHP minors: 8.1, 8.2, 8.3, 8.4, 8.5 (plus the newest RC when the QA feed has one, marked `"channel": "rc"`). Windows: NTS x64 only. macOS: arm64 and x86_64.
- Install layout: `<home>/bin/php/<minor>/` (one patch level per minor; an update replaces it). Ini: `<home>/config/php/<minor>/php.ini` (generated, never hand-edited) and `<home>/config/php/<minor>/user.ini` (the user's, appended). State: `<home>/config/php.json` (`{"default":"8.4","extensions":{"8.4":{"redis":true,"xdebug":false}}}`).
- Manifest URL: `https://github.com/wl-lankin/hearth-php/releases/download/manifest/manifest.json` plus `manifest.json.sig` (minisign, key pair `%USERPROFILE%\.tauri\hearth-php.key` like the updater key; public key compiled into hearth-core). A manifest whose signature fails is never used.
- Every download is checked against the manifest's SHA-256 before it is unpacked; unpacking goes to a temp dir next to the target and is renamed into place (no half-installed versions). Archive paths that escape the target (`..`, absolute) are rejected.
- Herd keeps working: Hearth never touches `~/.config/herd`, and adding Hearth to PATH is opt-in (Task 12).

## Review Focus

1. **Interrupted install** (network drop, app quit mid-download, disk full): no half-installed `bin/php/<minor>`, a retry works, the temp dir is cleaned. Tests in Tasks 5 and 9.
2. **Tampered or stale manifest / archive** (bad signature, hash mismatch, unknown schema version, a zip with `../` entries): refused with a clear message, nothing written. Tests in Tasks 4 and 5.
3. **Removing the default version**: the default moves to the newest remaining version and shims are rewritten; removing the last one leaves no dangling `php` shim. Test in Task 7.
4. **Paths with spaces / non-ASCII** in the Hearth home (Windows user names): ini paths, shims and `extension_dir` must quote correctly. Tests in Tasks 6 and 8.
5. **PHP already running** from a version being updated or removed (Windows locks `php.exe`/`php-cgi.exe`): the operation fails with "close the programs using PHP 8.4 and try again" and leaves the old version intact. Test in Task 5.

---

## Part A: hearth-php repo

### Task 1: hearth-php scaffold and manifest library

**Files (new repo `C:\Projects\private\hearth-php`):** `package.json` (type module, `"test": "node --test test/*.test.mjs"`), `LICENSE`, `README.md`, `.gitignore`, `lib/manifest.mjs`, `test/manifest.test.mjs`, `schema/manifest.schema.json`.

**Interfaces:**
- Manifest schema 1:
  ```json
  {
    "schema": 1,
    "generated": "2026-10-23T12:00:00Z",
    "php": [
      { "minor": "8.4", "version": "8.4.27", "channel": "stable", "os": "windows", "arch": "x64",
        "url": "...", "sha256": "...", "size": 35295232, "format": "zip", "root": "",
        "extensions": {
          "redis":  { "version": "6.3.0", "kind": "dll", "url": "...", "sha256": "...", "file": "php_redis.dll" },
          "xdebug": { "version": "3.5.3", "kind": "zend_dll", "url": "...", "sha256": "...", "file": "php_xdebug.dll" } } },
      { "minor": "8.4", "version": "8.4.27", "channel": "stable", "os": "macos", "arch": "arm64",
        "url": "...", "sha256": "...", "size": 0, "format": "tar.gz", "root": "php-8.4.27",
        "extensions": {
          "redis":  { "kind": "builtin" },
          "xdebug": { "kind": "zend_so", "file": "lib/xdebug.so" } } }
    ],
    "composer": { "version": "2.8.12", "url": "https://getcomposer.org/download/2.8.12/composer.phar", "sha256": "..." },
    "cacert":   { "date": "2026-09-09", "url": "https://curl.se/ca/cacert-2026-09-09.pem", "sha256": "..." }
  }
  ```
  `kind`: `builtin` (compiled in), `dll` / `zend_dll` (Windows zip containing the DLL, copied into `ext/`), `zend_so` (macOS, shipped inside the PHP archive at `file`).
- `lib/manifest.mjs`: `validate(manifest) -> string[]` (errors, empty = valid), `pickLatest(feed, minors) -> {minor, version, path, sha256}[]` for the php.net feed format (`{ "8.4": { "version", "nts-vs17-x64": { "zip": { "path", "sha256", "size" } } } }`; vs16 for 8.1 to 8.3), `peclZipName(ext, extVersion, minor, vs) -> "php_redis-6.3.0-8.4-nts-vs17-x64.zip"`.

- [ ] **Step 1: Failing tests:** `validate` accepts the example above and reports `php[0].sha256 missing` / `unknown schema 2`; `pickLatest` on a fixture feed returns 8.1 to 8.5 with the vs16/vs17 keys; `peclZipName` exact strings for redis 6.3.0 / 8.4 and xdebug 3.5.3 / 8.5.
- [ ] **Step 2-4:** RED, implement, GREEN (`npm test`).
- [ ] **Step 5:** `git init -b main`, noreply identity, commit `M1: Scaffold hearth-php and the manifest library`.

### Task 2: Windows manifest generation

**Files:** `scripts/windows.mjs`, `scripts/generate.mjs`, `test/windows.test.mjs`, `pins.json` (extension versions: `{"redis":"6.3.0","xdebug":"3.5.3","composer":"2.8.12"}`, updated by hand or by Task 4's workflow).

**Interfaces:**
- `windowsEntries({ feed, qaFeed, pins, hash }) -> Promise<Entry[]>`: for each minor, the NTS x64 zip from the feed (sha256 from the feed), plus redis and xdebug from `https://downloads.php.net/~windows/pecl/releases/<ext>/<ver>/<zip>`; `hash(url)` downloads and returns the SHA-256 (PECL publishes none). A minor whose PECL zip is missing (404) gets that extension omitted, never the whole entry.
- `generate.mjs`: fetches feeds (User-Agent header: the php.net host answers HTML without one), composer sha256 from `https://getcomposer.org/download/<v>/composer.phar.sha256sum`, cacert from `https://curl.se/ca/cacert.pem` + `.sha256`, merges macOS entries from `macos-entries.json` (Task 3), writes `dist/manifest.json`, validates it, exits non-zero on any error.

- [ ] **Step 1: Failing tests** with an injected fake `hash` and fixture feeds: 5 Windows entries; a 404 PECL zip drops only that extension; RC from the QA feed gets `"channel": "rc"`.
- [ ] **Step 2-4:** RED, implement, GREEN. Then one real run: `node scripts/generate.mjs --windows-only` writes a valid manifest (check 2 hashes by hand with `certutil -hashfile`).
- [ ] **Step 5:** Commit `M1: Generate the Windows part of the manifest`.

### Task 3: macOS builds with static-php-cli

**Files:** `.github/workflows/macos.yml`, `build/extensions.txt`, `scripts/macos-entry.mjs`, `test/macos.test.mjs`.

**Interfaces:**
- `build/extensions.txt` (one line, comma-separated, what Laravel apps need plus Herd's set): `bcmath,bz2,calendar,ctype,curl,dom,exif,fileinfo,filter,ftp,gd,gmp,iconv,intl,mbstring,mysqli,mysqlnd,opcache,openssl,pcntl,pdo,pdo_mysql,pdo_pgsql,pdo_sqlite,pgsql,phar,posix,readline,redis,session,simplexml,sockets,sodium,sqlite3,tokenizer,xml,xmlreader,xmlwriter,xsl,zip,zlib`.
- Workflow (manual dispatch + called by Task 4): matrix `php: [8.1, 8.2, 8.3, 8.4, 8.5]` x `runner: [macos-14 (arm64), macos-13 (x86_64)]`; downloads the pinned `spc` binary, `spc download --with-php=<minor> --for-extensions=<list>,xdebug`, `spc build <list> --build-cli --build-fpm --build-shared=xdebug`; packs `php-<ver>/{bin/php, sbin/php-fpm, lib/xdebug.so}` as `php-<ver>-macos-<arch>.tar.gz`; smoke test in the job (`bin/php -v`, `bin/php -m` contains every listed extension, `bin/php -d zend_extension=lib/xdebug.so -v` mentions Xdebug, `sbin/php-fpm -t` with a minimal conf); uploads to release `php-<ver>` (one release per patch); writes `macos-entries.json` as an artifact.
- `macos-entry.mjs`: `macosEntry({ minor, version, arch, url, sha256, size }) -> Entry` with the builtin/zend_so extension kinds above.

- [ ] **Step 1: Failing test:** `macosEntry` output passes `validate` and has `root: "php-8.4.27"`.
- [ ] **Step 2-4:** RED, implement, GREEN.
- [ ] **Step 5:** Commit `M1: Build macOS PHP with static-php-cli`. (The workflow first runs after the owner's go to push, Task 13.)

### Task 4: Signed manifest publishing

**Files:** `.github/workflows/manifest.yml`, `scripts/sign.mjs`.

**Interfaces:** weekly schedule (Monday 05:00 UTC) + manual dispatch; job 1 calls `macos.yml` only for patch versions without a `php-<ver>` release yet; job 2 runs `generate.mjs`, signs `dist/manifest.json` with `npx tauri signer sign` (minisign format; secrets `HEARTH_PHP_SIGNING_KEY`, `HEARTH_PHP_SIGNING_KEY_PASSWORD`), uploads `manifest.json` + `manifest.json.sig` to the rolling release `manifest` with `--clobber`. Key pair generated locally like the updater key: `%USERPROFILE%\.tauri\hearth-php.key` (+ `.password`, `.pub`); the owner sets the two secrets.

- [ ] **Step 1-3:** Write workflow + script; test `sign.mjs` locally against a throwaway key (`npx tauri signer generate --ci -w <scratch>`). The resulting `manifest.json` + `.sig` pair, signed with that test key, becomes the fixture Task 5's verifier tests use, which proves both sides speak the same format.
- [ ] **Step 4:** Commit `M1: Publish a signed manifest weekly`.

## Part B: Hearth

### Task 5: Manifest client (core)

**Files:** `crates/hearth-core/src/php/{mod.rs, manifest.rs}`, tests inline; fixtures `crates/hearth-core/tests/fixtures/manifest*.json(.sig)` signed with a test key whose public key is a test constant.

**Interfaces:**
- `pub struct Manifest { schema, generated, php: Vec<PhpBuild>, composer: Artifact, cacert: CaCert }`, `PhpBuild { minor, version, channel, os, arch, url, sha256, size, format, root, extensions: BTreeMap<String, ExtensionSource> }`.
- `pub fn parse_signed(json: &[u8], sig: &[u8], public_key: &str) -> Result<Manifest, CoreError>`: verifies with `minisign-verify`, then parses; `schema != 1` is an error ("This Hearth is too old for the PHP catalog; update Hearth").
- `pub fn builds_for(m: &Manifest, os: Os, arch: Arch) -> Vec<&PhpBuild>` newest first; `Os::current()`, `Arch::current()`.
- `pub async fn fetch(paths, url) -> Result<Manifest, CoreError>`: downloads both files, verifies, caches to `<data>/php/manifest.json(.sig)`; offline falls back to the cached copy (re-verified).
- New `CoreError::Php(String)` for user-facing PHP errors.

- [ ] **Step 1: Failing tests:** good signature parses; flipped byte in json fails "signature"; schema 2 fails with the update message; `builds_for(windows, x64)` returns 5 builds newest first and skips macOS entries.
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M1: Read the signed PHP catalog`.

### Task 6: Download, verify, unpack (core)

**Files:** `crates/hearth-core/src/php/{download.rs, unpack.rs}`.

**Interfaces:**
- `pub async fn download(url, dest: &Path, sha256: &str, progress: impl FnMut(u64, Option<u64>)) -> Result<(), CoreError>`: streams to `dest.part`, hashes while writing, renames only on a match; mismatch deletes the part file ("The download of PHP 8.4.27 is damaged; try again").
- `pub fn unpack(archive: &Path, format: Format, root: &str, target: &Path) -> Result<(), CoreError>`: unpacks into `<target>.new` (sibling), strips `root`, rejects entries escaping the target, then swaps: existing `target` is renamed to `<target>.old`, `.new` to `target`, `.old` removed; a failed rename of the existing target (Windows file lock) restores it and returns `CoreError::Php("Close the programs using PHP <minor> and try again")`.

- [ ] **Step 1: Failing tests** (tiny_http server on 127.0.0.1:0 in the test): hash match keeps the file; mismatch leaves neither file nor `.part`; zip with `../evil.txt` refused, nothing written; tar.gz with `root` stripped lands at `target/bin/php`; `#[cfg(windows)]` holding an open handle on `target\php.exe` makes the swap fail with the close-programs message and `target` still intact; unicode target path works.
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M1: Download and unpack PHP safely`.

### Task 7: PHP store: installed versions, default, extensions (core)

**Files:** `crates/hearth-core/src/php/store.rs`.

**Interfaces:**
- `PhpState { default: Option<String>, extensions: BTreeMap<String, BTreeMap<String, bool>> }` in `config/php.json` (atomic write, corrupt file kept aside like settings).
- `pub fn installed(paths) -> Vec<InstalledPhp { minor, version, path }>` (version read from `<dir>/.hearth-version`, written after a successful unpack).
- `pub fn set_default(paths, minor)`, `pub fn remove(paths, minor)` (moves the default to the newest remaining minor or `None`), `pub fn set_extension(paths, minor, name, on)` (unknown extension for that build is an error).
- Defaults for a new install: `redis` on, `xdebug` off.

- [ ] **Step 1: Failing tests:** first install becomes default; removing the default picks the newest remaining; removing the last leaves `None`; extension toggles persist; unknown extension refused.
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M1: Track installed PHP versions`.

### Task 8: php.ini and shims (core)

**Files:** `crates/hearth-core/src/php/{ini.rs, shims.rs}`.

**Interfaces:**
- `pub fn render_ini(ctx: IniContext) -> String` (pure). `IniContext { os, install_dir, cacert_path, extensions_on: Vec<(name, ExtensionSource)> }`. Fixed content: `extension_dir`, the default extension list for Windows (`curl, fileinfo, gd, intl, mbstring, mysqli, openssl, pdo_mysql, pdo_pgsql, pdo_sqlite, pgsql, sodium, sqlite3, zip, exif, bz2, gmp, sockets, xsl`; macOS has them built in), `zend_extension=opcache`, `memory_limit = 512M`, `upload_max_filesize = 128M`, `post_max_size = 128M`, `max_execution_time = 300`, `curl.cainfo` and `openssl.cafile` = cacert path, `date.timezone = UTC`; `redis` as `extension=`, `xdebug` as `zend_extension=` with `xdebug.mode = debug,develop`, `xdebug.start_with_request = trigger`; finally the line `; user.ini is appended below by Hearth`. Paths always double-quoted.
- `pub fn write_ini(paths, minor, build, state)` writes `php.ini` = rendered + contents of `user.ini` (created empty with a comment on first write).
- `pub fn render_shims(os, home_bin: &Path, default: Option<&str>, installed: &[String]) -> Vec<(file_name, contents)>` (pure). Windows: `php.bat`, `php81.bat` ... per installed minor, `composer.bat`, `laravel.bat`; macOS: `php`, `php84`..., `composer`, `laravel` (sh, mode 0755). `php` points at the default; without a default no `php`/`composer`/`laravel` shim is written. Each shim passes `-c "<config>/php/<minor>/php.ini"`. Windows example: `@"%~dp0php\8.4\php.exe" -c "%~dp0..\config\php\8.4\php.ini" %*`.
- `pub fn write_shims(paths)` rewrites all shims and deletes stale ones it owns (a `rem hearth-shim` / `# hearth-shim` marker line identifies them).

- [ ] **Step 1: Failing tests:** ini for Windows 8.4 with redis on / xdebug off has `extension=redis` and no `zend_extension=xdebug`; xdebug on adds the three xdebug lines; a home `C:\Users\Jörg Müller\.config\hearth` appears double-quoted; user.ini text is appended verbatim; shims for installed [8.3, 8.4] default 8.4 produce `php.bat`, `php83.bat`, `php84.bat`, `composer.bat`, `laravel.bat` with the exact example line; no default = no `php.bat`; stale `php82.bat` with marker is deleted, a foreign `php82.bat` without marker is kept.
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M1: Generate php.ini and shims`.

### Task 9: Install service, Composer and Laravel installer, jobs (core)

**Files:** `crates/hearth-core/src/php/{service.rs, jobs.rs, composer.rs}`.

**Interfaces:**
- `pub struct Jobs` (in `CoreState`): `start(kind, label) -> JobId`, `progress(id, f64, message)`, `finish(id, Result<(), String>)`, `get(id) -> Option<JobStatus { id, kind, label, progress, message, state: running|done|failed, error }>`, `list()`. Finished jobs are kept for 10 minutes.
- `pub async fn install(core: &CoreState, minor) -> JobId`: one running install per minor (a second call returns the running job); steps: fetch manifest, pick build, download to `<data>/php/downloads/`, unpack, extension DLLs (Windows: unzip the DLL into `ext/`), write `.hearth-version`, state, ini, shims; ensure Composer + cacert (Task 9 composer.rs); delete the download. Same function updates an installed minor to the manifest's newer patch.
- `composer.rs`: `ensure_composer(core)` downloads `composer.phar` to `<bin>/composer.phar` (sha256 from manifest) and cacert to `<data>/php/cacert.pem`; `ensure_laravel_installer(core)` runs `composer global require laravel/installer` with `COMPOSER_HOME=<data>/composer` using the default PHP (job step, failure is non-fatal and reported).
- `pub fn uninstall(core, minor) -> Result<(), CoreError>` (same lock handling as Task 6), then state, shims.

- [ ] **Step 1: Failing tests** against the tiny_http server serving a fixture manifest and fake archives (a zip with `php.exe` + `ext/`; a tar.gz with `bin/php`): install produces dir + `.hearth-version` + ini + shims + composer.phar and the job ends `done`; a second concurrent install returns the same job id; a hash mismatch ends the job `failed` with the damaged-download message and no install dir; uninstall removes dir, ini dir stays (user.ini survives), shims rewritten.
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M1: Install and update PHP as background jobs`.

### Task 10: IPC methods and CLI

**Files:** `crates/hearth-core/src/ipc/mod.rs` (router), `crates/hearth-cli/src/main.rs`, `crates/hearth-cli/tests/php.rs`.

**Interfaces:**
- IPC: `php.list` -> installed + default + extensions, `php.available` -> catalog builds for this OS/arch with `installed`/`updateAvailable` flags, `php.install {minor}` -> `{jobId}`, `php.uninstall {minor}`, `php.use {minor}`, `php.extension {minor, name, on}`, `php.ini.get {minor}` -> `{generated, user}`, `php.ini.set {minor, user}`, `jobs.get {id}`. Bad params give `-32602`.
- CLI: `hearth php:list`, `hearth php:available`, `hearth php:install <minor>` (prints a progress line, polls `jobs.get` every 500 ms, exit 1 on failure), `hearth php:uninstall <minor>`, `hearth php:use <minor>`, `hearth php:ext <minor> <name> <on|off>`, and `hearth php [args...]` / `hearth composer [args...]` run the default shim's PHP directly (exit code passed through). All `--json` capable except the pass-throughs.

- [ ] **Step 1: Failing tests:** router unit tests for each method's happy path and `-32602`; CLI `php:list --json` against a served core with one fake install; `php:use 9.9` exits 1 with "PHP 9.9 is not installed".
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M1: PHP commands in the CLI`.

### Task 11: PHP view in the app

**Files:** `src-tauri/src/commands.rs` (thin commands calling the same core functions), `ui/src/api.ts`, `ui/src/php.svelte.ts` (store), `ui/src/views/Php.svelte`, `ui/src/views/PhpIni.svelte`, `ui/src/php.spec.ts`, `ui/src/App.svelte` (nav item "PHP").

**Interfaces:**
- Store `createPhpStore(api)`: `installed`, `available`, `jobs` (polled every 500 ms while any runs), `install(minor)`, `uninstall(minor)`, `use(minor)`, `toggle(minor, name)`, errors per row.
- View: one row per minor (8.5 down to 8.1, RC last with a "Preview" tag): version, state (Not installed / Installing 45 % / Installed 8.4.27 / Update to 8.4.28), Default badge or "Make default", extension switches (Redis, Xdebug), menu with "Edit php.ini" and "Remove". Remove asks inline (no `confirm()`): "Remove PHP 8.4? Your user.ini is kept." Ini editor: read-only generated part, editable user part (monospace textarea), Save.
- Filled-tile selection style only, no accent rails (owner taste).

- [ ] **Step 1: Failing tests** (store with a fake api): install shows the running job's percent and ends with the row Installed; a failed job shows its error on that row and clears on retry; removing the default moves the badge; toggling Xdebug calls `php.extension` and rolls back on error.
- [ ] **Step 2-4:** RED, implement, GREEN; screenshots light/dark at 1100x720 and 760x520 in browser demo mode (`api.ts` demo data with 8.4 installed, 8.5 available, 8.3 installing).
- [ ] **Step 5:** Commit `M1: PHP view`.

### Task 12: Use Hearth's PHP in terminals (opt-in PATH)

**Files:** `crates/hearth-core/src/path_env.rs` (pure functions + OS writers), `src-tauri/src/commands.rs`, `ui/src/views/Settings.svelte` (Terminal card).

**Interfaces:**
- Pure: `pub fn with_entry(path_value: &str, entry: &str, sep: char) -> String` (prepends once, case-insensitive on Windows), `without_entry(...)`, `pub fn shadowing(path_value, ours, sep) -> Vec<String>` (entries before ours that contain a `php` shim, e.g. Herd's bin).
- Windows writer: HKCU `Environment\Path` (REG_EXPAND_SZ preserved), then broadcast `WM_SETTINGCHANGE` "Environment". macOS: a managed block in `~/.zprofile` and `~/.bash_profile` between `# >>> hearth >>>` / `# <<< hearth <<<`.
- Settings card "Terminal": switch "Use Hearth's PHP in terminals" (off by default) and, when Herd's bin is on PATH, the note "Herd's PHP is also on your PATH. New terminals use Hearth's PHP first while this is on."

- [ ] **Step 1: Failing tests:** prepend once; remove leaves other entries untouched and keeps `%VAR%` text; Windows comparison ignores case and a trailing backslash; managed block insert/replace/remove keeps the rest of the profile byte-for-byte.
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M1: Opt-in PATH for Hearth's PHP`.

### Task 13: Publish hearth-php and end-to-end check (owner go needed)

- [ ] **Step 1:** Ask the owner; on yes: `gh repo create wl-lankin/hearth-php --public`, push, the owner sets `HEARTH_PHP_SIGNING_KEY(_PASSWORD)`; run `macos.yml` then `manifest.yml` by dispatch; check the release `manifest` has both files and the signature verifies with the public key compiled into Hearth.
- [ ] **Step 2: End-to-end (ask before running, uses a test home):** dev app, install 8.4 from the real manifest, `hearth php -v` through the shim, toggle Xdebug and see it in `php -v`, Edit php.ini adds `memory_limit=1G` and `php -i` shows it, remove 8.4. macOS: same via CI smoke in `macos.yml` plus the owner's Mac when available.
- [ ] **Step 3:** Push `HEARTH-2`, open PR, CI green.

## Done when

- Both repos green in CI; a signed manifest is live; on Windows the owner can install, switch, configure and remove PHP versions from the UI and the CLI; macOS builds pass their smoke tests on both architectures.
