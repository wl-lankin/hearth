# Hearth M3 (DNS + TLS) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `<name>.test` resolves on Windows and macOS without touching the browser, and `hearth secure <name>` serves the site over HTTPS with a certificate from Hearth's own local CA, trusted once, renewed automatically.

**Architecture:** A `tls` module in hearth-core owns the CA (created once with rcgen) and one leaf certificate per secured site; the web sync issues and renews leaves before it renders the Caddyfile, which gains an HTTPS block per secured site (Caddy loads the files, its own automatic HTTPS stays off). A `dns` module keeps the names resolving: on Windows a managed block in the hosts file, written by a small helper service (`hearth-helper`) that is installed once with one UAC prompt; on macOS a tiny DNS responder inside Hearth answers `*.test`, and `/etc/resolver/test` (written once with one admin prompt) points macOS at it.

**Tech Stack:** Existing M2 stack. `rcgen` 0.13 (CA + leaf, `x509-parser` feature to read expiry back), `windows-service` 0.8 for the helper, `windows-sys` `Win32_Security_Cryptography` (cert store) and `Win32_UI_Shell` (`ShellExecuteExW` "runas").

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md` (milestone M3, "Compatibility with an existing Herd install", architecture DNS/TLS lines).

## Global Constraints

- Everything from M0 to M2 applies (public repo, no machine or company details, no long dashes, commits `M3: <description>`, branch `HEARTH-6` from `main`, push only on the owner's go).
- Side by side with Herd: no ports 80/443. HTTPS port `8443` (`httpsPort` in `sites.json`), HTTP stays `8080`. Herd's hosts block, CA and certificates are never read or changed in M3 (adopting Herd's CA is M13).
- One-time elevation only: the helper install (Windows, one UAC prompt), the CA trust (Windows: the user-store confirmation dialog, no admin; macOS: one password prompt), `/etc/resolver/test` (macOS, one password prompt). Nothing else ever asks.
- Names: hosts lines only for valid site names (`[a-z0-9-]` labels, M2 rules) + `.test`, always `127.0.0.1` (like Herd; IPv4 only so Herd's own sites never try `::1` first). Hearth's block sits between `# hearth-start` and `# hearth-end`; nothing outside it changes, line endings and encoding of the rest are kept byte for byte.
- Files: CA at `<data>/tls/ca.crt` + `ca.key`, leaves at `<data>/tls/sites/<name>.crt` + `.key`; keys readable by the user only (macOS `0600`; Windows profile folder is already private). CA valid 10 years, CN `Hearth Local CA (<user>@<host>)`; leaves valid 397 days with SANs `<name>.test` and `*.<name>.test`, renewed when fewer than 30 days remain.
- The helper accepts exactly two requests over its pipe: `status` and `hosts {names}`. It writes nothing but Hearth's hosts block, refuses any invalid name, and runs from `%ProgramFiles%\Hearth\hearth-helper.exe` (admin-only folder) so a user process cannot swap the binary.

## Review Focus

1. **The hosts file has other content, CRLF or LF, a BOM, no trailing newline, or a broken earlier Hearth block (start without end)**: everything outside Hearth's block stays byte-identical; a broken block is replaced, not duplicated. Tests in Task 5.
2. **A name that would inject a hosts line** (`a.test\n1.2.3.4 bank.com`, `bank.com`, a 300-character name) reaches the helper: refused as a whole, hosts untouched. Test in Task 6.
3. **A secured site whose certificate expires or is deleted, or a proxy secured before a renewal**: the next sync issues a new leaf, the proxy keeps its upstream, Caddy restarts once. Tests in Tasks 2 and 4.
4. **The CA is not trusted yet, or trust was removed in the OS**: the Sites view says "Browsers do not trust Hearth yet" with one "Trust" button; sites still serve. Test in Task 3 (status) and Task 8 (view).
5. **The helper is not installed or not running**: sites work through `curl --resolve`, the view shows "Set up .test names" instead of failing silently; a site change never blocks on the helper (1 s pipe timeout). Test in Task 7.

---

### Task 1: Local CA and leaf certificates (core)

**Files:** `crates/hearth-core/src/tls/{mod.rs, ca.rs}`, `Cargo.toml` (`rcgen = { version = "0.13", features = ["pem", "x509-parser"] }`, `time`).

**Interfaces:**
- `ensure_ca(paths) -> Result<Ca, CoreError>`: loads `ca.crt`/`ca.key`, or creates them (CA flag, key usage cert sign + CRL sign, path length 0).
- `Ca::fingerprint() -> String` (SHA-256 hex of the DER, used to find it in OS stores).
- `ensure_leaf(paths, ca, name, now) -> Result<LeafState, CoreError>`, `LeafState = Unchanged | Issued`: issues when missing, unreadable, signed by another CA, or expiring within 30 days.
- `remove_leaf(paths, name)`.

- [ ] **Step 1:** Failing tests: a fresh home creates the CA once (second call returns the same fingerprint); a leaf has both SANs and verifies against the CA (rcgen parse, issuer = CA subject); `ensure_leaf` at `now` returns `Unchanged`, at `not_after - 29 days` returns `Issued`; a leaf whose file was deleted is re-issued; a leaf from a replaced CA is re-issued; on unix the key mode is `0600`.
- [ ] **Step 2-4:** RED, implement, GREEN. **Step 5:** Commit `M3: Local CA and site certificates`.

### Task 2: Secured sites in the registry and the Caddyfile (core)

**Files:** `crates/hearth-core/src/sites/{registry.rs, caddyfile.rs}`.

**Interfaces:**
- `SitesConfig` gains `secured: BTreeSet<String>` and `https_port: u16` (default `8443`, `#[serde(default)]` keeps old files loading); `Site` gains `secured: bool`.
- `secure(paths, name)`, `unsecure(paths, name)`: the name must be a current site ("No site named shop.test"); `unlink`/`unproxy` drop the name from `secured`.
- `SiteRoute` gains `tls: Option<(PathBuf, PathBuf)>`; `CaddyContext` gains `https_port`. Global block adds `https_port <p>`. A secured route renders its block at `https://<name>.test:<https>` with `tls "<crt>" "<key>"`, plus `http://<name>.test:<http> { redir https://<name>.test:<https>{uri} }`.

- [ ] Failing tests: a secured PHP site renders both blocks exactly; a secured proxy keeps `reverse_proxy <upstream>` and the Host header line; key paths with spaces and braces are quoted like docroots; an old `sites.json` without the new fields loads with `httpsPort: 8443`; `unproxy` removes the name from `secured`.
- [ ] Implement, GREEN, commit `M3: Secure sites over HTTPS`.

### Task 3: Trusting the CA (core, OS layer)

**Files:** `crates/hearth-core/src/tls/trust/{mod.rs, windows.rs, macos.rs}`.

**Interfaces:** `is_trusted(ca) -> bool`; `trust(ca) -> Result<(), CoreError>`; `untrust(ca)`. Windows: `CertOpenStore(CERT_SYSTEM_STORE_CURRENT_USER, "Root")`, find by SHA-1 hash, `CertAddEncodedCertificateToStore` (Windows shows its own confirmation; "No" -> `CoreError::Tls("Trust was not confirmed")`). macOS: `security add-trusted-cert -r trustRoot -k <login keychain> ca.crt` (prompts for the password), `is_trusted` = `security verify-cert -c ca.crt -p ssl` exit 0.

- [ ] Failing tests: `is_trusted` is false for a freshly created CA (both OSes, no prompt); macOS command lines built by a pure function match exactly. The positive path is covered in the Task 9 check (it needs the confirmation).
- [ ] Implement, GREEN, commit `M3: Trust the local CA once`.

### Task 4: Certificates in the web sync, auto-renew (core)

**Files:** `crates/hearth-core/src/web/service.rs`, `src-tauri/src/lib.rs` (health loop).

**Interfaces:** `sync_with` runs `ensure_ca` + `ensure_leaf` for every secured site before rendering (a TLS failure skips that site's HTTPS block with a `# <name>: <reason>` comment and goes into `error`, the HTTP block still serves); leaves of names no longer secured are removed. `WebStatus` gains `https_port` and `ca_trusted: bool`. A leaf that was `Issued` forces one Caddy restart even when the Caddyfile text is unchanged (Caddy reads certificate files only at start). The existing 30 s health loop covers renewal.

- [ ] Failing tests (fake Caddy from M2): securing a site writes its leaf and an HTTPS block; deleting the leaf file and syncing again re-issues it and restarts Caddy (restart count +1) with the proxy upstream unchanged; unsecuring removes the leaf files.
- [ ] Implement, GREEN, commit `M3: Issue and renew certificates during sync`.

### Task 5: Hosts block (core, pure)

**Files:** `crates/hearth-core/src/dns/{mod.rs, hosts.rs}`.

**Interfaces:** `valid_host(name) -> bool` (site-name rules + `.test`, <= 253 chars); `apply_block(hosts: &[u8], names: &BTreeSet<String>) -> Vec<u8>`: replaces or appends the block (`# hearth-start`, one `127.0.0.1 <name>` line per name, `# hearth-end`), keeps the file's line ending style (CRLF if the file uses it), its BOM and everything else; an empty set removes the block; a start marker without an end replaces from the marker to the end of the file. `names_in_block(hosts) -> BTreeSet<String>`.

- [ ] Failing tests: Review Focus 1 cases one by one (CRLF file, LF file, BOM, no trailing newline, broken block, Herd's `#herd` block untouched, idempotent second apply returns identical bytes); `valid_host` refuses the Review Focus 2 inputs.
- [ ] Implement, GREEN, commit `M3: Manage a hosts block`.

### Task 6: The Windows helper service

**Files:** new crate `crates/hearth-helper` (`main.rs`, `service.rs`, `protocol.rs`), workspace member; `src-tauri/tauri.conf.json` `bundle.externalBin` so installers ship it next to the app.

**Interfaces:**
- `hearth-helper install` (run elevated): copies itself to `%ProgramFiles%\Hearth\hearth-helper.exe`, registers service `HearthHelper` (LocalSystem, automatic start, description "Keeps Hearth's .test names in the hosts file"), starts it. `hearth-helper uninstall`: removes Hearth's block, stops and deletes the service and the copy.
- Pipe `\\.\pipe\hearth-helper`, one JSON line per request: `{"op":"status"}` -> `{"version":"0.1.0","names":[...]}`; `{"op":"hosts","names":[...]}` -> `{"ok":true}` or `{"error":"..."}`. Pipe security: Authenticated Users read/write, nothing else. Max request 64 KiB, max 2000 names.
- `protocol::handle(request, hosts_path) -> Response` is the testable core (no SCM, no pipe).

- [ ] Failing tests on `protocol::handle` with a temp hosts file: `hosts` writes the block; an invalid name refuses the whole request and leaves the file byte-identical; `status` reports the names in the block; an oversized request is refused.
- [ ] Implement (service wrapper and install verbs are thin around it), GREEN, commit `M3: Hosts helper service for Windows`.

### Task 7: Keeping names in step (core)

**Files:** `crates/hearth-core/src/dns/{client.rs, resolver.rs}`, `web/service.rs`.

**Interfaces:**
- Windows `dns::client`: `status() -> HelperStatus { installed, running, names }`, `set_names(names)` (1 s connect timeout, never blocks a sync); `install()` runs the bundled helper with `ShellExecuteExW` verb `runas` and waits for it.
- macOS `dns::resolver`: a UDP + TCP responder on `127.0.0.1:10053` answering A `127.0.0.1` / AAAA `::1` for `*.test`, NXDOMAIN for any other name, ignoring malformed packets; `install()` writes `/etc/resolver/test` (`nameserver 127.0.0.1` + `port 10053`) through `osascript ... with administrator privileges`.
- The sync pushes the set of all site names (`<name>.test`) when it differs from the helper's; `WebStatus` gains `dns: DnsStatus { ready: bool, problem: Option<String> }`.

- [ ] Failing tests: with no helper pipe, `set_names` returns within 1.5 s with "The .test helper is not installed" and the sync still serves; (unix) the responder answers `shop.test` A with `127.0.0.1`, AAAA with `::1`, `example.com` with NXDOMAIN, and survives a 3-byte garbage packet; the resolver file text is exact.
- [ ] Implement, GREEN, commit `M3: Resolve .test names on Windows and macOS`.

### Task 8: IPC, CLI and the Sites view

**Interfaces:** IPC `sites.secure {name}`, `sites.unsecure {name}`, `tls.trust`, `dns.setup`; `web.status` carries `httpsPort`, `caTrusted`, `dns`. CLI (Herd-shaped): `hearth secure [name]`, `hearth unsecure [name]`, `hearth secured [--json]`, `hearth trust`, `hearth dns:setup`; `hearth open` uses `https://` for secured sites.

**View:** a lock toggle per row (Secure / Not secure), URLs show `https://shop.test:8443` when secured; above the table at most one setup card at a time, in order: "Set up .test names" (button "Set up", says one admin prompt follows), then "Browsers do not trust Hearth yet" (button "Trust"). No colored side rails (owner taste).

- [ ] Failing tests: router happy paths and `-32602`s; store: toggling secure rolls back on error; the setup card picks DNS before trust; CLI `secured --json` against a served core.
- [ ] Implement, GREEN, screenshots light/dark at 1100x720 and 760x520, commit `M3: Secure sites from the app and CLI`.

### Task 9: End-to-end check (owner go needed: UAC prompt, trust dialog, real hosts file)

- [ ] With a test home: `hearth dns:setup` installs the helper (one UAC prompt), linking a site adds exactly its line to Hearth's hosts block and Herd's block is unchanged; `hearth trust` (one confirmation); `hearth secure demo`, then `curl https://demo.test:8443` (schannel, no `-k`) returns the Laravel page and a browser shows the lock; a secured proxy to Vite works over `https://`; deleting the leaf makes the next sync renew it; `hearth unlink demo` removes the name from the hosts block; `hearth-helper uninstall` leaves the hosts file as it was before the test.

## Done when

- On Windows, `https://<name>.test:8443` works in a browser without warnings for linked, parked and proxied sites, names follow site changes without prompts after the one-time setup, certificates renew on their own, and the macOS parts pass CI.
