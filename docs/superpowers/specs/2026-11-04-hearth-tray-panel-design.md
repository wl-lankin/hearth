# Hearth tray panel

## Why

Hearth's tray icon has only a plain system menu: Open, the update item, and Quit. The owner uses Herd's tray for quick actions without opening the app. Those actions are switching PHP, starting services, opening sites, updates, opening the main window, and quitting completely. System menus can't take Hearth's look, and the owner wants it "awesome, fitting our design".

**Approved:** the owner approved the mockup `2026-11-04-hearth-tray-panel-mockup.html` (next to this file): "build it exactly like the preview".

## What it is

A right-click on the tray icon opens a small borderless panel in the Ember look, placed at the icon.

- **Placement:** above the icon on Windows, where the taskbar is at the bottom, and below it on macOS, where the menu bar is at the top. Generally, the panel opens on the side of the icon with more room.
- **Closing:** it closes when it loses focus or on Esc.
- **Left-click:** a left click on the tray icon keeps doing what it does today (show or hide the main window).

Top to bottom, as in the mockup:

1. **Head:** the logo, "Hearth", and a status line.
   - "Serving N sites" with a green dot while the web server runs.
   - A red dot and the problem when something is wrong.
   - A grey dot and "Hearth core is not running" without a core.
2. **PHP:** a row "Default version" showing the default minor, for example 8.4.
   - A click opens a list of the installed versions as a flyout to the left of the panel.
   - Each version shows its patch, and the default has an ember tick. One click makes a version the default and closes the flyout.
   - The last entry, "Install another...", opens the main window on Runtimes, PHP tab.
3. **Services:** every service has a status dot (green when running) and an Ember switch that starts or stops it. While a switch works, it shows that it is busy, and an error shows under the row.
4. **Sites:**
   - The first 5 sites by name. A click opens the site's URL in the browser, with HTTPS and the ports Hearth serves on.
   - Secured sites carry an "HTTPS" badge.
   - Proxies count as sites.
   - "All N sites..." opens the main window on Sites.
5. **Update strip:** only while an update has been downloaded, "Hearth X is ready" with "Restart to update".
6. **Footer:** "Open Hearth" (shows the main window) and "Quit Hearth" (quits completely, as the tray's Quit does today).

## Constraints

- **Look:** exactly the mockup.
  - Ember colours, the bottom glow inside the panel, 14 px corners, 300 px wide.
  - The flyout is a second floating card to the left.
- **Data:** it comes from the core methods the app already uses (php.list and php.use, services.*, sites.list, web.status, the update commands). No new core methods.
- **Freshness:** the panel refreshes when it opens and every few seconds while it's open.
- **Motion:** opening fades in quickly; nothing moves under reduced motion.
- **Owner UI rules:** no native selects, one button look, no cut-off text (long site names get the full row width; the panel is never wider than 300 px plus the flyout), no em or en dashes.
- **Windows and macOS:**
  - The panel is a transparent, borderless, always-on-top window without a taskbar entry.
  - macOS needs `macOSPrivateApi` for a transparent window. That is acceptable: Hearth isn't sold through the App Store.

## Left to the builder

The exact spacing, the refresh interval, how the window is sized to its content, and how the main window learns which page to open.
