# Hearth Ember Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle Hearth's UI as "Ember" (warm dark only), with a grouped menu, a living home screen, illustrated empty pages and the new icon.

**Architecture:**
- Presentation only, in `ui/src`.
- The theme machinery goes; one dark token set in `styles.css` carries Ember.
- The menu becomes data (`nav.ts`) rendered by a `Rail` component. PHP and Node merge into `Runtimes`, and Dumps, Requests and Profiles into `Inspect`, both through a shared `Tabs` component.
- Home layers a lit and an unlit house; the windows that light are decided by a pure function (`house.ts`).
- Illustrations ship as WebP made by a script.

**Tech Stack:** Svelte 5, Vite, Tauri 2, vitest (jsdom for component tests), `@resvg/resvg-js` (icons), `sharp` (new devDependency, illustrations only).

**Spec:** `docs/superpowers/specs/2026-11-04-hearth-ember-redesign-design.md`

## Global Constraints

- **Dark only:** no light theme, no theme switching. The core's `theme` setting stays in the API, unused.
- **No behaviour changes:** no new features, IPC methods or core changes.
- **Owner UI rules:**
  - Custom dropdowns only (`Select.svelte`), never a native `<select>`.
  - One shared button look.
  - No text cut off or wrapped awkwardly in rows.
  - No coloured left-border accent rails.
- **Motion:** slow and quiet. Hover and active fades about 150 ms, window lights 400 ms, page cross-fade. None of it under `prefers-reduced-motion: reduce`.
- **Palette** (from the spec): `#17120f` darkest, `#2a221d` surface, `#5a4a40` stone, `#c2361f` deep red, `#f07a40` ember, `#ffd27a` gold, `#ffe2a0` light gold. Text stays within WCAG AA.
- **UI text:** no em or en dashes, English only.
- **Deep links:** existing `?view=` values keep working.
- **Assets:** the source art lives in the git-ignored `.superpowers/brainstorm/1369-1793821557/assets/`. Only the processed WebP and SVG files are committed.

## Review Focus

1. **A legacy deep link opens the merged page on the right tab:** `?view=node` opens Runtimes on Node, `?view=profiles` opens Inspect on Profiles, and an unknown value opens Home. Pinned in Task 3.
2. **The house with 0 services or with many (9 running):** 0 leaves only the chimney flame, more than the windows lights the rack, nothing throws. Pinned in Task 6.
3. **Home while the core does not answer:** no welcome screen (that needs a loaded, empty site list), the status shows "Not responding", no crash. Pinned in Task 6.
4. **The window at its minimum size (760x520):** rail and page with no horizontal scroll and no cut-off text; the house scales down. Checked in Task 7's screenshots.
5. **Reduced motion:** the glow, the page fade and the window lights do not animate. Pinned in Task 2.

---

### Task 1: Dark only

**Files:**
- Delete: `ui/src/theme.ts`, `ui/src/theme.spec.ts`
- Modify:
  - `ui/src/App.svelte` (drop `applyTheme`, `resolveTheme`, `prefersDark` and both theme `$effect`s)
  - `ui/src/views/Settings.svelte` (drop the Theme row and `themes`)
  - `ui/src/styles.css` (one `:root` token set)
  - `src-tauri/tauri.conf.json` (window `"theme": "Dark"`, `"backgroundColor": "#120e0c"`)
- Test: `ui/src/dark.spec.ts`

- [ ] **Step 1: Write the failing tests** in `ui/src/dark.spec.ts`:
  - `the Settings page has no theme choice`: (jsdom) mount `Settings` with `createSettingsStore(api)`, flush, and expect `target.textContent` not to contain `'Theme'`, `'Light'` or `'System'`.
  - `there is one dark token set`: read `ui/src/styles.css` and expect no `data-theme` and a `color-scheme: dark`.
- [ ] **Step 2: Run the tests.** Run: `npx vitest run ui/src/dark.spec.ts`. Expected: FAIL (the Theme row exists, the light set exists).
- [ ] **Step 3: Implement.** In `styles.css`, write a single `:root { color-scheme: dark; ... }` using today's dark values; Task 2 retunes them. Remove `theme.ts` and its imports.
- [ ] **Step 4: Run.** Run: `npx vitest run` and `npm run check`. Expected: all pass, no unused imports.
- [ ] **Step 5: Commit** `HEARTH-26: Dark only, no theme switching`.

### Task 2: Ember foundation

**Files:**
- Modify: `ui/src/styles.css`, `ui/src/App.svelte` (shell and glow layer), `ui/src/Select.svelte` (glass panel)
- Test: `ui/src/dark.spec.ts`

**Interfaces:** produces these CSS tokens, which every later task uses:
- `--bg #120e0c`, `--surface rgb(255 255 255 / 0.05)`, `--surface-2 rgb(255 255 255 / 0.08)`, `--border rgb(255 255 255 / 0.09)`
- `--text #f3e9e1`, `--muted #a8998f`
- `--accent #f07a40`, `--accent-deep #c2361f`, `--gold #ffd27a`, `--accent-text #1a1310`
- `--ok #5cc58a`, `--danger #f08a73`
- `--glass-blur 12px`, `--radius 12px`

Utility classes: `.glass` (frosted panel), `.page-title`.

- [ ] **Step 1: Write the failing test** `motion stops when the OS asks`: `styles.css` contains `@media (prefers-reduced-motion: reduce)`, and inside it `animation: none` and `transition: none`.
- [ ] **Step 2: Run it.** Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **Glow:** a fixed `.glow` layer behind `main` in `App.svelte` with two radial gradients: ember at the bottom right, gold faint at the top left. It breathes through a `@keyframes` opacity change between 0.85 and 1 over 8 s.
  - **Buttons:** primary is an ember gradient (`#f6904f` to `#e8642c`) with dark text; the default is glass. Keep the `@layer base` structure so pages' own button styles still win.
  - **Inputs:** glass.
  - **Tables:** glass header, hairline row separators, about 10 px row padding.
  - **Select.svelte:** the open panel is glass with blur.
- [ ] **Step 4: Run.** Run: `npx vitest run` and `npm run check`. Expected: pass.
- [ ] **Step 5: Look.**
  - In the background, run `npx vite --port 1440` (the browser demo mode serves fake data).
  - For each of `sites` and `services`, screenshot `http://localhost:1440/?view=<view>` with headless Chrome (`--window-size=1100,720`) and read the PNG.
  - Expected: warm dark, the glow visible, readable text.
- [ ] **Step 6: Commit** `HEARTH-26: Ember look`.

### Task 3: The grouped menu

**Files:**
- Create: `ui/src/nav.ts`, `ui/src/nav.spec.ts`, `ui/src/Rail.svelte`, `ui/src/Tabs.svelte`, `ui/src/views/Runtimes.svelte`, `ui/src/views/Inspect.svelte`, `ui/src/app.spec.ts`
- Modify: `ui/src/App.svelte`

**Interfaces (produces):**
- `export type View = 'home' | 'sites' | 'workspaces' | 'services' | 'processes' | 'runtimes' | 'mail' | 'inspect' | 'logs' | 'settings'`
- `export const groups: { label: string | null; items: { id: View; label: string }[] }[]`. The order is:
  - `null` group: Home
  - Serve: Sites, Workspaces
  - Run: Services, Processes, Runtimes
  - Debug: Mail, Inspect, Logs

  Settings is not in `groups`; the rail pins it to the bottom.
- `export function resolveView(param: string | null): { view: View; tab: string | null }`
- `export function unreadCount(mails: { seen: boolean }[]): number`
- `Tabs.svelte` props: `{ tabs: { id: string; label: string }[]; active: string; onchange: (id: string) => void }`, with `role="tablist"` and `aria-selected`.
- `Runtimes.svelte` props: `{ tab?: 'php' | 'node' }`. `Inspect.svelte` props: `{ tab?: 'dumps' | 'requests' | 'profiles' }`. Both render the existing page components unchanged under `Tabs`.
- `App.svelte` exposes `navigate(view: View, opts?: { tab?: string; link?: boolean })` to the views that need it (Home, in Task 6) as an `onnavigate` prop.

- [ ] **Step 1: Write the failing tests.**
  - `nav.spec.ts`:
    - `resolveView`:
      - `'php'` gives `{view:'runtimes',tab:'php'}` and `'node'` gives `{view:'runtimes',tab:'node'}`.
      - `'dumps'`, `'requests'` and `'profiles'` give `inspect` with that tab.
      - `'dashboard'` gives `{view:'home',tab:null}`.
      - `'sites'` gives `{view:'sites',tab:null}`.
      - `'nope'` and `null` give `{view:'home',tab:null}`.
    - `unreadCount([{seen:true},{seen:false},{seen:false}])` is 2.
    - The labels across `groups` are exactly `['Home','Sites','Workspaces','Services','Processes','Runtimes','Mail','Inspect','Logs']`, and the group labels are `[null,'Serve','Run','Debug']`.
  - `app.spec.ts` (jsdom; set `history.replaceState(null,'','/?view=profiles')` before mounting `App`, then flush a few times):
    - the rail's `button[aria-current="page"]` reads `Inspect`;
    - `[role="tab"][aria-selected="true"]` reads `Profiles`;
    - the rail has 10 buttons including `Settings`.
- [ ] **Step 2: Run.** Run: `npx vitest run ui/src/nav.spec.ts ui/src/app.spec.ts`. Expected: FAIL (no module).
- [ ] **Step 3: Implement.**
  - **Rail:** the icon (`../../assets/logo.svg?url`, see Task 4), the groups with small uppercase labels, the active entry with a soft ember gradient behind it (no left rail), Settings and the version at the bottom.
  - **Mail badge:** the Mail entry shows `unreadCount` from `mailList()`, polled every 5 s, and is hidden at 0.
  - **Page fade:** pages cross-fade on change (about 150 ms, none under reduced motion).
- [ ] **Step 4: Run.** Run: `npx vitest run` and `npm run check`. Expected: pass.
- [ ] **Step 5: Commit** `HEARTH-26: Grouped menu, Runtimes and Inspect`.

### Task 4: Icon, tray and illustrations

**Files:**
- Replace: `assets/logo.svg` (from `.superpowers/brainstorm/1369-1793821557/assets/icon-v4b.svg`), `assets/tray.svg`, `assets/tray-template.svg`
- Regenerate: `src-tauri/icons/*`, `docs/logo.png` (`npm run icons`)
- Create:
  - `scripts/make-illustrations.mjs`
  - `ui/src/assets/illustrations/*.webp`: `home-lit`, `home-dark`, `sites`, `workspaces`, `mail`, `services`, `processes`, `logs`, `inspect`, `welcome`
  - `ui/src/illustrations.ts`, `ui/src/illustrations.spec.ts`
- Modify: `package.json` (devDependency `sharp`, script `"illustrations": "node scripts/make-illustrations.mjs"`)

**Interfaces (produces):** `export const art: Record<'homeLit'|'homeDark'|'sites'|'workspaces'|'mail'|'services'|'processes'|'logs'|'inspect'|'welcome', string>` (URLs, via `?url` imports).

- [ ] **Step 1: Make the unlit house.**
  - Higgsfield `generate_image`, model `gpt_image_2_5`, `quality: high`, `resolution: 2k`, `background: transparent`, `aspect_ratio: 3:2`.
  - Reference: `medias: [{ value: "4e3cd970-9fab-4f9f-806a-75d08c3c0282", role: "image_references" }]`.
  - Prompt: "Exactly the same picture as the reference, identical composition, camera, size and position of every element, but at night with every window, the terminal window, the door panes and all server rack LEDs completely dark and unlit; only the chimney flame still glows. No haze, transparent background, no text."
  - Fetch the result to the brainstorm `assets/` folder as `hero-2-dark.png`.
  - Overlay it on `hero-2.png` (headless Chrome page, 50 % opacity) and read the screenshot. If the outlines don't line up within a few pixels, retouch: make `hero-2-dark.png` from `hero-2.png` by darkening the window regions by hand (Node, `sharp` composite with dark-brown fills, `#2a221d`, over each window rectangle measured in Task 6). Ledger which way was used.
- [ ] **Step 2: Write the failing test** `illustrations.spec.ts`. For every key of `art`, the file `ui/src/assets/illustrations/<file>.webp` exists, is under 300 KB, and `sharp(...).metadata()` reports `hasAlpha: true` and `format: 'webp'`.
- [ ] **Step 3: Run it.** Expected: FAIL.
- [ ] **Step 4: Implement `scripts/make-illustrations.mjs <source dir>`.**
  - Map the sources to targets:

    | Source | Target |
    |---|---|
    | `hero-2.png` | `home-lit` |
    | `hero-2-dark.png` | `home-dark` |
    | `empty-0.png` | `sites` |
    | `empty-1b.png` | `workspaces` |
    | `empty-2b.png` | `mail` |
    | `empty-3.png` | `services` |
    | `empty-4.png` | `processes` |
    | `empty-5.png` | `logs` |
    | `empty-6b.png` | `inspect` |
    | `empty-7.png` | `welcome` |

  - Resize to 1400 px wide for `home-*` and `welcome`, 720 px for the others.
  - Encode WebP at quality 82 with alpha.
  - Run it, and add `illustrations.ts`.
- [ ] **Step 5: The icon.**
  - Copy `icon-v4b.svg` to `assets/logo.svg`.
  - Draw `assets/tray.svg` (64 viewBox): the three branch strokes and nodes only, no tile, in the fire gradient.
  - Draw `assets/tray-template.svg`: the same shapes in black on transparent.
  - Run `npm run icons`.
  - Render `tray.svg` at 16 px with resvg and read it. Expected: the three branches are tellable apart.
- [ ] **Step 6: Run.** Run: `npx vitest run`. Expected: pass.
- [ ] **Step 7: Commit** `HEARTH-26: New icon, tray icon and illustrations`, including the regenerated `src-tauri/icons`.

### Task 5: Illustrated empty pages

**Files:**
- Create: `ui/src/EmptyState.svelte`, `ui/src/empty-state.spec.ts`
- Modify (the "nothing yet" message only; the other `.empty` texts stay):
  - `views/Sites.svelte:111`
  - `views/Workspaces.svelte` (its empty list)
  - `views/Mail.svelte:130`
  - `views/Services.svelte:90`
  - `views/Processes.svelte:59`
  - `views/Logs.svelte:76` (only the `This log is empty.` case)
  - `views/Dumps.svelte`, `views/Requests.svelte:97`, `views/Profiles.svelte:35` (all with `art.inspect`)

**Interfaces:**
- Consumes `art` (Task 4).
- Produces `EmptyState.svelte` props: `{ art: string; title: string; line: string; action?: { label: string; onclick: () => void } }`.
- Sites gains the prop `startLinking?: boolean`. When true, the link form opens on mount, as the "Link a folder" button does today.

The texts, from the spec:

| Page | Title | Line | Button |
|---|---|---|---|
| Sites | No sites yet | Link a folder and Hearth serves it at name.test | Link a folder |
| Workspaces | No workspaces | Give a branch its own folder, domain and database | New workspace |
| Mail | The inbox is empty | Mail your sites send lands here | none |
| Services | No services yet | MySQL, Redis, Meilisearch and more, one click away | Add a service |
| Processes | Nothing running | Vite, queue workers and other long-running commands | Start a process |
| Logs | All quiet | No errors in your logs | none |
| Inspect pages | Nothing captured yet | Dumps, requests and profiles show up as your sites run | none |

Each button triggers the page's existing add action.

- [ ] **Step 1: Write the failing tests** in `empty-state.spec.ts` (jsdom).
  - Mount `EmptyState` with an action:
    - the image `src` is the given art and `alt=""`;
    - the title and line are shown;
    - clicking the button calls `onclick` once.
  - Without an action there is no button.
  - Mount `Sites` with `startLinking: true` (demo mode) and expect the link form's `Choose` button to be present.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.** The illustration is 220 px high, centred, over a soft ember glow, with `loading="lazy"`.
- [ ] **Step 4: Run.** Run: `npx vitest run` and `npm run check`. Expected: pass.
- [ ] **Step 5: Commit** `HEARTH-26: Illustrated empty pages`.

### Task 6: Home

**Files:**
- Create: `ui/src/house.ts`, `ui/src/house.spec.ts`, `ui/src/views/Home.svelte`, `ui/src/home.spec.ts`
- Delete: `ui/src/views/Dashboard.svelte` (its core status card moves into Home)
- Modify: `ui/src/App.svelte`

**Interfaces:**
- Consumes `art.homeLit`, `art.homeDark`, `art.welcome` and `navigate`.
- Produces:
  - `export type Win = { id: string; x: number; y: number; w: number; h: number }`, as percentages of the image.
  - `export const windows: Win[]`, with ids `'terminal'`, `'dormer'`, `'left'`, `'door'`.
  - `export const rack: Win`
  - `export function litWindows(webRunning: boolean, runningServices: number): { windows: string[]; rack: boolean }`
  - `export function greeting(hour: number): string`
- Home props: `{ onnavigate: (view: View, opts?: { tab?: string; link?: boolean }) => void }`

- [ ] **Step 1: Write the failing tests.**
  - `house.spec.ts`, for `litWindows`:

    | Call | Expected |
    |---|---|
    | `(false, 0)` | `{windows:[],rack:false}` |
    | `(true, 0)` | `{windows:['terminal'],rack:false}` |
    | `(true, 2)` | `{windows:['terminal','dormer','left'],rack:false}` |
    | `(true, 3)` | `{windows:['terminal','dormer','left','door'],rack:false}` |
    | `(true, 9)` | all four windows, `rack: true` |
    | `(false, 1)` | `{windows:['dormer'],rack:false}` |
    | `(true, -1)` | the same as `(true, 0)` |

  - `house.spec.ts`, for `greeting`:

    | Hours | Greeting |
    |---|---|
    | 5 to 11 | `'Good morning'` |
    | 12 to 17 | `'Good afternoon'` |
    | 18 to 22 | `'Good evening'` |
    | 23, 0 and 4 | `'Good night'` |

  - `home.spec.ts` (jsdom, demo api; poll a few times as in `processes-view.spec.ts`):
    - Home shows a greeting and the stat tiles: `4` sites from the demo data, the running-service count, and the default PHP `8.4` or whatever `phpList()` demo returns.
    - Make `api.sitesList` reject (`vi.spyOn`): no `Welcome to Hearth`, the core card still renders, no throw.
    - Make it resolve `{...demo, sites: []}`: `Welcome to Hearth` shows, and clicking `Get started` calls `onnavigate('sites', { link: true })`.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Measure the windows.**
  - Open `hero-2.png` in a headless Chrome page with a percentage grid overlay and read the screenshot.
  - Write the rectangles into `windows` and `rack`. `terminal` is the big screen window on the rack side; `dormer`, `left` and `door` are the others; `rack` covers the LED field.
- [ ] **Step 4: Implement Home.**
  - `home-dark` is the base image. Over it, for each `Win`, an absolutely positioned copy of `home-lit` clipped with `clip-path: inset(...)` to that rectangle, at opacity 1 when lit and 0 when not, with a 400 ms transition.
  - Poll `webStatus()`, `servicesList()`, `sitesList()` and `phpList()` every 5 s.
  - Show a greeting with one status line: "`N` sites, `M` services running".
  - Below it, glass stat tiles: Sites, Services running, PHP.
  - The core card from Dashboard stays.
  - The art sits right, scaling with the window width (`width: min(62%, 720px)`).
- [ ] **Step 5: Run.** Run: `npx vitest run` and `npm run check`. Expected: pass.
- [ ] **Step 6: Look.**
  - Screenshot `?view=home` at 1100x720 and 760x520, and read both.
  - Expected: the house with lit windows matching the demo state, no overflow.
- [ ] **Step 7: Commit** `HEARTH-26: Home with the living house`.

### Task 7: Polish and verification

**Files:** the page-level styles of all of `ui/src/views/*.svelte` (spacing, glass cards, headers). No behaviour changes.

- [ ] **Step 1: Screenshot every page.**
  - Pages: `home`, `sites`, `workspaces`, `services`, `processes`, `runtimes`, `mail`, `inspect`, `logs`, `settings`.
  - Take each at 1100x720 and at 760x520 (the minimum window) in the demo, and read every PNG.
  - Fix:
    - any light-theme leftovers (white panels, light hover colours);
    - cut-off text;
    - horizontal scroll;
    - mixed button styles;
    - native selects.
- [ ] **Step 2: Check the rules by search.**
  - `rg -n "<select" ui/src` gives no results.
  - `rg -n "[\x{2013}\x{2014}]" ui/src` gives no results.
  - `rg -n "border-left: *[0-9]+px solid var\(--accent" ui/src` gives no results.
- [ ] **Step 3: Verify.** Run: `npx vitest run`, `npm run check` and `npm run build`. Expected: all pass, and no warnings from svelte-check.
- [ ] **Step 4: Commit** `HEARTH-26: Ember polish on every page`.
