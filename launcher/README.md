# lsuite (the launcher)

Part of [lsuite](https://lsuite.xyz). One native app to install, update, open and remove the
lsuite apps (ryolune, kimchi, zenith, nori, folio), to hold the **lsuite AI** account they all
share, and to manage **lsuite Cloud**, the storage that comes with an lsuite AI plan
([CLOUD.md](../CLOUD.md)). Written in Rust like the apps: the window is GPUI (the same pinned Zed
commit as kimchi, nori and folio) and wears the lsuite design system v2.

**Beta: Linux only.** lsuite is released for Linux x86_64 (AppImage and tar.gz) while it is in
beta; macOS and Windows are coming soon. Their code paths below stay in the source, but no builds
are made or shipped for them.

```
crates/lsuite-core     everything: the command registry (registry.rs), the app catalogue and its
                       release keys (catalog.rs), signed release lookup and verified downloads
                       (release.rs), install / update / remove / open (install.rs, apps.rs), the
                       shared lsuite AI account (account.rs), the lsuite Cloud client (cloud.rs),
                       settings and the event stream (events.rs)
crates/lsuite-desktop  the window (package and binary `lsuite`): store.rs, app.rs, views/
crates/lsuite-cli      `lsuite-cli <command> key=value…`
crates/lsuite-mcp      `lsuite-mcp`, the commands as MCP tools (stdio)
docs/COMMANDS.md       generated from the registry (`cargo run -p lsuite-cli -- docs`)
scripts/gen-mark.py    the mark and the app icon (brand/, resources/)
```

```bash
cargo run -p lsuite                      # the window
cargo run -p lsuite-cli -- apps.list     # or any command
cargo test --workspace                   # core tests, incl. install/update/remove end to end
```

## What it does

- **Apps.** Each app comes from its own GitHub releases. Nothing is installed unless it checks
  against the release key built into the launcher: kimchi, nori and folio sign each file with
  minisign (`latest.json`, the signature names the version); ryolune and zenith sign
  `SHA256SUMS` with Ed25519. Where apps go: `/Applications` (or `~/Applications`) on macOS,
  `~/.local/share/lsuite/apps/<app>/` on Linux (with an app-menu entry), the app's installer on
  Windows. A new version is unpacked next to the old one and swapped in; the old copy stays if
  anything fails. Open apps are never replaced or removed. Removing an app keeps its documents,
  settings and data. Copies built from source (outside the usual places) are shown, never
  touched. What is installed comes from the launcher's own record
  (`~/.lsuite/launcher/installed.json`), each app's discovery file (`~/.lsuite/apps/<app>.json`)
  and the usual places.
- **Account.** The same `~/.lsuite/account.json` every app reads (AI.md): signing in here signs in
  every lsuite app, through the browser (loopback, `app=lsuite`) or with a key. Shows the plan,
  the month's allowance, the cloud storage and the plans (chosen on lsuite.xyz).
- **Cloud.** Browse folders, upload files or whole folders (or drop them on the window),
  download files or folders (checked against their SHA-256), rename, move, new folder, delete
  with confirmation. Usage against the plan's quota.
- **Agents.** Every action is a command (`apps.*`, `account.*`, `cloud.*`, `settings.*`, `app.*`),
  the same from the window, `lsuite-cli` and `lsuite-mcp`. Agents are held to
  `settings.agent`: installing and cloud uploads are on, removing apps, deleting cloud files and
  signing in or out are off until the person turns them on (Settings › Agents).
  `claude mcp add lsuite -- /path/to/lsuite-mcp`.

## Environment

| Variable | Does |
| --- | --- |
| `LSUITE_HOME` | Replaces `~/.lsuite` (account, discovery files, the launcher's state). |
| `LSUITE_APPS_DIR` | Where apps are installed (and, on Linux, their menu entries and icons). |
| `LSUITE_ACCOUNT_SERVER` | The lsuite server (default `https://lsuite.xyz`; `http://127.0.0.1:4321` for a local site). |
| `LSUITE_GITHUB` | Another host for releases (tests). Signatures are still checked against the built-in keys. |
| `LSUITE_PLATFORM` | Pretend to be another platform (`macos-arm64`…), for tests. |
| `LSUITE_NO_UPDATE=1` | No release check at start. |
| `LSUITE_WINDOW_SIZE=1600x1000` | The window's size at start (screenshots). |
| `LSUITE_NO_PICKER=1` | Never open the system file picker (headless sessions): uploads ask for a path, downloads go to Downloads. |

Testing the window on Linux: `vscreen start target/debug/lsuite`, `vscreen shot`, with
`LSUITE_HOME` and `LSUITE_APPS_DIR` pointing at scratch folders.

## Synced folders

`cloud.syncAdd local=<folder> remote=<cloud folder>` (or **Sync a folder** on the Cloud page) keeps
a folder in step with the cloud both ways; the window syncs every few minutes
(`syncEveryMinutes`), `cloud.syncNow` any time. A file changed on both sides is kept twice
(`name (conflict <computer> <date>).ext`); deletions go across, but files deleted here because
they were deleted in the cloud are kept in `~/.lsuite/launcher/sync-trash/`; a first sync never
deletes; a sync that would delete more than half of a folder stops (`force=true` to go on).
Files only: empty folders aren't synced. See `crates/lsuite-core/src/sync.rs`.

## Updates of the launcher itself

`app.checkUpdates` / `app.installUpdate`, and a banner in the sidebar: the signed `latest.json`
of the latest `launcher-v*` release, files signed with the launcher's minisign key
(`selfupdate::PUBLIC_KEY`). The macOS bundle and the AppImage replace themselves; the Windows
installer runs once the launcher quits; portable copies (Windows zip, Linux tar.gz) link to the
new file.

## The lsuite agent

The **Agent** area (and `lsuite-cli agent.run prompt=…`) runs one agent across the apps
(HARNESS.md, part 8, `crates/lsuite-core/src/agent.rs`). It reads each app's expert brief and
skills (`harness.*`, or the app's MCP instructions and prompts on older versions), looks up the
commands it needs, does the work, looks at the result (`harness.look`) and fixes it before it
reports. It runs on **Claude Code** (every installed app's MCP server plus `lsuite mcp`, the suite
brief appended; no key needed), on **lsuite AI** (a Pass plan) or on the **Anthropic API**
(`ANTHROPIC_API_KEY`); with the last two the launcher runs the loop itself through MCP clients.
An app can still refuse an action for its own agent permissions; the agent says which setting.

## The marketplace

The **Marketplace** area and `market.*` (MARKETPLACE.md): browse, install and update plugins
(with lsuite Pass; files checked against the listing's SHA-256, unpacked into
`~/.lsuite/plugins/<app>/<id>/`, a running app asked to `plugin.rescan`), and publish a bundle
folder for review (`market.publish`, `agent.publish` for agents, off by default).

## Getting the apps

Since 0.2.0 the apps come through lsuite.xyz with the account (DISTRIBUTION.md): `apps.*` need a
free lsuite account; files are checked against the keys built into the launcher as before.

## Releasing

1. Set the version in `Cargo.toml` (`[workspace.package]`), `cargo run -p lsuite-cli -- docs`,
   commit and push to `main` of ludovic111/lsuite.
2. Build and sign with the suite release workflow (it holds `LSUITE_UPDATE_SIGNING_KEY`, and the
   Apple Developer ID and notarization secrets for when macOS returns):
   `gh workflow run suite-build.yml -R ludovic111/kimchi -f app=lsuite -f ref=<full commit SHA>`
   (Linux AppImage and tar.gz during the beta, each update file signed with `lsuite-release
   sign`; the macOS and Windows matrix lines are commented out until they ship).
3. Download the artifacts into one folder, write `latest.json` and the checksums, and publish:
   ```bash
   gh run download <run id> -R ludovic111/kimchi -D dist && mv dist/*/* dist/
   cargo run -p lsuite-release -- manifest dist --version X.Y.Z \
     --base-url https://github.com/ludovic111/lsuite/releases/download/launcher-vX.Y.Z --out dist/latest.json
   (cd dist && sha256sum lsuite-* latest.json > SHA256SUMS)
   gh release create launcher-vX.Y.Z -R ludovic111/lsuite --title "lsuite X.Y.Z" --notes-file notes.md dist/lsuite-* dist/latest.json dist/SHA256SUMS
   ```
   The site's `/launcher/download/<platform>` finds the newest `launcher-v*` release by itself.

The update key's secret half is the `LSUITE_UPDATE_SIGNING_KEY` secret of ludovic111/kimchi; the
owner keeps a copy outside Git. Losing it means the next release must ship a new key in a build
people install by hand.

## Limits

- Beta: Linux only. macOS and Windows are coming soon; the launcher has no builds for them yet.
- Apps installed by their Windows installers go where the installer puts them
  (`%LOCALAPPDATA%\<app>`), whatever `LSUITE_APPS_DIR` says.
- The file picker needs the system's portal on Linux; without one, uploads ask for a path
  (`LSUITE_NO_PICKER=1` forces that).
