# M7 Mail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Mail sent by local apps to `127.0.0.1:2525` lands in a Mail page in Hearth: an inbox per app, each mail shown as HTML, text or raw source, with its attachments and links.

**Architecture:** An SMTP catcher runs inside the core (a tokio listener on loopback, started by the app like the web server). Every mail is stored as `<data>/mail/<id>.eml` plus a small `<id>.json` summary. `mail.*` IPC methods list, read, delete and save from that folder; the Svelte Mail page polls `mail.list` while it is open. Apps are told apart by their SMTP username, as in Herd (`MAIL_USERNAME="${APP_NAME}"`).

**Tech Stack:** Rust (hearth-core, src-tauri), `mail-parser` 1.0 for MIME, `lettre` as a dev-dependency (a real SMTP client in tests), Svelte 5 UI.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`, M7: "SMTP catcher, inbox UI (HTML/text/raw/attachments), per-site filter, link extraction." The owner skipped the design questions on 2026-11-02 ("start coding"); every choice below not quoted from the spec is the builder's call.

## Global Constraints

- Never use em or en dashes, in code, comments, messages or docs.
- The catcher listens on `127.0.0.1` only, default port 2525 (Herd's), stored in `config/mail.json` as `{"port": 2525}`. Any credentials are accepted; nothing is ever relayed.
- Mail HTML never runs scripts in Hearth: it is shown in an `<iframe sandbox="allow-same-origin">` (no `allow-scripts`), and links open only through `open_url` (http and https).
- At most 1000 mails are kept; the oldest go first. One mail may be at most 25 MB (`SIZE 26214400`).
- Tests never use port 2525: they bind `127.0.0.1:0`.

## Review Focus

1. A mail that is not clean MIME (no Subject, 8-bit headers, broken multipart, bare `\n` line ends) is still stored, listed and shown, and its raw source is byte for byte what was sent.
2. A client that disconnects or sends RSET in the middle of DATA leaves nothing stored, and the catcher keeps serving other clients.
3. A mail over 25 MB gets `552`, nothing is stored, and the same connection can send a smaller one after.
4. Port 2525 taken (Herd running side by side): the Mail page shows the problem and lets the owner pick another port; the rest of Hearth starts normally.
5. Hostile HTML (`<script>`, `onerror=`, `<meta http-equiv=refresh>`, `javascript:` links, a form) does nothing in the app; only http(s) links open, in the default browser.

---

### Task 1: `mail::store` keeps and reads mails

**Files:**
- Create: `crates/hearth-core/src/mail/mod.rs`, `mail/store.rs`, `mail/store_tests.rs`
- Modify: `crates/hearth-core/src/lib.rs`, `crates/hearth-core/Cargo.toml` (`mail-parser = "1"`)

**Interfaces:**
- Produces: `pub struct Envelope { site: Option<String>, from: String, to: Vec<String> }`; `pub struct Summary { id, site: Option<String>, from: String, to: Vec<String>, subject: String, received: u64 (unix secs), size: u64, attachments: u32, seen: bool }` (serde camelCase); `pub fn save(paths, &Envelope, raw: &[u8]) -> Result<Summary, CoreError>`; `pub fn list(paths) -> Vec<Summary>` (newest first; unreadable files skipped); `pub fn get(paths, id) -> Result<Mail, CoreError>` with `Mail { summary, cc: Vec<String>, html: Option<String>, text: Option<String>, attachments: Vec<Attachment { index, name, content_type, size }> }`, marking it seen; `pub fn raw(paths, id) -> Result<(String, bool), CoreError>` (lossy UTF-8, first 1 MB, truncated flag); `pub fn attachment(paths, id, index) -> Result<(String, Vec<u8>), CoreError>`; `pub fn delete(paths, id)`; `pub fn clear(paths, site: Option<Option<&str>>)` (`None` all, `Some(None)` mails without a username, `Some(Some(s))` one app).

- [ ] **Step 1: failing tests:** `save_then_list_and_get_a_multipart_mail` (HTML + text + one PDF attachment: summary fields, `get` gives both bodies and the attachment, `seen` flips to true); `inline_cid_images_become_data_urls` (`<img src="cid:logo">` in the HTML is replaced by a `data:image/png;base64,` URL and the part is not listed as an attachment); `a_broken_mail_is_still_kept` (no headers at all, bare `\n`, 8-bit bytes: listed with subject `(no subject)`, `raw` returns the exact bytes); `ids_sort_by_arrival_and_only_1000_are_kept` (1002 saves, the two oldest are gone); `clear_by_site_and_unnamed`; `ids_from_outside_are_checked` (`get("../x")` is an error, no file read).
- [ ] **Step 2:** `cargo test -p hearth-core mail::store` fails to compile.
- [ ] **Step 3:** implement. Id `{unix_millis:013}-{seq:04}` (a process-wide counter), checked against `^[0-9]{13}-[0-9]{4}$` on every read. Writes go through `atomic::write`, `.eml` first, then `.json`. Subject and From come from `mail-parser`, falling back to the envelope; HTML-only mails get their text from `mail-parser`'s `body_text`.
- [ ] **Step 4:** tests pass, clippy clean.
- [ ] **Step 5:** commit "Keep caught mails on disk".

### Task 2: `mail::smtp` catches mail

**Files:**
- Create: `crates/hearth-core/src/mail/smtp.rs`, `mail/smtp_tests.rs`
- Modify: `crates/hearth-core/src/mail/mod.rs`, `crates/hearth-core/src/ipc/mod.rs` (field `mail: Arc<mail::smtp::Catcher>`), `crates/hearth-core/Cargo.toml` (dev: `lettre` without default features, `smtp-transport`, `builder`)

**Interfaces:**
- Produces: `pub async fn serve(listener: tokio::net::TcpListener, paths: HearthPaths) -> ()` (accept loop, one task per client); `pub struct Catcher` with `pub async fn start(&self, paths: &HearthPaths, port: u16)`, `pub fn stop(&self)`, `pub fn status(&self) -> CatcherStatus { port: u16, listening: bool, error: Option<String> }`; `pub fn configured_port(paths) -> u16` / `pub fn set_configured_port(paths, port)` on `config/mail.json`.
- Protocol: greeting `220 hearth ESMTP`; EHLO lists `SIZE 26214400`, `8BITMIME`, `AUTH PLAIN LOGIN`; HELO, AUTH PLAIN (inline or after `334 `), AUTH LOGIN, MAIL FROM (with `SIZE=`), RCPT TO, DATA (dot unstuffing, `\r\n.\r\n` or `\n.\n`), RSET, NOOP, VRFY (`252`), QUIT. Order errors `503`, unknown `502`, oversize `552`, a command line over 4 KB `500`. 5 minutes idle closes the connection. The AUTH username (trimmed, empty is none) becomes `Envelope.site`.

- [ ] **Step 1: failing tests:** `lettre_sends_with_a_username_and_it_is_kept_for_that_site`; `plain_smtp_without_auth_is_kept_without_a_site` (raw TCP script, including a dot-stuffed line); `auth_plain_and_login_both_work`; `a_dropped_connection_mid_data_keeps_nothing` (then a second client still gets `220`); `too_big_is_552_and_the_connection_goes_on`; `commands_out_of_order_get_503`; `a_taken_port_is_reported_not_fatal` (`start` on a bound port: `listening: false`, error names the port and says another program uses it).
- [ ] **Step 2:** `cargo test -p hearth-core mail::smtp` fails.
- [ ] **Step 3:** implement with `tokio::io::BufReader` and `read_until(b'\n')`, a byte budget while reading DATA (keep reading to the dot, store nothing, answer `552`).
- [ ] **Step 4:** tests pass.
- [ ] **Step 5:** commit "Catch mail on 127.0.0.1:2525".

### Task 3: `mail.*` methods and the app

**Files:**
- Create: `crates/hearth-core/src/mail/api.rs`, `mail/api_tests.rs`
- Modify: `crates/hearth-core/src/ipc/mod.rs` (route `mail::api::handle`), `src-tauri/src/lib.rs` (start the catcher in `start_web`, stop it in `stop_core`), `src-tauri/capabilities/default.json` (`dialog:allow-save`), `src-tauri/tauri.conf.json` (`img-src 'self' data: http: https:` so mails show their remote images)

**Interfaces:**
- Produces: `mail.status` -> `{port, listening, error, env}` where `env` is the `.env` snippet `MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT=<port>\nMAIL_USERNAME="${APP_NAME}"\nMAIL_PASSWORD=null`; `mail.list` -> `Summary[]`; `mail.get {id}` -> `Mail`; `mail.raw {id}` -> `{text, truncated}`; `mail.saveAttachment {id, index, path}`; `mail.delete {id}`; `mail.clear {site?}` (`site` missing: all, `null`: no username, string: that app); `mail.port {port}` (saves, restarts the catcher, answers the new status; a taken port is an error and keeps the old one listening).

- [ ] **Step 1: failing tests:** `status_list_get_and_delete_through_the_core`; `clear_takes_a_site_null_or_nothing`; `save_attachment_writes_the_bytes_to_the_chosen_path`; `port_change_moves_the_catcher_and_a_taken_port_keeps_the_old_one` (ports picked free by the test, never 2525).
- [ ] **Step 2:** run, expect failures.
- [ ] **Step 3:** implement; the app starts `core.mail.start(&paths, configured_port)` next to the autostart services.
- [ ] **Step 4:** `cargo test -p hearth-core mail`, `cargo clippy --workspace --all-targets` clean.
- [ ] **Step 5:** commit "Mail methods in the core and the app".

### Task 4: the Mail page

**Files:**
- Create: `ui/src/mail.svelte.ts`, `ui/src/mail.spec.ts`, `ui/src/views/Mail.svelte`
- Modify: `ui/src/api.ts` (types, calls, demo data with three mails in two apps), `ui/src/App.svelte` (nav "Mail" after Node)

**Interfaces:**
- Consumes: Task 3 methods.
- Produces: `createMailStore(api)` with `load()`, `refresh()`, `inboxes` (distinct sites with counts and unread counts, unnamed shown as "Without a name"), `filter`, `mails` (filtered), `open(id)`, `current`, `raw()`, `remove(id)`, `clear()`, `setPort(port)`; `links(html, text): {url, text}[]` (anchors from `DOMParser`, then bare URLs in the text, http(s) only, first occurrence wins).

Layout (builder's call, like a mail client): a top strip with "Listening on 127.0.0.1:2525" (or the error and a port field), the `.env` snippet with Copy; below, a list (inbox filter, then subject, from, time, unread dot, paperclip) beside the open mail (subject, from, to, cc, date, attachments with Save; tabs HTML, Text, Raw, Links (n)). Delete on the open mail, Clear for the filtered inbox. The page polls `mail.list` every 2 s while open.

- [ ] **Step 1: failing tests** in `mail.spec.ts`: inboxes and unread counts; filter then clear only that inbox; opening marks read; `links` dedupes, drops `javascript:` and `mailto:`, keeps text URLs; a port error shows and `setPort` clears it.
- [ ] **Step 2:** `npx vitest run src/mail.spec.ts` fails.
- [ ] **Step 3:** implement the store and the view. The HTML tab is an iframe with `srcdoc` and `sandbox="allow-same-origin"`; a click listener on its document sends http(s) links to `openUrl` and swallows every other click on a link. Attachments save through the dialog's `save()` with the mail's file name as the default.
- [ ] **Step 4:** `npx vitest run` and `npm run check` pass; screenshots light and dark at 1100 and 760 from the browser demo (`?view=mail`).
- [ ] **Step 5:** commit "Mail page".

### Task 5: hands-on check on Windows

- [ ] With a test home whose `config/mail.json` says 2626, start the app (own PID), send from Python's `smtplib` with and without login, an HTML mail with a cid image and a PDF, and one over 25 MB. Check the Mail page: inboxes, HTML with the image, Text, Raw, Links opening in the browser, saving the PDF, Delete, Clear. Change the port in the page, send again. Stop the app by its PID and delete the test home.
