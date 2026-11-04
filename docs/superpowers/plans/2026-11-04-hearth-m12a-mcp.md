# Hearth M12a: MCP Server (Herd Tool Surface) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `hearth mcp` is an MCP server with Herd's MCP tool surface. Agents set up for Herd (Claude Code first) keep working: the same tool names, actions and parameters, answered by Hearth's core. `hearth mcp:install` registers it.

**Architecture:**
- A new CLI command `mcp` runs a stdio JSON-RPC 2.0 server (newline-delimited) inside `hearth-cli`.
- Each tool is a table of actions. Each action builds a core IPC call (the same methods the app and CLI use) and turns the reply into a text result.
- The site comes from the `site` parameter, else `SITE_PATH`, else the process's working folder, through `sites::locate`.
- Actions that need core features M12b adds (env, db, site error) and features Hearth does not have (Studio, Forge, Herd's debug sessions) answer `isError` with a clear sentence. The tool list stays complete.

**Tech Stack:** Rust (hearth-cli, hearth-core IPC client), serde_json.

**Spec:** `docs/superpowers/specs/2026-10-23-hearth-design.md`:
- Goals: "same MCP tools for Claude Code"; "An MCP server with the same tool surface as the Herd MCP (`site`, `logs`, `insights`, `mail`, `db`, `env`, `services`, `processes`, `runtime`, `workspaces`)."
- "Hearth registers `hearth` alongside; the user drops `herd` at switch-over."
- Reference for Herd's schemas: Herd's own `tools/list`, extracted to the scratchpad file `herd-mcp/tools-list.json`. Descriptions and parameters are copied from it, with "Herd" replaced by "Hearth" and Pro-only notes dropped.

## Global Constraints

- No em or en dashes. English. No attribution footers.
- Branch `HEARTH-24`, stacked on `HEARTH-23` (#18 waits for a macOS runner).
- stdout carries only JSON-RPC lines; everything else goes to stderr.
- Tests never bind 2525, 2306 or 9003, and never register anything in the owner's real Claude config.

## Protocol (from Herd's server)

- **Handled methods:** `initialize`, `ping`, `tools/list` (one page), `tools/call`, `prompts/list` (empty), `resources/list` (empty), `notifications/*` (no reply). Others answer -32601.
- **initialize:** echoes the client's `protocolVersion` (default `2025-03-26`). Returns `serverInfo {name:"Hearth", version}`, capabilities `tools`, `prompts`, `resources` (`listChanged:false`), and an `instructions` text adapted from Herd's (site at `<site>.test`, start with `site info`, use `env`/`db` instead of editing `.env`, `workspaces create` instead of `git worktree add`, `processes` for dev processes).
- **tools/call:**
  - Result `{content:[{type:"text",text}], isError?}`. A JSON result is pretty-printed text.
  - Unknown action: `Unknown action "x". Use one of: a, b`.
  - Missing parameter: `The {action} action needs: a, b.`
  - Core not running: "Hearth is not running. Start the Hearth app."
  - Unknown tool: JSON-RPC -32602 with the tool name.
  - A `null` argument counts as absent.

## Tool map (M12a)

| Tool | Actions backed now | Core methods |
|---|---|---|
| site | info, list, link, secure, unsecure, isolate, unisolate | `sites.list`, `web.status`, `node.which`, `workspaces.list`, `sites.link/secure/unsecure/isolate` |
| site | error, studio | not yet / no Studio |
| mail | list, show, links, wait, clear, config | `mail.list/get/raw/clear/status` (wait polls `mail.list` every 500 ms for mail newer than the call's start; `links` extracts http(s) URLs from the body) |
| insights | list, analyze, clear, capture | `dumps.list`, `requests.list/get` (findings), `dumps.clear`, `requests.clear`, `dumps.enable` + `requests.enable` |
| insights | debug_start, debug_stop | not yet |
| logs | files, read | `logs.sites`, `logs.read` |
| processes | list, start, stop, output | `processes.*`; `process` is a process name (Hearth's are per site, not fixed to dev/vite/pail/queue) |
| services | list, available, install, start, stop | `services.*` (install picks the version line and port like Herd's) |
| workspaces | list, create, delete, merge | `workspaces.*` |
| runtime | status, php_versions, php_install, node_versions, node_install | `core.status`, `web.status`, `services.list`, `php.*`, `node.*` |
| env | get, set, unset, check, use_service | M12b ("Hearth's env tool comes in the next release: edit .env directly for now") |
| db | connection, list, create, use, drop | M12b |

## Review Focus

1. A site given as a name (case-insensitive), a `.test` host, or a path inside a site (also from `SITE_PATH` or the working folder) resolves to the deepest matching site. None gives Herd's error "No Hearth site matches [X]. Call `site list`, then pass the site name."
2. stdout never carries anything but protocol lines (no prints from the core client, panics go to stderr), and CRLF input lines are accepted.
3. Long calls (`mail wait` up to 300 s, `services install`, `workspaces create`) do not hit the IPC client's default timeout.
4. Destructive actions keep their guards: workspace delete only by name, and the services' and workspaces' own checks run in the core.
5. `mcp:install` writes the client config without losing other entries, refuses to overwrite a foreign `hearth` entry without `--force`, and never touches Herd's `herd` entry.

---

### Task 1: The protocol

**Files:** new `crates/hearth-cli/src/mcp/{mod.rs,protocol.rs}` + `tests/mcp.rs`; `main.rs` gets `Mcp` (`hearth mcp`).

- [ ] Write the tests: spawn `hearth mcp` with a test home and no core running.
  - `initialize` echoes the protocol version.
  - `tools/list` names the 10 tools: site, env, db, mail, insights, logs, processes, services, workspaces, runtime.
  - Every schema has `action` with an enum.
  - `tools/call` of `runtime status` without a core gives `isError` with "Hearth is not running".
  - An unknown method gives -32601; garbage gives -32700.
  - A notification gets no reply.
- [ ] Run and expect FAIL. Implement: a line loop on stdin, serde_json, one writer for stdout. Run and expect PASS. Commit "HEARTH-24: An MCP server speaking Herd's protocol".

### Task 2: Tools, actions and site resolution

**Files:** `mcp/tools.rs` (the table: tool, summary, actions with params), `mcp/site.rs` + unit tests.

- [ ] Write the tests:
  - The generated description of `site` lists every action with its required params, as Herd's does.
  - Missing and unknown action errors as in the Protocol section.
  - Site resolution follows Review Focus 1, against a temp home with `shop` linked and `api.shop` linked under it.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-24: Herd's tools and how they find the site".

### Task 3: Actions backed by the core

**Files:** `mcp/actions.rs` + an extended `tests/mcp.rs` (a real core on a test home, as in `tests/services.rs`).

- [ ] Write the tests:
  - `site list` and `site info` for a linked site (URL, path, PHP, HTTPS, workspaces).
  - `site isolate` then `unisolate`.
  - `mail list` / `show` / `links` on a mail stored in the test home.
  - `mail wait` returns a mail sent after the call, and times out after `timeout=1` with an error.
  - `logs files` and `read` on a `storage/logs/laravel.log`.
  - `processes list`.
  - `workspaces list`.
  - `runtime status`.
  - `env get` and `db list` answer the "not yet" error.
- [ ] Run and expect FAIL. Implement each action as a mapping onto IPC calls (with `call_waiting` for the long ones). Run and expect PASS. Commit "HEARTH-24: Herd's tool actions on Hearth's core".

### Task 4: Registration

**Files:** `mcp/install.rs` + tests; `main.rs` gets `mcp:install [--global|--project] [--agent=claude|cursor|vscode] [--path=] [--force] [--json]`.

- [ ] Write the tests on temp folders:
  - Project mode writes `.mcp.json` `mcpServers.hearth = {command:<hearth exe>, args:["mcp"]}` and keeps other entries (`herd` included).
  - VS Code uses `servers` with `type:"stdio"`.
  - Cursor writes `.cursor/mcp.json`.
  - A different existing `hearth` entry is refused without `--force`.
  - Global Claude Code (`claude mcp add --scope user hearth -- <exe> mcp`) is printed as the command to run, never executed in tests.
- [ ] Run and expect FAIL. Implement, run and expect PASS. Commit "HEARTH-24: Register Hearth's MCP server with agents".

### Task 5: Verification

- [ ] `cargo test --workspace` and clippy pass.
- [ ] Hands-on, with the owner's approval: register Hearth's MCP in a scratch project's `.mcp.json` (project scope, test home), then from a Claude Code session in that folder call `site info`, `mail list` and `runtime status`.
