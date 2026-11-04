# Hearth M11: Workspaces Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One command (or one click) gives a branch its own git worktree, served at its own `.test` host. It gets its own `.env` and `.env.testing` and, if wanted, its own database. Merging and deleting work the same way.

**Architecture:** A new `workspaces` module in hearth-core:
- runs `git` (worktree add, merge, remove);
- edits `.env` files line by line;
- creates databases through the database's own client tools: Hearth's service binaries first, then PATH;
- links the worktree with the existing site registry;
- keeps a registry in `config/workspaces.json` and a `.hearth-workspace.json` marker in each worktree.

The CLI mirrors Herd's `workspace*` commands. A Workspaces page lists, creates, opens, merges and deletes them.

**Tech Stack:** Rust (hearth-core, hearth-cli), system `git`, Svelte 5.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`, M11: "Git worktree + own `.test` domain + optional own database, copies `.env` and `.env.testing`." The MCP tool `workspaces` (M12) builds on these core methods.

## Global Constraints

- No em or en dashes. Everything in English. No attribution footers.
- Branch `HEARTH-23` from `origin/main`. PRs go to `main`.
- Tests use temp repos and temp homes, never the owner's projects. No test touches the MariaDB on port 3306.
- Paths are compared case-insensitively with `/` on Windows.

## Design calls (mine, matching Herd where it does not conflict with the spec)

- **Kind:** a real `git worktree` (the spec), not Herd's Devtools full clone.
  - Location: `<home>/data/workspaces/<parent-site>/<folder>`. That is outside parked folders, which would otherwise serve it a second time.
  - The folder is the host label, plus `-2`, `-3`, ... on collision.
- **Branch:** `name` if given, else a random `<adjective>-<animal>`. If the branch exists it is checked out, otherwise it is created from the parent's current branch (`git worktree add -b <branch> <path> <base>`). Herd's `--local` behavior is the default here: a worktree shares the repo, so unpushed parent commits are included.
- **Host:** `<label>.<parent>.test`, Herd's scheme (cookies scoped to `.parent.test` keep working). The label is the branch lowercased, other characters become `-`, runs collapse, trimmed, and empty becomes `workspace`. The whole host stays under 60 characters. Collisions get `-2`, `-3`, ...
- **Inherited from the parent:** its PHP isolation and its HTTPS, unless overridden with `--secure`/`--no-secure`.
- **Seeding:**
  - `.env` and `.env.testing` are copied from the parent when present (the spec).
  - `vendor` is copied when `composer.lock` is identical; `node_modules` when the first present JS lockfile is identical.
  - `public/build` is copied when the worktree has neither `public/build` nor `public/hot`.
- **`.env` rewrites:** in `.env` only.
  - `APP_URL` is set to the workspace URL, only if the key exists.
  - `SESSION_DOMAIN` and `SANCTUM_STATEFUL_DOMAINS` entries equal to the parent host move to the new host.
  - `DB_DATABASE` follows the database mode.
  - The first `KEY=` line is replaced; a missing key is appended. CRLF or LF is kept. Values with spaces, `#`, quotes or `$` are double-quoted.
- **Database modes:** `--database=shared|empty|clone`, with aliases `copy` (clone), `fresh` and `new` (empty) and `none` (shared). The default is `shared`, as in Herd. Every database failure falls back to shared with a note and never fails the create.
  - The name is `<parentdb>_<branch>` sanitized, max 63 characters, with `_2` on collision.
  - **MySQL/MariaDB:** `CREATE DATABASE` with the parent's charset and collation. Clone runs `mysqldump --single-transaction --routines --triggers --events --no-tablespaces --skip-comments --skip-lock-tables` piped into `mysql`.
  - **Postgres:** `CREATE DATABASE` (empty), or `TEMPLATE` with `pg_dump | psql` as fallback (clone).
  - **SQLite:** an in-repo file is copied (clone) or created empty. An absolute path outside the repo is copied into the worktree's `database/` folder.
  - **Clients:** found in the Hearth service whose port matches `DB_PORT`, else on PATH.
  - **Migrate:** `empty` runs `php artisan migrate --force` with the site's PHP unless `--no-migrate`. A failure only adds a note.
- **Delete:**
  - Asks for confirmation unless `--force`.
  - Unlinks the host and drops its own database unless `--keep-database`.
  - `git worktree remove --force` then `prune`, unless `--keep-files`.
  - The branch stays in the repository.
- **Merge:**
  - Refused with uncommitted changes.
  - First `git push . <branch>:<base>` in the parent (fast-forward); else `merge <branch> --no-edit` in a clean parent; a conflict is aborted with a clear message.
  - `--push` pushes the base to origin. `--delete` deletes afterwards.
- **Adopt:** `hearth workspace` run inside an existing linked worktree adopts it instead of creating one.
- **Not in M11:** `herd.yml` workspace hooks, env templates and `copy` lists; Herd Devtools' clone engine and its hidden commands (`workspace:provision`, `serve-host`, `candidates`); the "setup page" for unserved hosts.

## Review Focus

1. A branch name with slashes, capitals or non-ASCII letters (`feature/Ölrechnung`) gives a valid host, folder and database name, and the git branch keeps its real name.
2. `.env` files with CRLF, `export KEY=`, quoted values, comments and missing keys are edited without losing anything else.
3. A failure halfway through a create (git fails, the link fails) leaves no worktree, link, registry entry or database behind.
4. Delete on Windows works while a file in the worktree is open or read-only (git objects), and never deletes outside `data/workspaces`.
5. Merge never leaves the parent mid-merge: a conflict is aborted, a dirty parent is refused.

---

### Task 1: Names and `.env` editing

**Files:** new `crates/hearth-core/src/workspaces/{mod.rs,names.rs,env_file.rs}` + tests.

**Interfaces (produces):**
- `names::label(branch) -> String`
- `names::host(label, parent, taken: impl Fn(&str) -> bool) -> String`, returning `<label>.<parent>`
- `names::database(parent_db, branch, taken) -> String`
- `names::random_branch(taken) -> String`
- `env_file::set(text, &[(key, value)]) -> String`
- `env_file::get(text, key) -> Option<String>`

- [ ] Write the tests:
  - `label("feature/Ölrechnung")` is `"feature-lrechnung"`; `label("")` is `"workspace"`.
  - The host budget keeps the host under 60 characters.
  - Collisions get `-2`.
  - The database name is `shop_feature_x`, capped at 63 characters, with `_2` on collision.
  - `set` on a CRLF file keeps CRLF, replaces the first `APP_URL=` line, appends a missing `DB_DATABASE`, quotes `"a b"`, keeps comments and `export` lines.
  - `get` strips quotes and a trailing ` # comment`.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-23: Workspace names and .env editing".

### Task 2: Git worktrees

**Files:** `workspaces/git.rs` + tests (temp repos built with the system `git`).

**Interfaces:**
- `git::add(repo, path, branch, base) -> Result<()>`
- `git::current_branch(repo)`
- `git::is_clean(dir)`
- `git::remove(repo, path)`
- `git::merge(parent, branch, base) -> Result<MergeOutcome>`
- `git::push_origin(parent, base)`
- `git::linked_worktree_parent(dir) -> Option<PathBuf>`

- [ ] Write the tests on temp repos:
  - add with a new branch and with an existing branch;
  - the current branch;
  - merge as a fast-forward, and as a real merge when the parent moved on;
  - a conflict aborts and leaves the parent clean, with the error "The merge has conflicts: merge `main` into the workspace and resolve them there";
  - a dirty workspace is refused;
  - remove works when a file is read-only;
  - `linked_worktree_parent` finds the main repo from a worktree.
- [ ] Run and expect FAIL. Implement (`git -C`, `GIT_TERMINAL_PROMPT=0`, errors carry git's stderr), run and expect PASS. Commit "HEARTH-23: Git worktrees for workspaces".

### Task 3: Databases

**Files:** `workspaces/database.rs` + tests.

**Interfaces:**
- `Mode { Shared, Empty, Clone }` with `Mode::parse(&str)` covering the aliases
- `database::prepare(paths, worktree, parent_env, mode, branch) -> Prepared { mode, name, driver, notes }`
- `database::drop(paths, record)`

- [ ] Write the tests:
  - `Mode::parse` aliases.
  - SQLite: clone copies the parent file, empty creates an empty file, and an outside absolute path is copied into `database/` with `DB_DATABASE` pointing there.
  - An unsupported driver or an empty `DB_DATABASE` falls back to shared with a note.
  - MySQL against a Hearth-installed test MariaDB or MySQL service on a free port: empty creates `<db>_<branch>`, clone copies a table with rows, drop removes it. Skipped when no installed service binary is found (print "skipped").
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-23: Own databases for workspaces".

### Task 4: Create, list, delete, merge, adopt (core and IPC)

**Files:** `workspaces/{registry.rs,service.rs,api.rs}` + tests, `ipc/mod.rs` routing.

**IPC:**
- `workspaces.list {site?}`
- `workspaces.create {site, name?, database?, secure?, migrate?}`
- `workspaces.delete {name, keepDatabase?, keepFiles?}`
- `workspaces.merge {name, push?, delete?}`
- `workspaces.adopt {path}`
- `workspaces.find {cwd}`

Record fields follow Herd's keys in camelCase.

- [ ] Write the tests on a temp home and a temp Laravel-like git repo:
  - Create gives the folder under `data/workspaces/shop/`, a linked site `feature-x.shop`, the inherited isolation, `.env` with `APP_URL=http://feature-x.shop.test:8080`, a copied `.env.testing`, the registry record and the marker.
  - A failing link (a site with that name exists) rolls back the worktree and the registry.
  - Delete removes the link, the worktree and the record, and keeps the branch.
  - Merge brings the commit into the parent's branch.
  - Adopt of a hand-made `git worktree add` folder registers and links it.
  - A path outside `data/workspaces` is never removed by delete (a record pointing elsewhere with `keepFiles: false` uses `git worktree remove` only).
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-23: Create, merge and delete workspaces".

### Task 5: CLI

**Files:** `crates/hearth-cli/src/main.rs` + `tests/workspaces.rs`.

- [ ] Write the tests:
  - `hearth workspace feature-x` in the parent folder prints "Workspace feature-x is ready at http://feature-x.shop.test:8080" and the database line.
  - `hearth workspaces` lists it (`--json` too).
  - `hearth workspace:switch feature-x` prints the path.
  - `hearth workspace:delete feature-x --force` deletes it.
  - Run in a non-git folder, it prints "Workspaces need a git repository. Run `git init` and commit first."
  - `missing()` no longer covers `workspace`.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-23: Workspaces in the CLI".

### Task 6: Workspaces page

**Files:** `ui/src/api.ts` (types plus demo data), new `ui/src/workspaces.svelte.ts` + spec, new `ui/src/views/Workspaces.svelte`, `App.svelte` (nav "Workspaces" after "Sites").

- [ ] Write vitest tests for the store:
  - The list is grouped by parent.
  - Create sends site, name and database mode, and shows the result's notes.
  - Delete asks first, and keep-database and keep-files reach the API.
  - Merge errors show on the row.
- [ ] Run and expect FAIL. Implement the page:
  - A card per parent site with its workspaces: host link, branch, database (shared, own copy, own empty), Open, Merge, Delete.
  - "New workspace": parent site (Select), branch name, database mode (Select), and HTTPS following the parent.
  - Use the shared button styles and the Select component; no native selects.
- [ ] Run `npx vitest run` and `npm run check` and expect PASS. Take light and dark screenshots. Commit "HEARTH-23: Workspaces page".

### Task 7: Verification

- [ ] `cargo test --workspace`, clippy, `npx vitest run` and `npm run check` all pass.
- [ ] Hands-on with the owner's approval, in a test window and test home: a scratch git repo linked as a site, create a workspace from the page, open its URL, commit in it, merge, delete.
