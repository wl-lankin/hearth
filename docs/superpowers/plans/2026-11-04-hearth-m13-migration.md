# Hearth M13: Moving Over from Herd, and 1.0.0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A guided, checked and undoable move from Herd to Hearth on Windows, ending with Herd's own uninstaller, plus Hearth 1.0.0 released the way Klecks is.

**Architecture:**
- A new core module `moveover` (in `crates/hearth-core/src/moveover/`) builds on the existing site importer (`herd::read`, `herd::import`). It works in four steps:
  1. **Scan:** reads all of Herd into an `Inventory`, read-only.
  2. **Plan:** turns it into a `MovePlan` of items, each with what will happen and why.
  3. **Apply:** carries the plan out in a background job, writing every outside change into a `Record` first.
  4. **Check, Undo, Uninstall:** each works from that record.
- A wizard page in the UI drives it through IPC methods `moveover.*`.

**Tech Stack:** Rust (hearth-core, hearth-helper, src-tauri), Svelte 5, GitHub Actions with tauri-action.

**Spec:** `docs/superpowers/specs/2026-11-04-hearth-m13-migration-design.md`. The owner's machine inventory is in the git-ignored `docs/local/herd-footprint.md`.

## Global Constraints

- **Hands off:**
  - Never touch the separate MariaDB on 3306 (its client programs are only run, through forwarders).
  - Never delete Herd's files; Herd's own uninstaller does that.
- **Dry run:** read-only. It may start Herd once to record answers, and says so. It takes up to about a minute.
- **Admin rights:** one elevation prompt at most: the existing helper's one-time install. Every elevated write goes through the helper's pipe with a narrow allow-list.
- **Recovery:** every change outside Hearth's home goes into the record before it is made; Undo and a second "Move over" both work from it.
- **Moves only real sites:** parked, linked and proxied. Other hosts names are listed as "not moved"; `www.`/`*.` are not served.
- **Tests:** they use made-up Herd folders in temp homes, never the real Herd. They don't bind 80, 443, 2525, 2306, 6379 or 9003.
- **Owner rules:** Ember look, custom dropdowns, one button look, no cut-off text, no left-border rails, no em or en dashes, never kill processes by name.

## Review Focus

1. **A move interrupted halfway** (crash, reboot, a step fails): a second "Move over" continues, Undo reverts what the record says, and nothing is done twice. Pinned in Task 4.
2. **Herd's app or one of its services won't stop, or a port stays taken** (80, 443, 6379, 2525): the move stops before changing anything else and says which port. Pinned in Task 4.
3. **A site that already failed under Herd:** the baseline records the failure, so the check marks it "same as under Herd", not red. Pinned in Task 5.
4. **The uninstaller is cancelled, fails, or removes Herd's CA from the trust store:** the check runs again, and trust is restored with the existing prompt-free CurrentUser trust. Pinned in Task 5.
5. **A machine without Herd, or on macOS:** no wizard, nothing scanned, and `moveover.state` says "no Herd". Pinned in Task 1.

---

### Task 1: The inventory (scan)

**Files:**
- Create: `crates/hearth-core/src/moveover/{mod.rs,scan.rs,scan_tests.rs}`
- Modify: `crates/hearth-core/src/lib.rs`

**Interfaces (produces):**
- `pub struct HerdHome { pub root: PathBuf }` for `~/.config/herd`, with `pub fn find() -> Option<HerdHome>`. Windows only; `None` elsewhere or when the folder is missing.
- `pub fn scan(herd: &HerdHome, env: &dyn Machine) -> Result<Inventory, CoreError>`
- `pub struct Inventory`, with these fields:

  | Field | What it holds |
  |---|---|
  | `sites: HerdSetup` | the existing `herd::read` of `config/valet` |
  | `php_versions: Vec<String>` | minors from `config/php.json` keys `installed_8.x` |
  | `php_default: Option<String>` | read from `bin/php.bat`'s target path |
  | `php_ini: BTreeMap<String, Vec<IniLine>>` | per minor, from Herd's php.ini |
  | `ca: Option<(PathBuf, PathBuf)>` | `config/valet/CA/LaravelValetCASelfSigned.{pem,key}` |
  | `redis: Option<HerdRedis { port: u16, dump: Option<PathBuf> }>` | from `config/services.json`; the dump file is searched under Herd's Redis data folder |
  | `mariadb: Option<String>` | the version, listed as "not moved" |
  | `smtp_port: Option<u16>` | Herd's mail catcher port, from Herd's config (the builder finds the key) |
  | `node_versions: Vec<String>`, `node_default: Option<String>` | the `v*` folders under `bin/nvm`, and the `Program Files\nodejs` link target |
  | `user_path: Vec<String>`, `machine_path: Vec<String>`, `machine_vars: Vec<(String, String)>` | the entries that point into Herd or at its nvm link, plus `NVM_HOME` and `NVM_SYMLINK` |
  | `autostart: Option<(String, String)>` | the `HKCU\...\Run` value whose data points at Herd |
  | `mcp: Option<serde_json::Value>` | the `herd` entry of `~/.claude.json` `mcpServers` |
  | `uninstaller: Option<PathBuf>` | from the `Uninstall` registry keys whose `DisplayName` is Herd |
  | `other_hosts: Vec<String>` | names in Herd's hosts block that are no site |

- `pub trait Machine` reads PATH (user and machine), environment variables, `Run` values, `Uninstall` keys, the hosts file and the `~/.claude.json` path. A real one for Windows; tests pass a fake.
- `pub struct IniLine { pub key: String, pub value: String, pub kind: IniKind }`, where `IniKind` is one of `Setting`, `HerdPath`, `Extension(String)`.

- [ ] **Step 1: Write the failing tests** in `scan_tests.rs`. Build a fake Herd tree in a temp dir (only the files `scan` reads) and a fake `Machine`.
  - The inventory has the parked, linked and proxied sites (through `herd::read`).
  - `php_versions` are `["8.1","8.4","8.5"]`, and `php_default` is `8.5` from `bin/php.bat`.
  - The php.ini lines:
    - `memory_limit=2048M` is a `Setting`.
    - `auto_prepend_file="C:\Users\x\.config\herd\...\dump-loader.php"` is a `HerdPath`.
    - `extension="C:\Program Files\Herd\...\php_redis.dll"` is `Extension("redis")`.
    - Lines equal to PHP's stock value are left out (`display_errors` when stock).
  - Redis reads port 6379 and finds the dump file. MariaDB is listed. The SMTP port is read.
  - Node: the versions are `["v21.7.0","v22.9.0"]` and the default is `v22.9.0`.
  - PATH:
    - From the fake user PATH `C:\a;C:\Users\x\.config\herd\bin;C:\b`, only the Herd entry is listed.
    - The machine PATH entries and the `NVM_*` variables are listed.
  - Autostart, the MCP entry and the uninstaller are found.
  - `other_hosts` holds `bcd-1.api.shop.test`, not `shop.test`.
  - `HerdHome::find` gives `None` for a missing folder (Review Focus 5).
- [ ] **Step 2: Run them.** Run: `cargo test -p hearth-core --lib moveover::scan`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - The "stock" php.ini values come from the `php.ini-development` that ships with Hearth's build of the same minor. The builder checks it exists in the install folder; if not, the Hearth-generated `php.ini` is the reference.
  - Settings that differ are kept.
- [ ] **Step 4: Run.** Expected: PASS; clippy clean.
- [ ] **Step 5: Commit** `HEARTH-28: Read all of Herd`.

### Task 2: The plan, the baseline, and the dry run IPC

**Files:**
- Create: `crates/hearth-core/src/moveover/{plan.rs,plan_tests.rs,probe.rs,api.rs}`
- Modify: `crates/hearth-core/src/ipc/mod.rs`

**Interfaces (produces):**
- `pub fn plan(paths: &HearthPaths, inv: &Inventory, hearth_php: &[String]) -> MovePlan`
- `pub struct MovePlan { pub items: Vec<Item> }`
- `pub struct Item { pub area: Area, pub what: String, pub action: String, pub moved: bool, pub reason: Option<String> }`
  - `Area` is one of `Sites`, `Php`, `Https`, `Redis`, `Mail`, `MariaDb`, `Node`, `Path`, `Mcp`, `Autostart`, `Hosts`.
- `pub struct Answer { pub host: String, pub scheme: String, pub status: Option<u16>, pub location: Option<String>, pub error: Option<String> }`
- `pub async fn probe(host: &str, secure: bool, http_port: u16, https_port: u16) -> Answer`
  - A request to `127.0.0.1` with the `Host` header (HTTPS with SNI, no certificate check, no redirect following).
  - A 10 s timeout, and up to 8 sites at a time.
- IPC:
  - `moveover.state` gives `{herd: bool, moved: bool, uninstalled: bool}`.
  - `moveover.scan` gives `{plan, baseline: Answer[], startedHerd: bool}`. It starts Herd's app (`Program Files\Herd\Herd.exe`) when nothing answers on 80, waits up to 60 s, records the answers, and stores them in `<home>/config/moveover.json`.

- [ ] **Step 1: Write the failing tests** in `plan_tests.rs`. The plan, from a fake inventory, holds:
  - **Sites:** one item per park, link, proxy, isolate and secure, all `moved: true`.
  - **PHP:** an install item for each minor Hearth lacks, and a "default 8.5" item. `memory_limit` is carried; `auto_prepend_file` becomes "Hearth's dump loader"; `Extension("mongodb")` gets `moved: false` with reason `Hearth's PHP builds have no mongodb extension`.
  - **HTTPS:** "use Herd's CA", and "ports 80 and 443".
  - **Redis:** port 6379, with the dump.
  - **Mail:** port 2525.
  - **MariaDB:** `moved: false`, reason `It is not running; your other databases are not touched`.
  - **Node:** each version, plus the default.
  - **PATH:** each user entry and each machine entry, and the variables.
  - **MCP:** `herd` becomes Hearth.
  - **Autostart:** off.
  - **Hosts:** each other name gets `moved: false`, reason `Not a site (workspace or Herd's own)`.
  - Also: `probe` against a local test server on a free port, with a `Host` header, gives `status: Some(302)` and the `location`. With nothing listening, it gives an error and no panic.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.** Use the HTTP client the core already depends on, with certificate checks off for `.test` only.
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `HEARTH-28: The move's plan and Herd's answers`.

### Task 3: Changes outside Hearth's home, with their undo

**Files:**
- Create: `crates/hearth-core/src/moveover/{record.rs,outside.rs,outside_tests.rs}`
- Modify:
  - `crates/hearth-helper/src/*` and `crates/hearth-core/src/dns/protocol.rs`: a new pipe op `machine_env`.
  - `crates/hearth-cli/src/mcp/install.rs`: `global_claude_command` takes the entry name.

**Interfaces (produces):**
- `pub struct Record { pub steps: Vec<Done> }` in `<home>/config/moveover-record.json`, written atomically before each change.
  - `Done` is one of:
    - `UserPath { removed: Vec<String>, added: String }`
    - `MachineEnv { path_removed: Vec<String>, vars_removed: Vec<(String, String)> }`
    - `Autostart { name: String, value: String }`
    - `Mcp { herd_entry: Value }`
    - `Ca { previous: Option<(Vec<u8>, Vec<u8>)> }`
    - `Ports { http: u16, https: u16 }`
    - `Smtp { previous: u16 }`
    - `Imported { plan: ImportPlan }`
    - `Php { installed: Vec<String>, default_before: Option<String>, ini_before: BTreeMap<String, String> }`
    - `Redis { id: String }`
    - `Node { installed: Vec<String>, default_before: Option<String> }`
    - `Forwarders { names: Vec<String> }`
- `pub fn apply_outside(machine: &dyn MachineWrite, step: &Done) -> Result<(), CoreError>` and `pub fn undo_outside(machine: &dyn MachineWrite, step: &Done) -> Result<(), CoreError>`.
- Helper op `{"op":"machine_env","remove_path":[...],"remove_vars":[...]}` and its restore op `{"op":"machine_env_restore", ...}`.
  - The helper accepts only PATH entries inside `\.config\herd\` or equal to `C:\Program Files\nodejs`, and only the variables `NVM_HOME` and `NVM_SYMLINK`. Anything else is refused.
- MCP rename: `claude mcp remove --scope user herd`, then `claude mcp add --scope user herd -- <hearth> mcp`. Undo re-adds the recorded entry with `claude mcp add-json --scope user herd '<json>'`.

- [ ] **Step 1: Write the failing tests** in `outside_tests.rs`, with a fake `MachineWrite`.
  - The user PATH loses only the Herd entries and gains Hearth's bin; undo gives back the exact old string.
  - Autostart is removed, and undo restores it.
  - The MCP commands are the exact argument lists above.
  - The helper's `machine_env` refuses `C:\Windows\System32` and `PATHEXT`, and accepts `C:\Users\x\.config\herd\bin\nvm` and `NVM_HOME`.
  - The `Record` round-trips through JSON.
- [ ] **Step 2: Run them.** Run: `cargo test -p hearth-core --lib moveover::outside` and `cargo test -p hearth-helper`. Expected: FAIL.
- [ ] **Step 3: Implement.** The real `MachineWrite` on Windows writes `HKCU\Environment` PATH through the existing `path_env` helpers, the `Run` key through `winreg` (already used by `path_env`), MCP through `claude`, and the machine environment through the helper.
- [ ] **Step 4: Run.** Expected: PASS; clippy clean on Windows.
- [ ] **Step 5: Commit** `HEARTH-28: Outside changes with a record and their undo`.

### Task 4: The move (apply)

**Files:**
- Create: `crates/hearth-core/src/moveover/{apply.rs,apply_tests.rs,stop.rs}`
- Modify:
  - `crates/hearth-core/src/php/shims.rs`: extra forwarders from `<home>/config/forwarders.json`.
  - `crates/hearth-core/src/sites/api.rs`: an IPC method for the HTTPS port.

**Interfaces:**
- Consumes `Inventory`, `MovePlan`, `Record` and `apply_outside` from Tasks 1 to 3.
- Produces `pub fn start(core: &CoreState) -> JobId`, the job `moveover.apply`, with this order. Each step is recorded first, and steps already in the record are skipped.
  1. **Stop Herd:** ask its Redis to `SAVE` (through its `redis-cli` on its port), then stop every process whose executable lies under `C:\Program Files\Herd` or `~/.config/herd` (by path, never by name; `WM_CLOSE`, then terminate after 10 s). Wait until 80, 443, the Redis port and the SMTP port are free; otherwise stop with `Port <p> is still in use by <program>` (Review Focus 2).
  2. **Autostart off.**
  3. **Sites:** `herd::import::apply`.
  4. **HTTPS:** copy Herd's CA to `ca.crt`/`ca.key` (the previous CA goes in the record), remove leaf certificates so they are reissued from it, and set the ports to 80 and 443.
  5. **PHP:** install the missing minors (waiting for their jobs), set the default, merge the carried ini lines into each `user.ini`, and switch on the mapped extensions.
  6. **Redis:** create the service on Herd's port with `autostart: true`, copy `dump.rdb` into its data folder before the first start, then start it.
  7. **Mail:** set the SMTP port.
  8. **Node:** install the versions, then set the default.
  9. **Forwarders:**
     - `mysql`, `mysqldump` and `mariadb` point at the client programs next to the `mariadbd.exe` of the Windows service that listens on 3306, found through its service configuration.
     - `redis-cli` points at Hearth's Redis.
  10. **PATH, machine environment, MCP:** through Task 3.
  11. **Finish:** `moved: true` in `moveover.json`, then web sync.
- IPC `moveover.apply` gives `{jobId}`.

- [ ] **Step 1: Write the failing tests** in `apply_tests.rs`, on a temp home and a fake Herd, with a fake `MachineWrite` and fake process control.
  - All steps run in order, and the record holds them.
  - A second run after a simulated crash after step 4 skips steps 1 to 4 and finishes (Review Focus 1).
  - A port that stays taken stops the move after step 1 with the port named, and changes nothing else.
  - The forwarders file lists the client paths, and `render_shims` writes `mysql.bat` and the others as forwarders.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Run: `cargo test -p hearth-core --lib moveover` and the existing `sites`, `php` and `services` tests. Expected: PASS.
- [ ] **Step 5: Commit** `HEARTH-28: Move over from Herd`.

### Task 5: Check, Undo and the uninstall

**Files:**
- Create: `crates/hearth-core/src/moveover/{check.rs,undo.rs,uninstall.rs,check_tests.rs}`
- Modify: `crates/hearth-core/src/moveover/api.rs`

**Interfaces (produces):**
- `pub fn compare(before: &Answer, after: &Answer) -> Verdict`, where `Verdict` is one of `Same`, `Better`, `Changed(String)`, `Broken(String)`.
  - `Same` when the status class and the redirect target match.
  - `Same` too when Herd failed the same way (Review Focus 3).
  - `Better` when Herd failed and Hearth answers.
- IPC methods:
  - `moveover.check` gives `{sites: [{host, scheme, verdict}], services: [{name, running}], node: {from_hearth: bool}}`.
  - `moveover.undo` is a job that walks the record backwards. It is refused once `uninstalled` is true.
  - `moveover.uninstallHerd` is refused unless the last check had no `Broken` and no `Changed` (or the owner accepted each one through `moveover.accept {host}`). It starts the uninstaller, waits for it, re-runs the check, and re-trusts the CA when `is_trusted` is false. It gives `{exitCode, check}`.

- [ ] **Step 1: Write the failing tests** in `check_tests.rs`.
  - `compare`:
    - 200 and 200 give `Same`.
    - 302 to `/login` and 302 to `/login` give `Same`.
    - 302 to `/login` and 302 to `/` give `Changed`.
    - 200 and a connection error give `Broken`.
    - An error and the same error give `Same`.
    - An error and 200 give `Better`.
  - Undo of a full fake record calls every `undo_outside` in reverse order, and restores the previous CA, ports, SMTP port and PHP default.
  - The uninstall is refused with an unaccepted `Changed`.
  - After a fake uninstaller that removes trust, the CA is trusted again (Review Focus 4).
  - A cancelled uninstaller (exit code 1) leaves `uninstalled: false` and Undo still possible.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `HEARTH-28: Check, undo and uninstall Herd`.

### Task 6: The wizard

**Files:**
- Create: `ui/src/views/MoveOver.svelte`, `ui/src/moveover.svelte.ts`, `ui/src/moveover.spec.ts`
- Modify:
  - `ui/src/api.ts`: the `moveover.*` calls, with demo data.
  - `ui/src/nav.ts`: a hidden view `moveover` that opens from Home and Settings, without a rail entry.
  - `ui/src/views/Home.svelte`: a banner "Move over from Herd" when `herd && !moved`.
  - `ui/src/views/Settings.svelte`: a "Herd" section.

The wizard has five screens, in one Ember page with a step indicator:

1. **Dry run:** the plan grouped by area. "Not moved" rows are muted with their reason. If the dry run started Herd, a note says so. The button is "Move over".
2. **Moving:** job progress with the current step.
3. **Check:** green and red rows. Each `Changed` or `Broken` row has "Accept". The buttons are "Undo" and "Uninstall Herd" (enabled once everything is green or accepted).
4. **Uninstalling:** a waiting state, then the second check.
5. **Done:** "Herd is gone. One step left: Claude updates your CLAUDE.md and skills to Hearth's paths in a session."

- [ ] **Step 1: Write the failing tests** in `moveover.spec.ts` (jsdom, fake api):
  - each screen renders from its state;
  - "Uninstall Herd" is disabled with a red row and enabled after "Accept";
  - "Undo" is hidden after the uninstall;
  - the Home banner shows for `herd: true, moved: false` only.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Run: `npx vitest run`, `npm run check` and the demo screenshots at 1100x720 and 760x520. Expected: PASS, nothing cut off.
- [ ] **Step 5: Commit** `HEARTH-28: The move-over wizard`.

### Task 7: Hearth 1.0.0, released like Klecks

**Files:**
- Create: `CHANGELOG.md` (a `## 1.0.0` section), `.github/release-notes.sh` (Klecks' script with "Hearth"), `docs/RELEASE.md`
- Modify:
  - `.github/workflows/release.yml`:
    - notes from `release-notes.sh`, replacing the `docs/releases/<tag>.md` requirement;
    - the release stays a draft (the publish step goes away);
    - fixed names `Hearth-Windows-Setup.exe` and `Hearth-macOS.dmg`;
    - `prerelease` when the tag has a hyphen.
  - Version 1.0.0 in `package.json`, `src-tauri/tauri.conf.json` and the root `Cargo.toml`.
- Delete: `docs/releases/`

`docs/RELEASE.md` follows Klecks' structure, written for Hearth in English: how updates reach installed copies, the parts, the files in a release, the fixed links, where the updater key lives, and the steps for a release.

- [ ] **Step 1:** `node scripts/check-version.mjs 1.0.0` FAILS before the bump and passes after it.
- [ ] **Step 2:** `bash .github/release-notes.sh v1.0.0` prints the `## 1.0.0` section.
- [ ] **Step 3: Validate the workflow:** `actionlint` if available, else a YAML parse. The fixed-name step names exactly `Hearth-Windows-Setup.exe` and `Hearth-macOS.dmg`.
- [ ] **Step 4: Commit** `HEARTH-28: Hearth 1.0.0, released like Klecks`.

### Task 8: The switch-over (with the owner)

- [ ] **Step 1: Release.** Merge (on the owner's word), tag `v1.0.0` and wait for the Release run. The owner checks the draft and publishes it.
- [ ] **Step 2: Install.** The owner installs `Hearth-Windows-Setup.exe`. SmartScreen warns once, since the installer is unsigned.
- [ ] **Step 3: Move.** In the installed Hearth: the dry run, the move, then the check. Fix and re-release only if something is red that can't be accepted.
- [ ] **Step 4: Uninstall.** Herd's uninstaller runs, then the second check, which must pass.
- [ ] **Step 5: The owner's files.** In this session, Claude updates `~/.claude/CLAUDE.md`, the skills that name `.config\herd\bin` and the memory notes to `.config\hearth\bin`.
