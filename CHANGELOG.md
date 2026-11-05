# Changelog

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

The installers are not code-signed yet: Windows SmartScreen and macOS Gatekeeper warn the first time you open them.
