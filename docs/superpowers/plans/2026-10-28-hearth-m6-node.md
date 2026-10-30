# Hearth M6 (Node) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** install and remove Node versions from the app and the CLI, pick a default, and have `node`, `npm`, `npx` and `corepack` run the version a project asks for in its `.nvmrc` (or `.node-version`), in every terminal, without admin rights.

**Architecture:** the signed catalog (hearth-php) gains a `node` list: the newest release of each supported major for Windows x64 and macOS arm64/x86_64, verified against nodejs.org's `SHASUMS256.txt`. Hearth gets a `node` module: versions installed under `<data>/node/<version>`, a default in `<config>/node.json`, a resolver that walks up from the working folder to the nearest `.nvmrc` / `.node-version`, and launchers built into the existing multi-name CLI binary (like `php.exe` in M4). A `<data>/node/current` link to the default version makes globally installed npm tools reachable.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (M6; Herd's nvm folder stays on the PATH until the M13 switch-over).

## Global Constraints

- Everything from M0 to M5 applies (public repos, no machine or company details, no long dashes, commits `M6: <description>`, branch `HEARTH-12` in both repos from `main`, push only on the owner's go).
- Nothing changes for a user who has not installed a Node version in Hearth: no `node`/`npm`/`npx`/`corepack`/`nvm` launchers are written into `<home>/bin` until one exists, so Herd's nvm keeps answering.
- Hearth installs only what the signed catalog lists and verifies every download (decision 5).
- No admin rights: the `current` link is a directory junction on Windows and a symlink on macOS.
- A project asking for a version that is not installed never silently gets another one: the launcher exits 1 with one line naming the file, the wanted version and `hearth node:install <x>`.

## Calls made in this plan (owner to confirm)

1. **Catalog:** the newest release of every major from 20 up (`pins.json` `node.minMajor`, bumped by hand when a line reaches end of life), plus its LTS codename. SHASUMS256.txt is fetched over TLS; its GPG signature is not checked (same ruling as MySQL in M5); the signed catalog pins the SHA-256 from then on. An entry is reused only when SHASUMS256 states the same hash, so a weekly run downloads nothing new.
2. **No auto-install** from a launcher: a missing version is an error with the install command (an `npm run dev` that silently downloads 30 MB first is a surprise).
3. **Spec formats:** `22`, `v22`, `22.12`, `v22.12.0`, `lts/*`, `lts/<codename>`, `node` / `latest` / `current`, `system` (= the default); a trailing comment, CRLF, a BOM and whitespace are ignored. The highest installed version that matches wins.
4. **`nvm` compatibility:** an `nvm` launcher answers `nvm install|uninstall|use|list|ls|current|which` with Hearth's own commands (`use` sets the default), so muscle memory from Herd keeps working.
5. **Child PATH:** a launcher puts the chosen version's folder first on the PATH of the process it starts, so `npm run` scripts that call `node`, and that version's global tools, use the same version.

## Review Focus

1. **`.nvmrc` variants** (v-prefix, partial versions, `lts/*`, codenames, CRLF, BOM, comments, an empty file, a folder without any file up to the root): the right version or a clear error. Tests in Task 3.
2. **No Hearth Node installed / last version uninstalled**: no launchers in `<home>/bin`, `current` link removed, Herd's node answers again. Tests in Task 4.
3. **`npm run` scripts that call `node`** get the project's version, not the default. Test in Task 4 (fake node build).
4. **Arguments, exit codes and Ctrl+C** pass through like `php` in M4 (shared launcher code). Test in Task 4.
5. **Uninstalling the default version**: the default moves to the newest remaining version (or none) and `current` follows. Test in Task 2.

---

## Part A: hearth-php

### Task 1: Node in the catalog

**Files:** `scripts/node.mjs`, `scripts/generate.mjs`, `lib/manifest.mjs` (validate `node[]`: `major`, `version`, `lts` (codename or null), `os`, `arch`, `url`, `sha256`, `format`, `root`), `pins.json` (`node.minMajor: 20`), `test/node.test.mjs`.

- [ ] Failing tests with a recorded `index.json` excerpt: the newest release per major ≥ minMajor; three targets per version (`win-x64.zip`, `darwin-arm64.tar.gz`, `darwin-x64.tar.gz`) with `root` = the archive folder; the hash taken from `SHASUMS256.txt`, a mismatch throws with the file name; a published entry with the same hash is reused without a download.
- [ ] Implement (through `withFallback` and `retrying` from M5), GREEN, commit `M6: Node in the catalog`.

## Part B: Hearth

### Task 2: Install, remove and default (core)

**Files:** `crates/hearth-core/src/php/manifest.rs` (`NodeBuild`, `Manifest.node` with serde default), `crates/hearth-core/src/node/{mod,install,store}.rs`, tests.

- [ ] Failing tests: install from a local fake catalog into `<data>/node/v<version>` (checksum checked, nothing left behind on a failed download); `node.json` default; installing the first version makes it the default; uninstalling the default moves it to the newest remaining one or none; `current` link created, re-pointed and removed with the default.
- [ ] Implement (download and unpack code from PHP/services, `tools_lock`), GREEN, commit `M6: Install Node versions`.

### Task 3: Which Node a folder uses

**Files:** `crates/hearth-core/src/node/resolve.rs`, tests.

- [ ] Failing tests: `parse_spec` for every format in call 3; `find_spec(cwd)` walks up to the nearest `.nvmrc` or `.node-version` (`.nvmrc` wins in the same folder) and reports the file; `resolve(installed, lts_names, spec)` picks the highest match or errors `This project wants Node 20 (<file>), which is not installed. Install it with: hearth node:install 20`; no file means the default; no default and nothing installed errors `No Node version is installed in Hearth`.
- [ ] Implement, GREEN, commit `M6: Pick the Node version a project asks for`.

### Task 4: Launchers

**Files:** `crates/hearth-cli/src/main.rs` (dispatch on the name: `node`, `npm`, `npx`, `corepack`, `nvm`), `crates/hearth-core/src/cli_install.rs` (Node names copied only while a version is installed), `crates/hearth-core/src/php/shims.rs` (sh/.bat forwarders for the Node names), `crates/hearth-core/src/path_env.rs` (`current` added after `<home>/bin` with the same switch), tests (a fake Node build: a tiny program that prints its version, its args and `PATH`, and an `npm-cli.js` stand-in run by it).

- [ ] Failing tests: `node -v` in a folder with `.nvmrc` 20 runs the 20 build; `npm` runs `node_modules/npm/bin/npm-cli.js` (Windows) / `lib/node_modules/npm/bin/npm-cli.js` (macOS) with the same node; the child's PATH starts with that version's folder; exit codes pass through; `nvm use 22` sets the default; with no version installed the Node names are absent from `bin`.
- [ ] Implement (reuse M4's run-and-wait code for Ctrl+C), GREEN, commit `M6: node, npm, npx and corepack launchers`.

### Task 5: CLI and IPC

**Files:** `crates/hearth-core/src/node/api.rs` (`node.available|list|install|uninstall|default`), `crates/hearth-core/src/ipc/mod.rs`, `crates/hearth-cli/src/main.rs` (`node:available`, `node:list`, `node:install <spec>`, `node:uninstall <version>`, `node:default <spec>`, `node:which`; `missing("node")` removed), tests in `crates/hearth-cli/tests/node.rs`.

- [ ] Failing tests per command (install resolves a spec against the catalog: `22` → newest 22.x; `node:which` prints the version and why: `.nvmrc in <folder>` or `default`); the app's API answers the same; Herd tests that expected "no Node version manager yet" updated.
- [ ] Implement, GREEN, commit `M6: Node commands`.

### Task 6: Node in the app

**Files:** `ui/src/api.ts`, `ui/src/node.svelte.ts` (+ spec), `ui/src/views/Node.svelte`, `ui/src/App.svelte` (nav), `ui/src/views/Sites.svelte` (a site's Node version, read-only, from its `.nvmrc`).

- [ ] Failing store tests: list, install progress through the existing jobs, default switch, uninstall with confirmation.
- [ ] Implement with the PHP page's layout (installed versions with the default marked, available majors with Install), screenshots in light and dark, GREEN, commit `M6: Node page`.

### Task 7: End-to-end check (test home, no admin rights)

- [ ] With the real catalog: install Node 22 and 20; default 22; a folder with `.nvmrc` `20` gets 20 for `node -v`, `npm -v` and an `npm run` script that prints `process.version`; `npm i -g` of a small tool puts it on the PATH through `current`; uninstall 20 and the folder gets the install hint; uninstall everything and Herd's `node` answers again.

## Done when

- `node`, `npm`, `npx` and `corepack` follow `.nvmrc` / `.node-version` on Windows and macOS, the default comes from the app or `hearth node:default`, global npm tools work, and a user with no Hearth Node keeps Herd's.
