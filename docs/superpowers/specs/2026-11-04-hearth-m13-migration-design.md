# Hearth M13: Moving over from Herd, and Hearth 1.0.0

## Why

Hearth replaces Laravel Herd on the owner's machines. The owner wants to switch now, in one go, not site by site, and then uninstall Herd, but only once everything works under Hearth. For that, Hearth must run as a real installed app, so this milestone also ships Hearth 1.0.0, released the way the owner's Klecks app is.

**Success:** on the evening of the switch, the owner installs Hearth 1.0.0 and moves everything over in one guided run. Every site, proxy and service answers as it did under Herd. Herd gets uninstalled. Agents and the owner's tools keep working, and nothing has to be redone by hand.

The owner's actual Herd setup (counts, paths and project names) is in the git-ignored `docs/local/herd-footprint.md`. This public document stays generic.

## Goals

- **Move over:** a guided switch from Herd to Hearth on Windows, with a dry run first, then a check, then the uninstall.
- **Release:** Hearth 1.0.0 as a real installed app, plus the release routine for every version after it.

## Non-goals

- No side-by-side period and no site-by-site switching. That part of the original M13 entry is dropped by the owner's choice.
- No macOS move in this milestone: the owner switches on Windows. The import must not break on macOS, where it simply finds no Herd to move from.
- No code signing yet. Azure Trusted Signing and Apple notarization stay in M14, so Windows SmartScreen warns once when the unsigned 1.0.0 installer runs.
- No change to databases that run outside Herd. The owner's project databases live in a separate MariaDB on 3306, which Hearth must never touch.

## Anti-goals (a failure even if it technically works)

- **A broken site:** a site that answered under Herd and doesn't under Hearth after the switch.
- **An HTTPS warning:** the browser asks to trust a certificate again.
- **Uninstalling too early:** offering the Herd uninstall before the check passed, or Hearth deleting Herd's files itself.
- **Silent changes:** anything changed that the dry run didn't list.
- **Lost Redis data.**
- **The owner's other MariaDB:** anything touched on it.

## The move (decided with the owner)

### 1. Finding Herd

On start, Hearth looks for Herd (`~/.config/herd`). When it finds Herd and nothing has been moved yet, it offers "Move over from Herd". It does that on Home and in Settings, which keeps the entry for later.

### 2. Dry run

Hearth reads Herd's setup and shows every item with what will happen to it, changing nothing:

| What | From Herd | In Hearth |
|---|---|---|
| Parked folders | `config/valet/config.json` `paths` (the Sites folder itself excepted) | parked the same |
| Linked sites, aliases included | links in `config/valet/Sites` | linked the same, one site per link name |
| Proxies | per-site nginx configs with a `proxy_pass` in `config/valet/Nginx` | proxies to the same upstream |
| PHP per site | the nginx config's PHP socket (`$herd_sock_8x`; plain `$herd_sock` means the default) | isolated to that minor, or the default |
| PHP versions | `config/php.json` | the same minors installed from Hearth's builds; the same default; Herd's own php.ini changes shown and carried over |
| HTTPS | Herd's CA and leaf certificates in `config/valet/CA` and `Certificates` | copied into Hearth's home and used as they are, so the browser trusts them without asking |
| Hosts file | Herd's block of `.test` lines | Hearth's own block, the same hosts |
| Redis | the running Redis and its `dump.rdb` | Hearth's Redis on the same port with that data |
| Mail | the mail catcher's SMTP port | Hearth's catcher on that port |
| Herd's MariaDB | configured, not running | listed as "not moved" |
| Node | Herd's nvm versions | the same versions through Hearth's Node manager |
| PATH | Herd's entries | Hearth's `bin` instead |
| MCP | `herd` in `~/.claude.json` | Hearth's MCP server, registered under the same name `herd` so agents keep working |

Anything Hearth cannot move is listed with the reason, and isn't a reason to stop.

More rows, decided after the builder check:

| What | From Herd | In Hearth |
|---|---|---|
| Hosts that are no site | Herd workspaces, commit-hash subdomains, `herd-studio.test`, `database.herd.test` | dropped and listed as "not moved" (owner's decision: only the real sites move) |
| `www.` and `*.` of a site | Herd's nginx and certificates answer them | not carried over: Hearth serves the exact names, as today (owner's decision) |
| Database and Redis commands | `mysql`, `mysqldump`, `mariadb`, `redis-cli` reach the PATH only through Herd's entries | forwarders in Hearth's `bin`, to the separate MariaDB's own client programs (found where that service is installed; only run, never changed) and to Hearth's Redis *(designer's call)* |
| Herd's Node in the machine PATH | Herd's nvm entry, the `Program Files\nodejs` link, and `NVM_HOME` / `NVM_SYMLINK` | the machine PATH entries and variables are removed in the same single elevation and kept in the record for Undo; the link folder itself is left to Herd's uninstaller; the check confirms `node` comes from Hearth *(designer's call)* |
| Herd starting with Windows | Herd's start at login | switched off and kept in the record, so a reboot before the uninstall cannot bring Herd back onto Hearth's ports; Undo turns it back on *(designer's call)* |
| php.ini lines | Herd's `php.ini` per version | only the owner's own differences from stock PHP (such as `memory_limit`); lines that point into Herd (its dump loader, `C:\Program Files\Herd` extensions, its CA bundle) become Hearth's equivalents, or are listed as "not moved" when Hearth has none (for example an extension its builds lack) *(designer's call)* |

### 3. Moving

On "Move over":

1. Hearth stops Herd (it shuts down Herd's app and services), so ports are free.
2. Hearth carries out the list.
3. It keeps a record of everything it changed outside its own home: the hosts block, PATH, `~/.claude.json` and the certificate trust.

### 4. Check

Hearth checks every site, proxy and service:

- **The baseline:** the dry run records how every site answered under Herd. If Herd isn't running, the dry run starts it once for that recording, which changes nothing, and says so. The dry run then takes up to about a minute, not seconds *(designer's call)*.
- Each site and proxy must answer through Hearth over HTTP, and over HTTPS where it was secured. The status and the kind of answer must match that recording.
- Each moved service must be running.

The result is a list, green and red. The "Uninstall Herd" step only opens when everything is green or the owner accepts the red items one by one.

### 5. Undo

Until Herd is uninstalled, "Undo" puts everything back:

1. Hearth stops its moved sites and services.
2. It restores Herd's hosts lines, PATH entries and `herd` MCP entry from its record.
3. It starts Herd again.

### 6. Uninstalling Herd

"Uninstall Herd" starts Herd's own uninstaller; Hearth deletes nothing of Herd's itself. Afterwards Hearth runs the check again. If the uninstaller took Herd's CA out of Windows' trust, Hearth trusts its copy again, through the existing trust step, and the check must pass once more.

### 7. The owner's own files

The owner's global `~/.claude/CLAUDE.md`, their skills (phpstan, pest-tests, dev-workflow and any other that names Herd's paths) and their memory notes point at Herd's PHP and Composer (`.config\herd\bin\php.bat`, `php85.bat`, `composer.bat`).

- **Hearth doesn't touch them.** Right after the switch-over, Claude updates them in a session to Hearth's paths (`.config\hearth\bin\...`), as the owner asked.
- **The wizard's last screen** reminds the owner of this step.

## Hearth 1.0.0 (decided with the owner: "like Klecks")

The release follows the owner's Klecks app (`wl-lankin/klecks`, `docs/RELEASE.md` and `.github/workflows/release.yml`). Hearth's release workflow from M0 already has most of it. The changes:

- **Notes:** the release notes come from a `## X.Y.Z` section of a new `CHANGELOG.md` (else the tag message), as Klecks' `release-notes.sh` does. This replaces `docs/releases/<tag>.md`.
- **Draft:** the release stays a draft until the owner publishes it with `gh release edit vX.Y.Z --draft=false --latest`. Installed copies see the update only then.
- **Installers under fixed names:** `Hearth-Windows-Setup.exe` and `Hearth-macOS.dmg`, next to the versioned ones.
- **Prereleases:** a tag with a hyphen (`v1.1.0-beta.1`) is marked as a prerelease.
- **Routine:** `docs/RELEASE.md` describes the routine, as Klecks' does: where the updater key lives, the files in a release, the steps to make one, and the fixed links.
- **Version:** 1.0.0 everywhere (`package.json`, the Cargo files, `tauri.conf.json`; `scripts/check-version.mjs` already checks they match).
- **Installing on the owner's machine:**
  1. Once the draft is published, the owner installs `Hearth-Windows-Setup.exe`.
  2. The dev build and test homes stay out of the way: they use their own `HEARTH_HOME`.
  3. Then the move runs in the installed app.

## Constraints

- **Look:** Ember look, the owner's UI rules (custom dropdowns, one button look, no cut-off text, no left-border accent rails), no em or en dashes.
- **Admin rights:** writes that need them (the hosts file, machine-wide certificate trust) go through the existing one-time helper or a single elevation prompt, never one prompt per site.
- **Timing:** the dry run is read-only and takes up to about a minute (it records how every site answers).
- **Recovery:** the move must be recoverable. A crash halfway leaves the record, so Undo or a second "Move over" can finish or revert it.
- **Public repo:** tests use made-up Herd folders in a temp home, never the owner's real Herd.

## Left to the builder

- The wizard's screens and wording.
- How "the same answer" is compared in the check: status codes and redirect targets, not page bodies.
- How Herd is stopped.
- How the record is stored.
- How php.ini differences are shown.
- How to find Herd's uninstaller.
