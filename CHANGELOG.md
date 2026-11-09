# Changelog

## 2.0.1

- On a Mac, sites can use the usual ports 80 and 443: "Use 80 and 443" on the Sites page (admin password once) and they open as `http://shop.test`, without `:8080`. A system rule forwards only this computer's own 127.0.0.1 and ::1 to Stoke, and "Stop using 80 and 443" takes it away again
- Static sites and folders with an `index.html` answer again: `/` and `/docs/` served 404 because only `index.php` was looked for
- On a Mac, the "Move over from Herd" page no longer says Herd isn't installed: it points to `stoke import:herd`, which brings Herd's sites over
- The PHP builds for macOS now include mbstring's regular expressions, which Laravel needs (`Call to undefined function mb_split()`). PHP versions installed before get them by installing again: `stoke php:install <version>`

## 2.0.0

Hearth is now called **Stoke**: the old name sounded too much like Herd.

- On its first start, Stoke moves Hearth over by itself: it stops Hearth, moves `~/.config/hearth` to `~/.config/stoke` with every PHP and Node version, service, site, setting and the trusted certificate authority, and leaves a link at the old path so tools that still point there keep working
- The terminal PATH, Claude's and Cursor's MCP entries, the .test helper (Windows asks for permission once) and the "Start at login" entry move along, and Hearth's app is uninstalled; Home shows what is left if a step needs another try
- The command line tool is now `stoke` (the `herd`, `php`, `composer` and `laravel` commands stay as they were)

## 1.0.5

- The PHP page shows whether the Laravel installer is installed and which version, also after a restart; when it is there, the button updates it instead of installing it again
- The Home page fits the window: the house stays under the title strip and the core box sits at the bottom

## 1.0.4

- The Windows installer and the app are now digitally signed (publisher Wolfgang Linz), so Windows no longer warns about an unknown publisher

## 1.0.3

- Moving over from Herd also takes `C:\Program Files\nodejs\` out of the machine PATH when it is written with a trailing backslash, so `node` comes from Hearth afterwards
- The check after the move shows which Node really runs: the first one on the machine PATH, then the user PATH, as Windows looks it up

## 1.0.2

- Moving over from Herd sets up Hearth's helper service when it is missing (Windows asks for permission), instead of stopping with "The .test helper is not installed"
- A second "Continue the move" no longer stops at "Port 80 is still in use by another program" when that program is Hearth's own web server: ports are now read correctly on Windows in any language
- Update errors say what happened: no published release yet, or no connection to GitHub

## 1.0.1

- Moving over from Herd works: "Move over" and "Undo" now really start, instead of waiting at "Starting" forever

## 1.0.0

The first release of Hearth, a local PHP development environment for Windows and macOS.

- PHP 8.1 to 8.6 from Hearth's own builds, a default version, a version per site, and php.ini and extensions per version
- `.test` sites: parked folders, linked folders and proxies, with HTTPS from Hearth's own certificate authority
- Services: MySQL, MariaDB, PostgreSQL, Redis and more, each on its own port, started with Hearth if you like
- Node versions, with a default and a version per site
- Mail: every mail your sites send lands in Hearth's inbox
- Debugging: dumps, requests, Laravel logs, Xdebug per site and the SPX profiler
- Processes per site (`artisan dev`, Vite, queue workers) and workspaces, a copy of a site on its own branch and domain
- The `hearth` command, compatible with Herd's `herd`, and an MCP server so Claude and other agents see your sites, services, mail and logs
- A tray panel to switch the PHP version, start services, open sites and quit Hearth
- Moving over from Laravel Herd: Hearth reads Herd's setup, shows what it would change, moves your sites, PHP versions and settings, Redis data, mail port, Node versions, PATH and the MCP entry, checks every site against how it answered under Herd, and can go back until you uninstall Herd
- Hearth updates itself from these GitHub releases

The Windows installer is code-signed (publisher "Wolfgang Linz"); while the certificate is still new, SmartScreen can still ask once. The macOS app is not signed yet, so Gatekeeper warns the first time you open it.
