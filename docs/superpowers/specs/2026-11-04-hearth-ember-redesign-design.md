# Hearth Ember redesign

## Why

The owner finds Hearth's UI standard: flat panels, and a sidebar that is one long list of 13 entries. Other developer apps look more professional, with atmosphere, illustrations and a clear structure. The redesign comes before M13 (migration from Herd), so new users meet the new look first. The owner gave the visual decisions to the designer ("make it better by your own decisions, think out of the box") and picked from mockups.

Success: Hearth looks like a premium, warm, crafted app the moment it opens; finding a page is quicker than in today's long list; nothing that works today stops working.

## Goals

- A distinctive look ("Ember"), consistent across every page.
- A shorter, grouped navigation.
- Illustrations that give the app character: the home screen, every empty page, the first start.
- A new app icon and tray icon.

## Non-goals

- No new features, IPC methods or behaviour changes. This is presentation only, apart from removing the light theme and theme switching. If the core stores a theme setting, it may stay there unused; no core change is needed for the redesign.
- No change to how pages work inside (tables, forms, actions), beyond restyling them and the regrouping listed below.
- No marketing website.

## Anti-goals (fails even if it technically works)

- Anything that looks templated or generic again.
- Coloured left-border accent rails on cards or rows (the owner finds them "too AI").
- Native `<select>` dropdowns, mixed button styles, or text cut off or wrapped awkwardly in rows (standing owner rules).
- Motion that is busy or bouncy, or that ignores the OS "reduce motion" setting.
- Illustrations with grey haze, smoke or halos on the dark background.

## Decided (by the owner, from mockups)

- **Direction A, "Ember":** warm dark. A deep brown-black background, with a soft orange glow rising from the bottom of the window and a fainter one at the top left. Panels are frosted glass with hairline borders. The accent is Hearth's ember orange.
- **Icon: "branches become flames", variant 4, polished.** A git graph with three branches rising like flames from a glowing base node, on a dark rounded tile with an ember glow.
  - File: `.superpowers/brainstorm/1369-1793821557/assets/icon-v4b.svg`, a vector, 2048 viewBox.
  - Generated with Higgsfield (Recraft), then hand-polished: transparent corners, one fire gradient, the glow clipped to the tile.
  - The polish script is `.superpowers/brainstorm/1369-1793821557/polish-icon.mjs`.
- **Home illustration: House 2.** A cottage, half stone and half server rack, with glowing windows, a terminal window and the branch flame from the chimney.
  - File: `.superpowers/brainstorm/1369-1793821557/assets/hero-2.png`, 2k, transparent.
  - Higgsfield job `4e3cd970-9fab-4f9f-806a-75d08c3c0282`. It is the style reference for every other illustration.
- **Empty-page illustrations**, all in House 2's style. Transparent 2k PNGs in `.superpowers/brainstorm/1369-1793821557/assets/`:

  | Page | File | Title / line / button |
  |---|---|---|
  | Sites | `empty-0.png` | No sites yet / Link a folder and Hearth serves it at name.test / Link a folder |
  | Workspaces | `empty-1b.png` | No workspaces / Give a branch its own folder, domain and database / New workspace |
  | Mail | `empty-2b.png` | The inbox is empty / Mail your sites send lands here / none |
  | Services | `empty-3.png` | No services yet / MySQL, Redis, Meilisearch and more, one click away / Add a service |
  | Processes | `empty-4.png` | Nothing running / Vite, queue workers and other long-running commands / Start a process |
  | Logs | `empty-5.png` | All quiet / No errors in your logs / none |
  | Inspect | `empty-6b.png` | Nothing captured yet / Dumps, requests and profiles show up as your sites run / none |
  | First start | `empty-7.png` (3:2) | Welcome to Hearth / Your sites, services and tools, warm and ready / Get started |

  The wording is the designer's draft and may be tightened. The illustrations are final, apart from the unlit House 2 layer described under Home.

## Navigation

Today there are 13 flat entries. The new rail has a logo at the top, then:

- **Home** (today's Dashboard)
- **Serve:** Sites, Workspaces
- **Run:** Services, Processes, Runtimes
- **Debug:** Mail, Inspect, Logs
- **Settings**, pinned to the bottom of the rail

Pages that merge:

- **Runtimes** holds today's PHP and Node pages as two tabs. PhpIni stays reachable from PHP, as today.
- **Inspect** holds today's Dumps, Requests and Profiles pages as three tabs.

Mail shows a badge with the number of unread mails. Existing deep links (`?view=php`, `?view=dumps`, and so on) must keep working by opening the merged page on the right tab.

## Home

- A greeting by time of day ("Good evening"), and one line of status.
- House 2 sits large at the right, on the bottom glow.
- Glass stat tiles: sites, services running, default PHP.
- **The windows of the house reflect running services.** This is the signature detail. *(Designer's calls, after the builder check.)*
  - **Two layers:** `hero-2.png` has every window lit, so it stays the lit layer. A second, unlit version of House 2 is made for the resting state: the same picture with every window and the rack LEDs dark, made by a Higgsfield edit with `hero-2.png` as the reference. If the edit doesn't line up pixel for pixel, the dark layer may be made by hand-retouching `hero-2.png` instead.
  - **Composition:** the dark house is the base. Each lit window is revealed from the lit layer through its own mask, and fades in over about 400 ms.
  - **The windows:**
    - The big terminal window lights when the web server is running (Hearth itself, always first).
    - The other real windows (the dormer, the left window and the door panes) light one per running service, in the order the services are listed.
    - With more running services than windows, the server rack's LEDs light up as well, so many services read as "busy".
    - With nothing running, the house is dark except for the chimney flame.
- **First start is Home's empty state, not a new screen or flag.** *(Designer's call.)* When there are no sites yet, Home shows `empty-7.png` with "Welcome to Hearth" and a "Get started" button that opens the existing Link-a-folder flow on Sites. Once a site exists, Home shows the house. No new state, settings field or core change.
- Everything today's Dashboard shows stays on Home.

## Look and feel details

- **Colour:** the existing tokens (`--bg`, `--surface`, `--accent`, ...) are retuned for Ember. The palette comes from the icon and illustrations:
  - `#17120f` darkest, `#2a221d` surface, `#5a4a40` stone;
  - `#c2361f` deep red, `#f07a40` ember, `#ffd27a` gold, `#ffe2a0` light gold.
  - Text and muted text stay within WCAG AA contrast on these.
- **Dark only (owner's decision):** Ember is the one and only theme. The light theme and theme switching go away: the theme setting in Settings, the system-theme following in `theme.ts`, and the light token set in `styles.css`. Native window parts (the title bar, scrollbars, form controls) are set to dark as well.
- **Type:** the current system font stack stays. Headings are tighter and heavier than today.
- **Buttons:** one shared look. The primary button is an ember gradient with dark text; the secondary is glass. The same in every page.
- **Dropdowns:** the existing custom `Select.svelte`, restyled as a glass panel.
- **Motion:**
  - The background glow breathes slowly, a slight change of brightness over several seconds.
  - Hover and active states fade in about 150 ms.
  - Pages cross-fade.
  - Nothing moves when the OS asks for reduced motion.
- **Tables and rows:** they stay tables (no switch to cards), with more breathing room, hairline separators and glass headers. No cut-off text.

## Icons

- **App icon:** generated from `icon-v4b.svg` into every size Tauri needs (`src-tauri/icons/*`, `.ico`, `.icns`) and the repo's `assets/logo.svg`.
- **Tray icon:**
  - A simplified glyph of the same branch flame, readable at 16 px.
  - Windows gets a coloured version; macOS gets a template (monochrome) version, as today with `tray.png` and `tray-template.png`.
  - The builder draws it from the icon's shapes.

## Constraints

- **Stack:** Svelte 5, Vite and Tauri 2, as today. No CSS framework and no new runtime dependencies without need.
- **Size and loading:**
  - The 2k PNGs are converted to optimised WebP at the size they are shown (about 2x display size), so the app bundle stays small.
  - Illustrations load lazily per page.
- **Behaviour:** every existing vitest test keeps passing; tests that assert on nav labels or views are updated to the new structure.
- **Writing:** no em or en dashes in UI text. English only.
- **Public repo:** the generated assets are the owner's (made on their Higgsfield account) and may be committed.

## Left to the builder

- Exact spacing, radii, shadows and the blur strength of the glass.
- How the glow is drawn (CSS gradients or an SVG layer) and how it breathes.
- The exact window masks and their positions.
- The tray glyph's exact drawing.
- The final wording of the empty-page texts, keeping their meaning.
- Whether the merged pages use the existing tab style or a new one, as long as it is Ember.
