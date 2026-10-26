# Hearth - design and milestones

Date: 2026-10-23
Status: draft, waiting for owner approval

## Intent

Laravel Herd Pro costs money every year, and its newest releases add things the owner does not want (Studio, AI features). Hearth is the owner's own replacement: a local PHP development environment for Windows and macOS that does everything Herd (Free + Pro) does today, minus Studio and AI.

Success = the owner uninstalls Herd, and the daily work in their Laravel projects runs on Hearth unchanged: same `.test` domains, same databases, same TLS, same CLI habits, same MCP tools for Claude Code.

## Goals

- Full feature parity with Herd Pro (without Studio and AI), on Windows and macOS.
- Drop-in migration from an existing Herd install: sites, proxies, PHP versions, certificates, MariaDB/Redis data, without data loss.
- A `hearth` CLI that accepts the Herd command shapes the owner uses (`park`, `link`, `secure`, `proxy`, `isolate`, `php`, `composer`, `services:*`), plus `herd` alias shims so existing scripts keep working.
- An MCP server with the same tool surface as the Herd MCP (`site`, `logs`, `insights`, `mail`, `db`, `env`, `services`, `processes`, `runtime`, `workspaces`).
- Fix Herd pain points the owner already hit: certificates that expire silently (auto-renew), services that fail with no visible reason (errors in the UI), versioned service paths breaking PATH.

## Non-goals

- Studio, AI chat, AI anything.
- Linux (could come later; the design keeps OS code behind one interface).
- Forge/Vapor deployment integration.
- Hosting production sites.

## Anti-goals (fails even if it works)

- Slower page loads than Herd for the same site.
- Needing an admin prompt on every start (one-time elevation only).
- Breaking a running Herd install during evaluation: Hearth must be installable side by side and only take over ports when the owner switches.
- Losing local database data during migration.

## Constraints

- Stack (my call, matches Kern/Quire/Glint): Tauri 2, Rust core, Svelte 5 + TypeScript UI, Vite. One Rust "daemon" core owns all processes; the UI, CLI and MCP talk to it.
- Personal GitHub `wl-lankin`, noreply identity, **public** repo `wl-lankin/hearth` with code and releases in the same repo (owner decision, like Glint). No secrets, machine paths or company data in the repo.
- Signing (owner decision): Windows via Azure Trusted Signing, macOS via an Apple Developer account with notarization. Both are being set up by the owner; until they exist, release builds are unsigned and only carry Tauri updater signatures, and the workflow switches signing on once the secrets are present.
- Windows 11 is the primary machine; macOS (Apple Silicon first) is a full target.
- Writing style: no em/en dashes in UI strings, docs, commits.

## Name and logo

- **Hearth**: the warm home of local sites. CLI: `hearth`. Local domain stays `.test`.
- Logo concept (my call, owner reviews in M0): a rounded arch (fireplace opening / house silhouette) with a single soft flame inside, ember gradient (amber to deep red) on a warm charcoal tile. No sparkles, no AI-looking glyphs. Two alternatives are drawn as SVG in M0 for the owner to pick from.

## Compatibility with an existing Herd install (Windows)

What Hearth must keep working or take over when a user switches from Herd. The owner's own inventory (counts, project names, local paths) lives in the git-ignored `docs/local/herd-footprint.md`.

- **Shims**: scripts and AI tool configs often hardcode `~/.config/herd/bin/php.bat`, `php8X.bat`, `composer.bat`. Hearth writes its shims to `~/.config/hearth/bin` and, at switch-over, can replace the Herd shims with thin forwarders.
- **Hosts**: Herd writes exact `127.0.0.1 <host>.test` lines into the hosts file between its own markers; linked sites are symlinks in `config\valet\Sites` (often several aliases per app); parked paths live in `config\valet\config.json`; proxies forward to local dev servers such as Vite on `http://localhost:<port>`.
- **TLS**: Herd's Valet CA (`config\valet\CA\LaravelValetCASelfSigned.*`) is usually already trusted. Hearth can adopt it so browsers keep trusting every site.
- **PHP**: Herd ships php.net-style folders with `php-cgi.exe` on FastCGI ports 9000+ver, an Xdebug pool at +100 (chosen by an `XDEBUG_*` cookie in nginx), and a per-site `# ISOLATED_PHP_VERSION=XX` header in each site conf. Hearth uses the same scheme on its own ports so both can run side by side.
- **php.ini coupling**: Herd's ini files load `auto_prepend_file=...dump-loader.php` and `php_herd-8.X.dll`, which rewrites `bootstrap/app.php`. Hearth uses its own PHP builds and ini files and never reuses Herd's.
- **Services**: projects commonly expect Redis on 6379 and the mail catcher on 2525. Databases that run outside Herd (e.g. a standalone MySQL/MariaDB service on 3306) are left alone.
- **Node**: Herd's nvm folder is on the user PATH.
- **MCP**: Herd registers its MCP server as `herd` (`herd mcp`). Hearth registers `hearth` alongside; the user drops `herd` at switch-over.
- **PATH**: Herd's bin, service and nvm folders get swapped for Hearth's at switch-over.
- **DNS on Windows**: Herd uses only the hosts file (no resolver, no wildcard). Hearth does the same by default.

## Architecture

```
 Tray + main window (Svelte)      hearth CLI       Hearth MCP server
            \                         |                 /
             +------- local IPC (named pipe / unix socket, JSON-RPC) -----+
                                      |
                           hearthd (Rust core, runs in the app process,
                           or headless when the UI is closed)
   +---------+----------+-----------+-----------+----------+-----------+
   | PHP     | Web      | DNS + TLS | Services  | Capture  | Processes |
   | runtime | (nginx)  |           | (DB, ...) | (mail,   | (vite,    |
   | manager | sites    |           |           | dumps..) | queues)   |
   +---------+----------+-----------+-----------+----------+-----------+
                     OS layer: windows.rs / macos.rs
```

- **PHP**: official php.net builds on Windows (NTS x64), static-php-cli builds on macOS. One folder per version under `~/.config/hearth/bin/php/<ver>`. Shims `php`, `php83`, `php84`, `php85`, `composer`, `laravel` in `~/.config/hearth/bin`. Per-site isolation: Windows runs a small pool of `php-cgi` per used version (FastCGI, restarted on crash/after N requests); macOS runs `php-fpm` per version.
- **Web**: bundled nginx, one generated conf per site (parked, linked, proxied), Valet-style driver detection (Laravel, generic `public/`, static).
- **DNS**: Windows managed block in the hosts file (exact names, written by a one-time elevated helper service), optional wildcard via Acrylic; macOS `dnsmasq` + `/etc/resolver/test`.
- **TLS**: own local CA (rcgen), trusted once (Windows cert store / macOS keychain); leaf certs per site, auto-renewed 30 days before expiry. Migration can reuse Herd's already trusted Valet CA.
- **Services**: MySQL, MariaDB, PostgreSQL, Redis (Windows: Memurai-free alternative or redis-windows build), Meilisearch, Typesense, MinIO. Each instance = version + port + data dir + log; started by absolute path (never bare exe names), errors shown in the UI.
- **Capture**: a PHP `auto_prepend_file` collector (dumps, queries, logs, jobs, HTTP calls, mails via the SMTP catcher) that sends events to hearthd; built-in SMTP catcher in Rust on port 2525.

## Milestones

Each milestone gets its own implementation plan against the real code when it starts. Every milestone ends tested on Windows; macOS checks run in CI plus a manual pass before M14.

| # | Milestone | Done when |
|---|---|---|
| M0 | Brand + skeleton | Repo, Tauri app with tray + main window, Svelte shell, settings store, IPC core, CI building Windows + macOS, logo picked, icons generated. |
| M1 | PHP runtime manager | Install/remove PHP versions from the UI and CLI, global default, shims on PATH, php.ini editor, extension toggles (incl. Xdebug), Composer + Laravel installer bundled. |
| M2 | Sites | nginx managed by hearthd; park folders, link sites, per-site PHP isolation, proxies (`http://localhost:<port>`, IPv6-safe), site list with open/edit/unlink. |
| M3 | DNS + TLS | `.test` resolves on both OSes, local CA trusted once, `secure`/`unsecure`, auto-renew, proxies keep their upstream when renewed. |
| M4 | CLI + Herd compatibility | `hearth` CLI with Herd-shaped commands, `herd` alias shims, JSON output for scripts. |
| M5 | Services | All at once (owner decision): MySQL, MariaDB, PostgreSQL, Redis, Meilisearch, Typesense, MinIO. Create, version, port, start/stop/autostart, logs and errors in the UI, `.env` snippet copy. Port conflicts (e.g. a standalone database service on 3306) are detected and a free port is suggested. |
| M6 | Node | Node version manager (per-site `.nvmrc` respected), `node`/`npm`/`npx` shims. |
| M7 | Mail | SMTP catcher, inbox UI (HTML/text/raw/attachments), per-site filter, link extraction. |
| M8 | Dumps, logs, insights | Collector extension, dump viewer, Laravel log viewer, per-request timeline with N+1 / duplicate / slow query detection. |
| M9 | Debugging | One-click Xdebug on/off per site, auto-detect a listening IDE, SPX profiler view. |
| M10 | Processes | Long-running dev processes per site (`artisan dev`, Vite, queue, Pail) with live output and restart. |
| M11 | Workspaces | Git worktree + own `.test` domain + optional own database, copies `.env` and `.env.testing`. |
| M12 | Hearth MCP | MCP server with the Herd tool surface, registered for Claude Code. |
| M13 | Migration from Herd | Import wizard: parked paths, linked sites + aliases, proxies, isolations, Valet CA + leaf certs, hosts block, Redis `dump.rdb`; shim forwarders, PATH swap, MCP registration; dry run first; side-by-side mode until switch-over; leaves databases that run outside Herd untouched. |
| M14 | Release | Installers polish (NSIS per user + one-time helper; macOS dmg), code signing (Azure Trusted Signing, Apple notarization). Auto-update and the release workflow already ship in M0. |

Daily-driver point: after **M5 + M7 + M12 + M13** the owner can switch off Herd for normal work; M6, M8 to M11 can follow while using it.

## Left to the builder

Exact crate choices, UI layout per screen (mockups at the start of each UI milestone), port defaults, and file formats of Hearth's own config (JSON under `~/.config/hearth/config`).

## Owner decisions (2026-10-23)

1. Apple Developer account and Azure Trusted Signing certificate: both in progress on the owner's side.
2. Public repo.
3. M5 ships all services at once.
4. Hearth starts with the OS by default (installed builds register "Start at login" on their first run; the switch in Settings turns it off) and updates itself from the GitHub releases of `wl-lankin/hearth` (background download, installs on quit or via "Restart to update"). Both move into M0; M14 keeps installer polish and code signing.
5. PHP comes from a Hearth-maintained manifest (decided at M1 start): a second public repo `wl-lankin/hearth-php` builds macOS PHP (cli + fpm, arm64 + x86_64) with static-php-cli on GitHub Actions, including `redis` built in and `xdebug` as a shared extension, and publishes one signed `manifest.json` with URL + SHA-256 for every OS, PHP version, extension, Composer and the CA bundle. Windows entries point to the official php.net zips and the official PECL Windows builds of `redis` and `xdebug`. Hearth installs only what the manifest lists and verifies every download.
6. The web server is Caddy instead of nginx (decided at M2 start): official checksummed builds for Windows and macOS (arm64 + x64), listed in the same signed catalog; built-in PHP FastCGI and reverse proxy. Hearth generates the whole Caddyfile; Caddy's admin API stays off and Hearth restarts Caddy to apply changes.
