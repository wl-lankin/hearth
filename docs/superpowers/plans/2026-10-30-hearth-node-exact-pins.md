# Node Versions Outside the Catalog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `hearth node:install 20.11.1` (and any other version spec the catalog cannot answer) installs that build straight from nodejs.org, checked against nodejs.org's `SHASUMS256.txt`.

**Architecture:** `node.install` first asks the signed catalog, as today. When the catalog has no match, a new `node::dist` module reads `<dist>/index.json`, picks the newest release that matches the spec and has a build for this computer, takes its hash from `<dist>/v<version>/SHASUMS256.txt`, and hands an ordinary `NodeBuild` to the existing `install::install`. Nothing after the download changes.

**Tech Stack:** Rust (hearth-core, hearth-cli), Svelte 5 UI.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (Node section) and the owner's decision of 2026-10-30: "should Hearth install versions that aren't in the catalog, by taking their hash from nodejs.org at install time?" - "yes".

## Global Constraints

- Never use em or en dashes, in code, comments, messages or docs.
- Builds from the catalog keep the catalog's signed hash. nodejs.org is asked only when the catalog has no build for the spec.
- The hash for a nodejs.org build comes from that release's `SHASUMS256.txt` over HTTPS (the same trust nvm and fnm use). A file missing from it, or a download that does not match, installs nothing.
- Targets as in the catalog: Windows x64 `win-x64` zip, macOS arm64 `darwin-arm64` tar.gz, macOS x86_64 `darwin-x64` tar.gz.
- Tests never reach the real nodejs.org: the dist URL is a `CoreState` field (default `https://nodejs.org/dist`) that tests point at the fixture server.

## Review Focus

1. A spec nodejs.org has no build of for this computer (`node:install 14` on a Mac with Apple silicon: no `darwin-arm64` before 16) says so, and does not fall back to another version.
2. nodejs.org unreachable or answering garbage: a clear error naming nodejs.org, no half-installed folder.
3. A version that is in `index.json` but whose file is missing from `SHASUMS256.txt`: error, nothing installed.
4. `lts/hydrogen` (an LTS line the catalog no longer carries) resolves through nodejs.org too, using `index.json`'s `lts` field.
5. A spec that both sources match (`22`) always takes the catalog build, even when nodejs.org has a newer patch.

---

### Task 1: `node::dist` reads nodejs.org

**Files:**
- Create: `crates/hearth-core/src/node/dist.rs`, `crates/hearth-core/src/node/dist_tests.rs`
- Modify: `crates/hearth-core/src/node/mod.rs`, `crates/hearth-core/src/ipc/mod.rs` (field `node_dist: String` on `CoreState`, default `NODE_DIST_URL = "https://nodejs.org/dist"`)

**Interfaces:**
- Produces: `pub fn pick(index: &[u8], spec: &Spec, os: Os, arch: Arch) -> Result<Option<Release>, CoreError>` with `Release { version: String, lts: Option<String> }` (newest match that lists this target's file key: `win-x64-zip`, `osx-arm64-tar`, `osx-x64-tar`); `pub fn hash_of(shasums: &str, file: &str) -> Option<String>`; `pub async fn find(dist: &str, spec: &Spec) -> Result<Option<NodeBuild>, CoreError>` building `url = <dist>/v<ver>/node-v<ver>-<suffix>.<zip|tar.gz>`, `root = node-v<ver>-<suffix>`, `major`, `lts`, `format`.

- [ ] **Step 1: failing tests** in `dist_tests.rs`: `pick_takes_the_newest_match_with_a_build_for_this_computer` (index with 20.11.1, 20.11.0, a 14.x without `osx-arm64-tar`: `20.11` gives 20.11.1, `14` gives `None` for macOS arm64 and the 14.x on Windows, `lts/iron` uses the `lts` field, `lts: false` is `None`); `hash_of_finds_the_file_line` (two-space and `*` binary-marker lines, missing file gives `None`); `garbage_index_is_an_error_naming_nodejs_org`.
- [ ] **Step 2:** run `cargo test -p hearth-core node::dist`, expect compile failure.
- [ ] **Step 3:** implement with `serde_json` (`version` string with `v`, `lts` `false` or a name, `files` list) and the existing `resolve::matches` / `compare_versions`. `find` fetches with the same HTTP client and retry as `php::download`; errors read "nodejs.org could not be read: <cause>".
- [ ] **Step 4:** tests pass.
- [ ] **Step 5:** commit "Read Node releases from nodejs.org".

### Task 2: `node.install` falls back to nodejs.org

**Files:**
- Modify: `crates/hearth-core/src/node/api.rs`, `crates/hearth-core/src/node/resolve.rs`, `crates/hearth-core/src/php/service_tests.rs` (fixture serves `/dist/index.json`, `/dist/v20.11.1/SHASUMS256.txt` and the 20.11.1 archive, plus a 20.10.0 whose archive does not match and an 18.20.4 missing from its SHASUMS), `crates/hearth-core/src/node/api_tests.rs`, `crates/hearth-core/src/node/resolve_tests.rs`

**Interfaces:**
- Consumes: `dist::find`.
- Produces: `node.install` returns `{jobId, version, source: "catalog" | "nodejs.org"}`; job label "Installing Node 20.11.1 from nodejs.org".

- [ ] **Step 1: failing tests:** `an_exact_version_outside_the_catalog_installs_from_nodejs_org` (20.11.1 installed, `source` is nodejs.org, `node.list` shows it); `the_catalog_wins_when_both_have_the_spec` (`22` gives 22.21.1 from the catalog); `a_bad_nodejs_org_build_installs_nothing` (20.10.0 mismatch and 18.20.4 missing from SHASUMS: job fails, no `v20.10.0`/`v18.20.4` folder); `no_build_anywhere_names_both_sources` ("Neither Hearth's catalog nor nodejs.org has Node 9.9.9 for this computer"); resolve's pinned hint goes back to "Install it with: hearth node:install 20.11.1".
- [ ] **Step 2:** run, expect failures.
- [ ] **Step 3:** in `start_install`, catalog first; on no match `dist::find(&core.node_dist, &spec)`; the job key stays `node.install:<version>`. Remove the "newest Node X only" branches in `api.rs` and `resolve.rs`.
- [ ] **Step 4:** `cargo test -p hearth-core node` passes.
- [ ] **Step 5:** commit "Install Node versions the catalog does not have from nodejs.org".

### Task 3: CLI and app

**Files:**
- Modify: `crates/hearth-cli/src/main.rs` (install wait prints "Node 20.11.1 is not in Hearth's catalog; downloading it from nodejs.org." before the progress), `crates/hearth-cli/tests/node.rs` if a message is asserted, `ui/src/api.ts`, `ui/src/node.svelte.ts`, `ui/src/node.spec.ts`, `ui/src/views/Node.svelte` (an "Install another version" field under the list: a spec like `20.11.1` or `lts/hydrogen`, same job flow as the row buttons, errors shown inline).

- [ ] **Step 1: failing tests:** `node.spec.ts` "installs a typed version and shows the core's error".
- [ ] **Step 2:** run `npx vitest run`, expect failure.
- [ ] **Step 3:** implement; demo data in `api.ts` answers a typed spec.
- [ ] **Step 4:** `npx vitest run`, `npm run check`, `cargo clippy --workspace --all-targets`, `cargo test -p hearth-cli --test node` pass. Screenshots of the field, light and dark.
- [ ] **Step 5:** commit "Type a Node version to install".

### Task 4: End-to-end check

Test home and hidden test app as in M6, real nodejs.org: `hearth node:install 20.11.1`, a project with `.nvmrc` `20.11.1` runs it, `node:install 22.12` gives the newest 22.12.x, `node:install lts/hydrogen` gives the newest 18. Remove all; test home deleted.
