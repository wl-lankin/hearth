# Hearth Tray Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Right-clicking Hearth's tray icon opens an Ember panel with PHP switching, services, sites, updates, Open and Quit, exactly like the approved mockup.

**Architecture:**
- A second Tauri window `panel` (transparent, borderless, always on top, no taskbar entry) loads its own page `ui/panel.html`, which renders `TrayPanel.svelte`.
- Rust places it at the tray icon with a tested pure function, resizes it to what the page measures, and hides it on blur.
- The panel talks to the core through the existing `api.ts`, and to the app through four new commands.

**Tech Stack:** Tauri 2 (Rust), Svelte 5, Vite multi-page build, vitest (jsdom).

**Spec:** `docs/superpowers/specs/2026-11-04-hearth-tray-panel-design.md`, with the mockup `docs/superpowers/specs/2026-11-04-hearth-tray-panel-mockup.html`.

## Global Constraints

- **Look:** exactly the mockup.
  - Panel 300 px wide, 14 px corners, Ember colours and the bottom glow.
  - The PHP flyout is a separate card to the left of the panel.
- **No new core methods.** Data comes from `phpList`, `phpUse`, `servicesList`, `servicesStart`/`servicesStop`, `sitesList`, `webStatus`, `openUrl`, `updateState`/`onUpdateState` and `installUpdateNow`.
- **Owner UI rules:** no native selects, one button look, no cut-off text, no em or en dashes. Nothing is selectable that is not an input.
- **Motion:** none under `prefers-reduced-motion: reduce`.
- **Platforms:** Windows and macOS. macOS transparency needs `app.macOSPrivateApi: true`.

## Review Focus

1. **Tray position:** a tray icon at the bottom right (Windows), at the top (macOS), or on a second monitor. The panel opens fully on screen, on the side of the icon with room. Pinned in Task 1.
2. **Opening and closing:** right-clicking while the panel is open, clicking elsewhere, or pressing Esc. The panel closes and never sticks open behind other windows. Checked by hand in Task 3.
3. **Without a core:** no crash and no endless spinner; the status says "Hearth core is not running". Pinned in Task 2.
4. **Many sites or services:** 40 sites give 5 rows plus "All 40 sites...". With 12 services, the panel height stays within the screen and the list scrolls. Pinned in Task 2.
5. **Flyout:** PHP with only one installed version, or none. The flyout still shows "Install another...", and with no PHP the row says "None". Pinned in Task 2.

---

### Task 1: The panel window in Rust

**Files:**
- Create: `src-tauri/src/panel.rs`
- Modify:
  - `src-tauri/src/tray.rs`: drop the system menu; a right-click opens the panel; a left-click keeps `window::toggle`.
  - `src-tauri/src/lib.rs`: register the commands; hide the panel on `Focused(false)`.
  - `src-tauri/src/commands.rs`
  - `src-tauri/tauri.conf.json`, `src-tauri/tauri.windows.conf.json`, `src-tauri/tauri.macos.conf.json`: add the `panel` window to each `windows` array, because the platform files replace the base array.
  - `src-tauri/capabilities/default.json`: `"windows": ["main", "panel"]`, plus `core:window:allow-set-size`, `core:window:allow-hide` and `core:event:default`.

**Interfaces (produces):**
- `pub struct Area { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }` (physical pixels).
- `pub fn place(icon: Area, panel_w: f64, panel_h: f64, work: Area) -> (f64, f64)`, which returns the panel's top-left corner:
  - The panel goes above the icon when the icon's centre is in the lower half of `work`, otherwise below it, with an 8 px gap.
  - Its right edge aligns with the icon's right edge.
  - The result is clamped fully inside `work`.
- Commands:
  - `panel_fit(app, width: f64, height: f64)`: logical size; resizes and re-places at the last icon.
  - `panel_hide(app)`
  - `app_open(app, view: Option<String>, tab: Option<String>)`: hides the panel, shows the main window and emits `hearth://navigate` `{view, tab}` to it.
  - `app_quit(app)`: calls `crate::quit`.
- The `panel` window config:
  - `url: "panel.html"`, `width: 300`, `height: 420`, `visible: false`.
  - `decorations: false`, `transparent: true`, `shadow: false`, `resizable: false`.
  - `alwaysOnTop: true`, `skipTaskbar: true`, `focus: true`.

- [ ] **Step 1: Write the failing Rust tests** in `panel.rs` (`#[cfg(test)]`). The work area is `{0,0,1920,1040}` unless a test says otherwise.

  | Test | Icon | Panel | Expected top-left |
  |---|---|---|---|
  | Windows, bottom right | `{1800,1044,24,36}` (in the taskbar, below the work area) | 300x400 | `(1524, 636)` |
  | macOS, top | `{1500,0,24,24}`, work `{0,25,1440,875}` | 300x400 | `x=1224`, `y=32`, below the icon |
  | Near the left edge | `{10,1044,24,36}` | 300x400 | `x` clamps to 0 |
  | Second monitor | work `{1920,0,2560,1400}`, icon `{4400,1404,24,36}` | 300x400 | inside x 1920..4480 |
  | Too tall | any icon | 300x2000 | `y` stays at the work area's top |

- [ ] **Step 2: Run them.** Run: `cargo test -p hearth-app panel`. Expected: FAIL (no `place`).
- [ ] **Step 3: Implement `place` and the panel module.**
  - Show: remember the icon's `rect` from `TrayIconEvent::Click { button: Right, button_state: Up, rect }`, place with the window's current outer size and the monitor's `work_area()`, show, then focus.
  - A second right-click while the panel is visible hides it.
  - Remove the system menu, so `tray::refresh` becomes a no-op. Instead, the panel learns about updates through `onUpdateState`.
- [ ] **Step 4: Run.** Run: `cargo test -p hearth-app panel` and `cargo clippy --workspace --all-targets -- -D warnings`. Expected: pass and clean.
- [ ] **Step 5: Commit** `HEARTH-27: The tray panel window`.

### Task 2: The panel page

**Files:**
- Create:
  - `ui/panel.html`, `ui/src/panel-main.ts` (mounts `TrayPanel` into `#panel`)
  - `ui/src/TrayPanel.svelte`
  - `ui/src/tray.ts`, `ui/src/tray.spec.ts`, `ui/src/tray-panel.spec.ts`
- Modify:
  - `vite.config.ts`: `build.rollupOptions.input` = `{ main: 'ui/index.html', panel: 'ui/panel.html' }`, relative to the root `ui`.
  - `ui/src/api.ts`: `panelFit`, `panelHide`, `appOpen`, `appQuit` (no-ops in the browser demo).
  - `ui/src/App.svelte`: listen for `hearth://navigate` and call `navigate(view, { tab })`.

**Interfaces (produces):**
- `export function siteUrl(site: { name: string; secured: boolean }, httpPort: number, httpsPort: number): string`. Same rule as the MCP server: the port is left out when it is 80 or 443.
- `export function shownSites<T extends { name: string }>(sites: T[], max = 5): { shown: T[]; more: number }`, sorted by name.
- `TrayPanel` props: `{ source?: PanelSource; actions?: PanelActions }`.
  - `source` defaults to the api module; `actions` default to the panel commands.
  - `PanelActions = { open(view?: string, tab?: string): void; quit(): void; fit(w: number, h: number): void; hide(): void }`
- The flyout widens the window: `fit(300 + 200 + 12, h)` while it is open, `fit(300, h)` otherwise.

- [ ] **Step 1: Write the failing tests.**
  - `tray.spec.ts`:
    - `siteUrl` gives `http://blog.test` (port 80), `https://shop.test:8443`, and `http://shop.test:8080`.
    - `shownSites` of 40 sites gives 5 rows, sorted, with `more: 40`.
  - `tray-panel.spec.ts` (jsdom, fake `source` and `actions` as `vi.fn()`, polled as in `home.spec.ts`):
    1. The status reads `Serving 4 sites`. The PHP row reads `8.4`. Each demo service shows a switch with `aria-checked`. The 4 demo sites show, with no "All" row.
    2. Clicking the PHP row shows the flyout with each installed version and `Install another...`.
       - Clicking `8.3` calls `phpUse('8.3')` and closes the flyout.
       - Clicking `Install another...` calls `actions.open('runtimes', 'php')`.
    3. Clicking a running service's switch calls `servicesStop(id)`; a stopped one's calls `servicesStart(id)`.
    4. Clicking `shop.test` calls `openUrl('https://shop.test:8443')`.
    5. With 40 sites: 5 rows and `All 40 sites...`, which calls `actions.open('sites')`.
    6. The update strip shows only for `{ phase: 'ready', version: '0.2.0' }`. `Restart to update` calls `installUpdateNow`.
    7. `Open Hearth` calls `actions.open()`; `Quit Hearth` calls `actions.quit()`; Esc calls `actions.hide()`.
    8. With all sources rejecting: `Hearth core is not running`, no throw.
    9. With no PHP installed: the row shows `None`, and the flyout still offers `Install another...`.
- [ ] **Step 2: Run them.** Run: `npx vitest run ui/src/tray.spec.ts ui/src/tray-panel.spec.ts`. Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Copy the mockup's look into `TrayPanel.svelte` with the shared tokens: head, sections with small labels, rows, switches, pill, update strip, footer and flyout.
  - The page body is transparent, and the panel card is right-aligned in the window.
  - A `ResizeObserver` on the card calls `actions.fit` with its size.
  - Refresh on `focus` and every 4 s while visible.
  - Services list: at most 6 rows high, then it scrolls.
  - Opening fades in over 120 ms.
- [ ] **Step 4: Run.** Run: `npx vitest run`, `npm run check` and `npm run build`. Expected: pass. `dist/panel.html` exists.
- [ ] **Step 5: Commit** `HEARTH-27: The tray panel page`.

### Task 3: Hands-on

- [ ] **Step 1: Start the app.** Use `HEARTH_HOME=<test home> npx tauri dev`, the same way as before.
- [ ] **Step 2: The owner checks:**
  - right-click opens the panel at the icon;
  - a click elsewhere, Esc, or a second right-click closes it;
  - PHP switches;
  - a service starts and stops;
  - a site opens in the browser;
  - Open Hearth and Quit Hearth work;
  - a left-click still shows the main window.
- [ ] **Step 3: Fix what they find** (TDD for anything testable), then commit.
