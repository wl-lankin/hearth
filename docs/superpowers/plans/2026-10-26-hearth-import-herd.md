# Hearth: import sites from Herd (pulled forward from M13)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** `hearth import:herd` copies Herd's parked folders, linked sites (with their aliases), proxies, PHP isolations and HTTPS choices into Hearth, read-only on Herd's side, with a dry run first.

**Architecture:** a `herd` module in hearth-core reads Herd's Valet folder into a `HerdSetup`, compares it with Hearth's registry into an `ImportPlan` (what would be added, what is skipped and why), and applies it through the existing registry functions. One IPC method, one CLI command. Herd keeps running; nothing of Herd is changed.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (M13 "Migration from Herd", the "Compatibility with an existing Herd install" section). The owner decided on 2026-10-26 to pull the sites part forward.

## Global Constraints

- Everything from M0 to M3 applies (public repo, no machine or company details in code, tests or docs; no long dashes; commits `HEARTH-7: <description>`; branch `HEARTH-7` from `main`; push only on the owner's go).
- Herd is only read: no file in Herd's folder is written, renamed or deleted.
- Not in scope (stays in M13): Herd's CA and certificates, the hosts block, Redis data, shims, PATH, MCP, switching ports.
- Herd's folder: Windows `~/.config/herd/config/valet`; macOS `~/Library/Application Support/Herd/config/valet`; `--from <dir>` overrides.

## Review Focus

1. **A link whose folder no longer exists, a parked path that is gone, or a name Hearth cannot use** (uppercase like `Passify`, characters outside `[a-z0-9.-]`): lowercase what can be lowercased, skip the rest with a reason, never fail the whole import.
2. **Names already in Hearth** (a link or proxy of the same name): skipped with "already in Hearth", Hearth's entry is kept.
3. **Isolation written as `81`, `8.1` or junk**: `81` -> `8.1`, junk skipped with a reason.
4. **Running the import twice**: the second run adds nothing and reports everything as already there.
5. **Herd's own `Sites` folder in its parked paths**: not parked in Hearth (its entries arrive as links).

---

### Task 1: Read Herd's setup (core)

**Files:** `crates/hearth-core/src/herd/{mod.rs, read.rs}`.

**Interfaces:** `default_valet_dir() -> Option<PathBuf>`; `read(valet: &Path) -> Result<HerdSetup, CoreError>` with `HerdSetup { parked: Vec<PathBuf>, links: BTreeMap<String, PathBuf>, proxies: BTreeMap<String, String>, isolated: BTreeMap<String, String>, secured: BTreeSet<String>, skipped: Vec<Skipped { name, reason }> }`. Parked = `config.json` `paths` minus `<valet>/Sites`. Links = entries of `<valet>/Sites` that are links (`read_link`). From `<valet>/Nginx/<name>.test.conf`: a `proxy_pass <url>` makes `<name>` a proxy (trailing `/` dropped); a first-line `# ISOLATED_PHP_VERSION=<v>` an isolation; a `listen ...:443 ssl` a secured site.

- [ ] Failing tests on a temp Valet folder (links made as junctions when symlinks need privileges): parked list without the Sites folder; two links incl. an alias to the same folder; a proxy from a conf; isolations `8.3` and `81` -> `8.1`, `x` skipped; secured names; `Passify` -> `passify`; a missing `config.json` -> "This is not a Herd folder".
- [ ] Implement, GREEN, commit `HEARTH-7: Read Herd's sites`.

### Task 2: Plan and apply the import (core)

**Files:** `crates/hearth-core/src/herd/import.rs`.

**Interfaces:** `plan(paths, &HerdSetup) -> ImportPlan { park: Vec<PathBuf>, link: Vec<(String, PathBuf)>, proxy: Vec<(String, String)>, isolate: Vec<(String, String)>, secure: Vec<String>, skipped: Vec<Skipped> }` (nothing written); `apply(paths, &ImportPlan) -> Result<(), CoreError>` through the registry (park, link, proxy, isolate, secure in that order). Skips: link/proxy name already in Hearth ("already in Hearth"), link folder missing ("Folder missing"), parked path missing ("Folder missing"), already parked ("already parked"), already isolated/secured the same way (left out, no skip line).

- [ ] Failing tests: a plan from a fresh Hearth lists everything and writes nothing; apply then list shows the sites with PHP and `secured`; a second plan is empty with skips "already in Hearth"/"already parked"; a Hearth link with a Herd name is kept.
- [ ] Implement, GREEN, commit `HEARTH-7: Plan and apply a Herd import`.

### Task 3: IPC and CLI

**Interfaces:** IPC `herd.import { from?: string, apply: bool }` -> the plan as JSON (applied when `apply`, then a web sync). CLI `hearth import:herd [--from <dir>] [--apply] [--json]`: without `--apply` it prints what would happen and how to apply; with it, what was done.

- [ ] Failing tests: router dry run returns the plan and leaves `sites.json` untouched; apply adds the sites; CLI dry run text ends with "Run `hearth import:herd --apply` to import."; a non-Herd folder gives exit 1 with the message.
- [ ] Implement, GREEN, commit `HEARTH-7: Import Herd sites from the CLI`.

## Done when

- `hearth import:herd` on the owner's machine shows all Herd sites, proxies, isolations and HTTPS choices, `--apply` brings them into Hearth (served on 8080/8443 next to Herd), and a second run adds nothing.
