# Hearth M12b: env, db and site error Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The MCP actions M12a left as "not yet" work: `env` (get, set, unset, check, use_service), `db` (connection, list, create, use, drop) and `site error`.

**Architecture:**
- New core modules `env` and `db` with IPC methods. The MCP actions call them like the others.
- `env` edits the site's `.env` with M11's line editor (`workspaces::env_file`). It moves to a shared place (`crate::dotenv`) and is re-exported from the old path.
- `db` reuses M11's connection parsing and client programs, which move from `workspaces::database` into `crate::db`, keeping the workspace behaviour.
- `site error` reads the newest exception from the site's Laravel log through the existing log parser.

**Tech Stack:** Rust (hearth-core, hearth-cli).

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (M12: the Herd MCP tool surface). Herd's behaviour comes from its tool docs (scratchpad `herd-tools-list.json`) and the review notes of M12a.

## Global Constraints

- No em or en dashes. English. No attribution footers.
- Branch `HEARTH-25`, stacked on `HEARTH-24` (PR #19 waits for CI).
- Tests never touch the MariaDB on 3306. The database tests use a throwaway server (`HEARTH_TEST_MYSQLD`) and are skipped without one.
- `.env` writes keep comments, order, line endings and quoting (M11's editor), and are atomic.

## Design calls (mine)

- **Site folder:** a site's `.env` is `<site path>/.env`, the project root. Proxies have none, and say so.
- **`env.check`:** keys in `.env.example` missing from `.env`, and keys in `.env` that are not in `.env.example`. Both sorted.
- **`env.useService`:**

  | category | selection | `.env` changes |
  |---|---|---|
  | database | `sqlite` | `DB_CONNECTION=sqlite` |
  | database | a Hearth MySQL/MariaDB/PostgreSQL service (id, name or type) | its `DB_*` lines; `DB_DATABASE` is kept |
  | queue | `sync`, `database` | `QUEUE_CONNECTION` |
  | queue | a Redis service | `QUEUE_CONNECTION=redis` + `REDIS_*` |
  | cache | `file`, `database`, `array` | `CACHE_STORE` (and `CACHE_DRIVER` when the file has it) |
  | cache | a Redis service | `CACHE_STORE=redis` + `REDIS_*` |
  | broadcasting | `log`, `null`, `reverb`, `pusher` | `BROADCAST_CONNECTION` (and `BROADCAST_DRIVER` when present) |

  - The first change to a category saves the values it replaces in `config/env-originals.json`, keyed by site and category.
  - `unmanaged` writes them back (removing keys that were not there) and forgets them.
- **`db`:**
  - `connection` reads `.env` (password included, as Herd's does).
  - `list` uses `SHOW DATABASES` / `pg_database` without system schemas.
  - `create` defaults to `DB_DATABASE`.
  - `use` sets `DB_DATABASE`, with `create` first.
  - `drop` refuses the database the site's `.env` uses.
  - SQLite: `create` makes the file and `list` shows the site's `database/*.sqlite`.
- **`site error`:** the newest log entry whose detail has a stack trace or whose message names an exception. Its message, class, file and line come from Laravel's `(Class(code: N): message at file:line)` form, with the stack trace in `detail`. "No exception in the log" otherwise.

## Review Focus

1. `env set` of a key that appears twice, or as `export KEY`, changes every line (M11's editor), and a value with spaces, `#` or `$` round-trips through `env get`.
2. `use_service` then `unmanaged` gives back the exact previous `.env`: removed keys stay removed, nothing extra stays behind.
3. `db drop` never drops the site's current database, also when the name is given with other case or quotes.
4. `db` on a site whose `.env` has no `DB_CONNECTION` (the SQLite default) and on an unsupported driver gives a clear sentence, not a client error.
5. `site error` on a log without exceptions, a missing log, or a huge log returns quickly with a clear answer.

---

### Task 1: The `.env` methods

**Files:**
- Move `workspaces/env_file.rs` to `crates/hearth-core/src/dotenv.rs`, re-exported as `workspaces::env_file`.
- New `crates/hearth-core/src/env/{mod.rs,api.rs}` + tests.
- `ipc/mod.rs` routing.

**IPC:**
- `env.get {site, key?}` returns `{values: {K: V}}`, or `{key, value}` for one key.
- `env.set {site, key, value}`
- `env.unset {site, key}`
- `env.check {site}` returns `{missing:[], extra:[]}`
- `env.useService {site, category, selection}` returns `{changed: {K: V|null}}`

- [ ] Write the tests on a temp home with a linked site and `.env`/`.env.example`:
  - get (all and one);
  - set (quoting, duplicates, CRLF kept);
  - unset;
  - check;
  - useService with database to a fake MySQL service record (`services::store`), queue to sync, cache to a Redis record, then `unmanaged` gives back the original bytes;
  - a proxy and an unknown site are refused.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-25: Read and change a site's .env".

### Task 2: The database methods

**Files:**
- New `crates/hearth-core/src/db/{mod.rs,api.rs}` with M11's `Driver`/`Conn`/client helpers moved in (`workspaces::database` calls them).
- Tests.

**IPC:**
- `db.connection {site}`
- `db.list {site}`
- `db.create {site, name?}`
- `db.use {site, name, create?}`
- `db.drop {site, name}`

- [ ] Write the tests:
  - connection from `.env`;
  - an unsupported driver and a missing `DB_CONNECTION` (SQLite) give clear answers;
  - SQLite create makes `database/<name>.sqlite`;
  - with `HEARTH_TEST_MYSQLD`: list shows a created database; use switches `DB_DATABASE` (with create); drop removes another database and refuses the current one (any case, quoted).
  - The M11 workspace database tests still pass.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-25: Databases of a site's server".

### Task 3: `site error`

**Files:** `crates/hearth-core/src/insights/logs.rs` (`last_exception(paths, site) -> Option<Exception>`) and the IPC `logs.lastException {site}`, + tests.

- [ ] Write the tests:
  - A Laravel log with an INFO entry, then an ERROR with `(DivisionByZeroError(code: 0): Division by zero at C:\x\app\Pricing.php:18)` and a `[stacktrace]`, then a later WARNING. The answer is the ERROR, with class, message, file, line and trace.
  - A log without exceptions answers `null`.
  - A 50 MB log answers within 2 s (only its tail is read).
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-25: The last exception in a site's log".

### Task 4: The MCP actions

**Files:** `crates/hearth-cli/src/mcp/actions.rs`, `tests/mcp.rs`.

- [ ] Write the tests:
  - `env get`/`set`/`check`/`use_service` (with `unmanaged`), `db connection`, and `site error` through the server, against a live core.
  - The instructions mention `env`/`db` again.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-25: env, db and site error for agents".

### Task 5: Verification

- [ ] `cargo test --workspace` (with `HEARTH_TEST_PHP` and `HEARTH_TEST_MYSQLD`), clippy `-D warnings`, `npx vitest run`.
