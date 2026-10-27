# Hearth M5 (Services) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** create, version, port, start/stop and autostart MySQL, MariaDB, PostgreSQL, Redis, Meilisearch, Typesense and Silo (S3) instances from the app and the CLI, with logs and errors in the UI and a ready `.env` snippet; a port already in use (e.g. a separate MariaDB on 3306) is detected and a free one suggested.

**Architecture:** the signed catalog (hearth-php) gains a `services` list: one entry per kind, version line, OS and arch, each with a verified URL and SHA-256. Vendor builds are used where they exist and are checksummed; the gaps (Redis and MariaDB on macOS) are built from official source in hearth-php CI. Hearth gets a `services` module: instances in `<config>/services.json`, binaries installed from the catalog like Caddy, one adapter per kind (first-run init, command line, readiness, `.env` lines), run by the existing supervisor.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (M5, decision 3: all services at once).

## Sources (researched 2026-10-27, owner decisions marked)

| Kind | Windows x64 | macOS arm64 / x86_64 | Verified by |
|---|---|---|---|
| MySQL 8.4 LTS (default), 9.7 LTS | Oracle noinstall zip | Oracle tar.gz (macos15) | vendor `.md5` + PGP `.asc` checked in CI |
| MariaDB 11.8 LTS (default), 12.3 LTS | official winx64 zip | **built in hearth-php CI from official source** (owner) | vendor `sha256sums.txt`; own build hashes |
| PostgreSQL 18, 17 | theseus-rs/postgresql-binaries | same project | per-asset `.sha256` |
| Redis 8 | redis-windows/redis-windows (msys2 zip) | **built in hearth-php CI** from redis/redis source | release digest; own build hashes |
| Meilisearch | official release asset | official release assets | GitHub asset digest |
| Typesense | **not available** (needs WSL) | official tar.gz | hash pinned when the catalog is built |
| S3: **Silo** (MinIO fork, owner) | pgsty/silo zip | pgsty/silo tar.gz | `checksums.txt` |

## Global Constraints

- Everything from M0 to M4 applies (public repos, no machine or company details, no long dashes, commits `M5: <description>`, branch `HEARTH-9` in both repos from `main`, push only on the owner's go).
- Services run as the user, bound to `127.0.0.1` only, started by absolute path, never as Windows services, never with admin rights.
- A service that runs outside Hearth (a MariaDB Windows service on 3306, Herd's Redis on 6379) is never touched: its port counts as taken, and Hearth suggests the next free one.
- Data under `<data>/services/<id>/`, logs under `<logs>/services/<id>.log`; deleting an instance deletes its data only after an explicit confirmation (`--force` in the CLI).
- Default credentials are local-dev style and shown in the UI: MySQL/MariaDB `root` with no password, PostgreSQL `postgres` trust auth, Meilisearch and Typesense a generated key, Silo `hearth` / a generated secret.

## Review Focus

1. **A port taken by another program, before start or while running**: "Port 3306 is used by another program" with "Use port 3307", never a crash loop. Tests in Tasks 5 and 6.
2. **First start of a database with an empty data folder, and a start after a crash with a stale lock or pid file**: init runs once, a stale lock never blocks a start. Tests in Task 4.
3. **An instance whose version was removed from the catalog or whose download fails halfway**: the existing install keeps working; a failed download leaves nothing half-installed. Tests in Task 3.
4. **Two instances of the same kind** (MySQL 8.4 and 9.7, or two Redis): separate data, ports and logs. Test in Task 4.
5. **Quitting or crashing Hearth**: no service keeps running unless it was meant to (none are detached); databases get a clean shutdown on quit (`mysqladmin shutdown`, `pg_ctl stop -m fast`) before the supervisor kills anything left. Test in Task 6.

---

## Part A: hearth-php (catalog and builds)

### Task 1: Services in the catalog

**Files:** `lib/manifest.mjs` (validate `services[]`: `kind`, `line`, `version`, `os`, `arch`, `url`, `sha256`, `format` = zip|tar.gz|binary, `root`), `scripts/services/*.mjs` (one per source), `scripts/generate.mjs`, tests.

- [ ] Failing tests per source with recorded fixtures: MySQL entry from the CDN listing with the `.md5` checked; MariaDB from `sha256sums.txt`; PostgreSQL from `.sha256`; Redis Windows and Meilisearch from GitHub asset digests; Silo from `checksums.txt`; Typesense hashed at build time; a checksum mismatch throws with the file name.
- [ ] Implement, GREEN, commit `M5: Services in the catalog`.

### Task 2: macOS builds of Redis and MariaDB

**Files:** `.github/workflows/services-macos.yml`, `scripts/build-redis.sh`, `scripts/build-mariadb.sh`.

- [ ] Redis: `make` from the official release tarball (checksum from the release), arm64 and x86_64, `redis-server` + `redis-cli`, relocatable.
- [ ] MariaDB: CMake from the official source tarball (checksum from `sha256sums.txt`), static OpenSSL, `@loader_path` rpaths, `mariadbd`, `mariadb`, `mariadb-install-db`, `mariadb-admin`, `mariadb-dump`; arm64 and x86_64.
- [ ] Both published as release assets and fed into the manifest job; a smoke step starts each build on its runner. Commit `M5: Build Redis and MariaDB for macOS`.

## Part B: Hearth

### Task 3: Service binaries from the catalog (core)

**Files:** `crates/hearth-core/src/php/manifest.rs` (`ServiceBuild`), `crates/hearth-core/src/services/install.rs`.

**Interfaces:** `available(manifest) -> Vec<(Kind, line, version)>` for this OS/arch; `ensure_installed(core, kind, line) -> Result<PathBuf, CoreError>` (download, verify, unpack to `<data>/services/bin/<kind>/<version>/`, strip macOS quarantine); reuses an installed build without network.

- [ ] Failing tests against the M1 test server: installs once; a second call does not download; a corrupted download leaves no folder; Typesense on Windows is "not available on Windows (needs WSL)".
- [ ] Implement, GREEN, commit `M5: Install services from the catalog`.

### Task 4: Instances and adapters (core)

**Files:** `crates/hearth-core/src/services/{mod.rs, store.rs, kinds.rs}`.

**Interfaces:**
- `services.json`: `[{ id, kind, line, port, autostart, name }]`; `create(kind, line, port?, name?)`, `delete(id)`, `set_port`, `set_autostart`.
- `Kind` adapter: `default_port`, `init(dir) -> Option<Command>` (MySQL `--initialize-insecure`, MariaDB `mariadb-install-db`, PostgreSQL `initdb -A trust -U postgres -E UTF8`), `spec(instance) -> ProcessSpec`, `ready` (port), `stop_gracefully` (MySQL/MariaDB admin shutdown, `pg_ctl stop -m fast`), `env(instance) -> Vec<(String, String)>`, stale-lock cleanup (`*.pid`, `postmaster.pid` whose process is gone).

- [ ] Failing tests with fake binaries (an example that listens and records its args): init runs once; command lines per kind; a stale `postmaster.pid` is removed; two MySQL instances get separate folders; `.env` lines per kind.
- [ ] Implement, GREEN, commit `M5: Service instances and their kinds`.

### Task 5: Ports (core)

**Interfaces:** `port_free(port) -> bool` (both 127.0.0.1 and `::1`); `suggest_port(kind, taken) -> u16` (default, then +1 upward, skipping ports of other instances); creating with a taken port fails with "Port 3306 is used by another program" and the suggestion.

- [ ] Failing tests: a listener on the default makes the suggestion default+1; another instance's port is skipped; the error text.
- [ ] Implement, GREEN, commit `M5: Free ports for services`.

### Task 6: Running services (core)

**Interfaces:** `start(id)`, `stop(id)` (graceful, then supervisor), `status() -> Vec<ServiceStatus { id, running, pid, port, error, log }>`; autostart instances start with the app; quit stops all databases gracefully before `stop_all`.

- [ ] Failing tests: start runs init then the process; a taken port reports the error without restarts piling up; stop calls the graceful command; autostart on core start.
- [ ] Implement, GREEN, commit `M5: Start, stop and autostart services`.

### Task 7: IPC and CLI

**Interfaces:** IPC `services.available|list|create|delete|start|stop|port|autostart|env|log`. CLI (Herd-shaped): `services:available`, `services:versions [type]`, `services:list [--json]`, `services:create [type] [--name=] [--port=] [--service-version=]`, `services:start|stop [id]`, `services:delete [id] [--force]`, `services:clone` stays "not yet"; `start`/`stop`/`restart <service>` act on services by name.

- [ ] Failing tests: router happy paths and errors; CLI create/list/start/stop/delete against a served core with fake binaries.
- [ ] Implement, GREEN, commit `M5: Services in the CLI`.

### Task 8: Services view

**View:** nav item "Services". List of instances: kind icon, name, version, port, state dot, Start/Stop, autostart switch, menu (Logs, Copy .env, Delete). "Add a service": kind, version, port (prefilled with a free one, warning when taken), name. Error text inline with the fix ("Use port 3307"). Typesense on Windows listed as unavailable with the reason. Filled-tile selection only (owner taste).

- [ ] Failing store tests: create with a suggested port; start error shows; delete asks first; copy .env.
- [ ] Implement, GREEN, screenshots light/dark at 1100x720 and 760x520, commit `M5: Services view`.

### Task 9: End-to-end check (test home, no admin rights)

- [ ] With the real catalog: create MySQL 8.4, MariaDB 11.8 (gets 3307 because of the separate MariaDB on 3306), PostgreSQL 18, Redis, Meilisearch, Silo; connect to each (PHP PDO for the databases, `redis-cli PING`, HTTP health for Meilisearch and Silo); restart the app and see autostart; quit and confirm no service process remains and the separate MariaDB on 3306 is untouched.

## Done when

- On Windows every service except Typesense runs from the app and the CLI with its data in Hearth's home, ports never collide with programs outside Hearth, and macOS has all seven through the catalog (MariaDB and Redis from Hearth's own builds).
