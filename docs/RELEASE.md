# Releases and automatic updates

Hearth keeps itself current without getting in the way. The installed app asks GitHub for `latest.json` 10 seconds after it starts and then every six hours, and downloads a newer version in the background. The signature is checked before anything is installed.

- **Windows:** the installer waits and runs when Hearth quits, or at once on "Restart to update".
- **macOS:** the new app bundle is put in place and is used from the next start, or at once on "Restart to update".

Updates can be turned off, and checked by hand, in Settings under Updates.

## The parts

| Part | Where |
|---|---|
| The updater | `src-tauri/src/updates.rs`, the page in `ui/src/views/Updates.svelte` |
| Feed URL, public key, Windows `passive` install | `src-tauri/tauri.conf.json`, section `plugins.updater` |
| The `hearth` command inside the app | `src-tauri/tauri.release.conf.json` (only in the pipeline) |
| Pipeline | `.github/workflows/release.yml` |
| Release notes | the `## X.Y.Z` section in `CHANGELOG.md`, else the tag message (`.github/release-notes.sh`) |
| Version check | `scripts/check-version.mjs`: `package.json`, `src-tauri/tauri.conf.json` and the root `Cargo.toml` must agree |

## The files in a release

Every installer is in the release twice: once with the version in its name, once under a fixed name for links.

| File | What for |
|---|---|
| `Hearth_X.Y.Z_x64-setup.exe` (+ `.sig`) | the Windows installer, also used by the updater |
| `Hearth-Windows-Setup.exe` | the same, fixed name |
| `Hearth_X.Y.Z_universal.dmg` | macOS, Apple Silicon and Intel |
| `Hearth-macOS.dmg` | the same, fixed name |
| `Hearth.app.tar.gz` (+ `.sig`) | the macOS update |
| `latest.json` | the feed the updater reads (`windows-x86_64`, `darwin-aarch64`, `darwin-x86_64`) |

Fixed links:

- https://github.com/wl-lankin/hearth/releases/latest/download/Hearth-Windows-Setup.exe
- https://github.com/wl-lankin/hearth/releases/latest/download/Hearth-macOS.dmg

## Where the updater key lives

- Private: `~/.tauri/hearth-updater.key`, its password in `~/.tauri/hearth-updater.key.password`. Only on the release machine and in the GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Never commit them.
- Public: `~/.tauri/hearth-updater.key.pub`, in `tauri.conf.json`.
- If the private key is lost, installed copies of Hearth accept no more updates. Keep the key and its password in the password manager as well.

## Making a release

1. Raise the version, the same everywhere: `package.json`, `src-tauri/tauri.conf.json` and the root `Cargo.toml` (then `cargo update --workspace` for `Cargo.lock`). `node scripts/check-version.mjs X.Y.Z` checks it.
2. Add a `## X.Y.Z` section at the top of `CHANGELOG.md`.
3. Commit, open a PR to `main`, and merge once CI is green.
4. Tag `main` and push the tag:
   ```bash
   git tag -a v1.1.0 -m "Hearth 1.1.0"
   git push origin v1.1.0
   ```
   A tag with a hyphen (`v1.1.0-beta.1`) becomes a prerelease; the version in the files stays plain (`1.1.0`).
5. Wait for the "release" run under Actions (Windows, then macOS, then the fixed names).
6. Check the draft (the files above, `latest.json` with a `signature` and `url` per platform) and publish it:
   ```bash
   gh release edit v1.1.0 --draft=false --latest
   ```
   Only then do installed copies see the update.

## Limits

- The installers are not code-signed yet (planned for M14). Windows SmartScreen warns on the first start. On macOS, open the app once with right click and Open, or run `xattr -dr com.apple.quarantine /Applications/Hearth.app`. Updates are not affected.
