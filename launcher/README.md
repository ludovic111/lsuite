# lsuite (the launcher)

Part of [lsuite](https://lsuite.xyz). One native app to install, update, open and remove the
lsuite apps (ryolune, kimchi, nori, folio), to see and remove the plugins installed for them and
build new ones with your agent, and to run the lsuite agent across them. Everything is free and
needs no account. Written in Rust like the apps: the window is GPUI (the same pinned Zed commit as
kimchi, nori and folio) and wears the lsuite design system v2.

**Beta: Linux only.** lsuite is released for Linux x86_64 (AppImage and tar.gz) while it is in
beta; macOS and Windows are coming soon. Their code paths below stay in the source, but no builds
are made or shipped for them.

```
crates/lsuite-core     everything: the command registry (registry.rs), the app catalogue and its
                       release keys (catalog.rs), signed release lookup and verified downloads
                       (release.rs), install / update / remove / open (install.rs, apps.rs), the
                       installed plugins (plugins.rs), the lsuite agent (agent.rs), settings and
                       the event stream (events.rs)
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

- **Apps.** Each app comes through lsuite.xyz, with no account (Getting the apps, below).
  Nothing is installed unless it checks against the release key built into the launcher: kimchi, nori and folio sign each file with
  minisign (`latest.json`, the signature names the version); ryolune signs
  `SHA256SUMS` with Ed25519. Where apps go: `/Applications` (or `~/Applications`) on macOS,
  `~/.local/share/lsuite/apps/<app>/` on Linux (with an app-menu entry), the app's installer on
  Windows. A new version is unpacked next to the old one and swapped in; the old copy stays if
  anything fails. Open apps are never replaced or removed. Removing an app keeps its documents,
  settings and data. Copies built from source (outside the usual places) are shown, never
  touched. What is installed comes from the launcher's own record
  (`~/.lsuite/launcher/installed.json`), each app's discovery file (`~/.lsuite/apps/<app>.json`)
  and the usual places.
- **Plugins.** For each installed app, the lsuite plugins in `~/.lsuite/plugins/<app>/<id>/`
  (PLUGINS.md), read from each bundle's `plugin.toml` (name, version, kind, description), with
  Remove (the folder is deleted after a confirmation; a running app is asked to `plugin.rescan`).
  **Build a plugin** asks the lsuite agent, in your words, to "Build a <app> plugin: …": it uses
  that app's `plugin.*` commands (guide, new, build, publishLocal) and the new plugin shows up here.
- **Agents.** Every action is a command (`apps.*`, `plugins.*`, `agent.*`, `settings.*`, `app.*`),
  the same from the window, `lsuite-cli` and `lsuite-mcp`. Agents are held to
  `settings.agent`: installing and updating apps is on, removing apps and plugins is off until
  the person turns it on (Settings › Agents).
  `claude mcp add lsuite -- /path/to/lsuite-mcp`.

## Environment

| Variable | Does |
| --- | --- |
| `LSUITE_HOME` | Replaces `~/.lsuite` (discovery files, plugins, the launcher's state). |
| `LSUITE_APPS_DIR` | Where apps are installed (and, on Linux, their menu entries and icons). |
| `LSUITE_SERVER` | Where the apps come from (default `https://lsuite.xyz`; `http://127.0.0.1:4321` for a local site). |
| `LSUITE_GITHUB` | Another host for the launcher's own releases (tests). Signatures are still checked against the built-in key. |
| `LSUITE_PLATFORM` | Pretend to be another platform (`macos-arm64`…), for tests. |
| `LSUITE_NO_UPDATE=1` | No release check at start. |
| `LSUITE_WINDOW_SIZE=1600x1000` | The window's size at start (screenshots). |
| `ANTHROPIC_API_KEY` | Lets the lsuite agent run on the Anthropic API (`LSUITE_AGENT_MODEL` picks the model). |

Testing the window on Linux: `vscreen start target/debug/lsuite`, `vscreen shot`, with
`LSUITE_HOME` and `LSUITE_APPS_DIR` pointing at scratch folders.

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
brief appended; no key needed) or on the **Anthropic API** (`ANTHROPIC_API_KEY`), where the
launcher runs the loop itself through MCP clients. An app can still refuse an action for its own
agent permissions; the agent says which setting.

## Getting the apps

The apps come only through the launcher (DISTRIBUTION.md), free and with no account. The launcher
asks `<server>/api/apps/<app>/latest` and downloads from `<server>/api/apps/<app>/files/<tag>/<name>`
(which sends it on to a short-lived address), with no Authorization header; `<server>` is
`LSUITE_SERVER`, else `https://lsuite.xyz`. Files are checked against the keys built into the
launcher, so the server can't change a build unnoticed. An `account.json` left in `~/.lsuite` by
launcher 0.2 is ignored.

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
- Building plugins needs the lsuite agent (Claude Code or `ANTHROPIC_API_KEY`) and Rust, which
  the app's plugin kit checks for.
