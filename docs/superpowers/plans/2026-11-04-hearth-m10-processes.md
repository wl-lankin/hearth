# Hearth M10: Processes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run a site's long-running dev processes (Vite, a queue worker, Pail, `composer run dev`) from Hearth, with live output, start, stop and restart.

**Architecture:**
- A process is a shell command saved per site in `config/processes.json`. The existing supervisor runs it in the site's folder. `PATH` starts with Hearth's `bin`, so `php`, `composer`, `node` and `npm` resolve to the site's own versions through the shims.
- Output goes to a log file per process. The UI reads new output from an offset, with ANSI codes stripped.
- On Windows each supervised child gets its own Job Object, nested in Hearth's. Stopping a process then ends its whole tree, such as the Node behind `npm run dev`.

**Tech Stack:** Rust (hearth-core, hearth-cli), Svelte 5.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`, M10: "Long-running dev processes per site (`artisan dev`, Vite, queue, Pail) with live output and restart." It also lists `processes` as an MCP tool for M12, so every action is a core method.

## Global Constraints

- No em or en dashes anywhere. Everything in English. No attribution footers.
- Branch `HEARTH-22`, stacked on `HEARTH-21` (PRs #15 and #16 are not merged yet).
- Only Hearth's own children are ever stopped, never processes by name.
- Tests never bind ports 2525, 2306 or 9003. Tests that need a real PHP use `HEARTH_TEST_PHP` and print "skipped" without it.

## Design calls (mine, the owner can overrule)

- **One list per site:** `{ name, command, autostart }`. The name is unique per site, `[a-z0-9-]{1,32}`.
- **Suggestions from the project, offered and never added on their own:**
  - `package.json` with a `dev` script: `vite`, `npm run dev`
  - `composer.json` with a `dev` script: `dev`, `composer run dev`
  - An `artisan` file: `queue`, `php artisan queue:listen --tries=1` (picks up code changes) and `schedule`, `php artisan schedule:work`
  - `composer.lock` naming `laravel/pail`: `pail`, `php artisan pail`
  - `laravel/reverb`: `reverb`, `php artisan reverb:start`
  - `laravel/horizon`: `horizon`, `php artisan horizon`
- **Shell:** `cmd /d /s /c "<command>"` on Windows, `/bin/sh -c '<command>'` on macOS.
- **Environment:** `NO_COLOR=1` and `FORCE_COLOR=0` cut most color codes; whatever is left is stripped when the log is read.
- **Crashes:** a crashed process restarts with the supervisor's back-off (1 to 30 s), and the page shows the restart count and last error.
- **Log:** `logs/processes/<site>/<name>.log`, emptied on each manual start or restart. A read returns at most the last 256 KB.
- **Autostart** processes start with Hearth and stop with it. Unlinking or unparking a site stops its processes and forgets them.
- **CLI:** `hearth processes` (list), `processes:start|stop|restart <site> <name>`. The "process manager" line leaves `missing()`.

## Review Focus

1. Stopping a process whose command started grandchildren (`npm run dev` starting node, `composer run dev` starting four) leaves none running, on Windows and macOS.
2. A command that exits at once (typo, missing `node_modules`) shows its error output and a stopped or restarting state. It never shows as running with no output.
3. A log read with an offset beyond the file (log emptied by a restart) starts again from 0 and never errors.
4. Names and commands with quotes, `&&` or non-ASCII characters run as the user typed them, and a bad name is refused with a reason.
5. A site that is removed while its process runs leaves no running child and no entry in `processes.json`.

---

### Task 1: Every child stops with its tree (supervisor)

**Files:** `crates/hearth-core/src/supervisor.rs`, its tests.

- [ ] Write the test `stopping_a_shell_stops_what_it_started`, which needs a real PHP:
  - `ensure` a spec that runs the platform shell with `<php> -S 127.0.0.1:<free port> -t <tmp>`, wait until the port answers, then `stop(key)`.
  - Expect the port to stop answering within 2 s.
- [ ] Run it and expect FAIL on Windows: the shell dies, PHP keeps the port.
- [ ] Implement:
  - Windows: each spawned child is assigned to Hearth's job first, then to its own new job (a nested job, Windows 8+). The entry keeps that job. `terminate` calls `TerminateJobObject` on it, then waits for the child.
  - macOS keeps process groups.
- [ ] Run the test and the existing supervisor tests and expect PASS. Commit "HEARTH-22: Stop a child together with what it started".

### Task 2: Process definitions and suggestions

**Files:** new `crates/hearth-core/src/processes/{mod.rs,store.rs,suggest.rs}` and tests.

**Interfaces (produces):**
- `Proc { name, command, autostart }`
- `store::load(paths) -> BTreeMap<String, Vec<Proc>>`
- `store::save`
- `store::add(paths, site, Proc) -> Result<()>`, which refuses a bad or duplicate name and an empty command
- `store::remove(paths, site, name)`
- `store::forget_site(paths, site)`
- `suggest::for_dir(dir) -> Vec<Proc>`

- [ ] Write the tests:
  - add, list, remove and duplicate refusal; `"Vite"` and `"a b"` are refused with "Use 1 to 32 lowercase letters, digits or dashes".
  - The suggestions for a temp Laravel folder (artisan, package.json dev, composer.lock with laravel/pail) come in the order of the design calls; an empty folder gets none.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-22: Keep processes per site and suggest them from the project".

### Task 3: Run, stop, restart, read output

**Files:** `processes/run.rs`, `processes/output.rs`, `processes/api.rs` (+ tests), the `ipc/mod.rs` routing, `sites/api.rs` (unlink and unpark clean up), `src-tauri/src/lib.rs` (autostart in `start_core`).

**Interfaces (produces):**
- IPC `processes.list` returns `{sites: [{site, processes: [{name, command, autostart, running, pid, restarts, lastError}]}]}`
- `processes.add {site,name,command,autostart}`
- `processes.remove {site,name}`
- `processes.start | stop | restart {site,name}`
- `processes.output {site,name,offset?}` returns `{offset, text, reset}`
- `processes.suggest {site}` returns `[Proc]`

- [ ] Write the tests:
  - `spec_for` builds the shell, cwd, `PATH` with `<home>/bin` first and the log path. A command with quotes and `&&` is passed as one argument after `/c` or `-c`.
  - `output::read` handles: no offset (the last 256 KB), an offset (only new text), an offset past the end (reset true, from 0), ANSI stripped (`"\x1b[32mready\x1b[0m"` becomes `"ready"`), and invalid UTF-8 replaced.
  - With a real PHP: `processes.start` of `php -r "echo 'hi'; sleep(30);"` runs and `processes.output` contains `hi`. `restart` empties the log. `stop` ends it.
  - A command that exits at once shows `running: false` or a `lastError`, and its output holds the shell's error.
  - `sites.unlink` stops and forgets the site's processes.
- [ ] Run and expect FAIL. Implement. Supervisor key `proc:<site>:<name>`. `restart` is stop, empty the log, ensure. Autostart runs in `start_core` after the web sync is scheduled. Run and expect PASS. Commit "HEARTH-22: Run a site's processes and read their output".

### Task 4: CLI

**Files:** `crates/hearth-cli/src/main.rs` and the CLI tests.

- [ ] Write the tests:
  - `hearth processes` against a test home prints `site  name  running|stopped  command`.
  - `processes:start shop nope` fails with "shop has no process named nope".
  - `missing("processes:list")` is no longer the "process manager" message.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-22: Processes in the CLI".

### Task 5: Processes page

**Files:** `ui/src/api.ts` (types plus demo data: shop with vite running and queue stopped), new `ui/src/processes.svelte.ts` + `processes.spec.ts`, new `ui/src/views/Processes.svelte`, `ui/src/App.svelte` (nav "Processes" after "Services").

- [ ] Write vitest tests for the store:
  - Picking a process loads its output, and later polls append only new text.
  - A reset replaces the text.
  - Output is capped at 2000 lines in memory.
  - Start, stop and restart call the API and refresh the status; a refused action shows its reason on that row.
  - Add uses a suggestion's name and command; an add the core refuses keeps the form filled.
- [ ] Run and expect FAIL. Implement the page:
  - Left: sites with their processes (status dot, restarts, Start/Stop/Restart).
  - Right: the output in monospace, sticking to the bottom unless scrolled up.
  - "Add a process": site select, suggestion chips, name, command and an autostart checkbox.
  - Remove sits under the output.
- [ ] Run `npx vitest run` and `npm run check` and expect PASS. Take light and dark screenshots in demo mode. Commit "HEARTH-22: Processes page".

### Task 6: Verification

- [ ] `cargo test --workspace` (with `HEARTH_TEST_PHP`), clippy, `npx vitest run` and `npm run check` all pass.
- [ ] Hands-on in a test home, with the owner's approval because a window opens:
  - Link a scratch folder that has a `package.json` whose `dev` script starts a PHP built-in server on a free port through `npm run dev`.
  - Start it from the page, see its output, restart it, stop it.
  - Check that the port is free after Stop.
