# The lsuite standard

lsuite (always lowercase) is a free, open-source creative suite: **ryolune** (music),
**kimchi** (video), **zenith** (hub: projects, day, agents). Like a creative suite you pay for,
but MIT licensed, written in Rust, and **every app can be driven end to end by an AI agent**.

This file is the contract every app meets. Each app's `CLAUDE.md` has an "lsuite" section with
the gaps that app still has against it. When an app closes a gap, update its section and the
status table at the bottom of this file. Decided by the owner on 2026-10-01.

## 1. Everything is a command

- Every action a person can take in the window is a **named command** in one registry
  (`family.verb`, JSON parameters in, JSON result out, validated in one place). The window is
  one client of that registry, never a privileged one.
- The same registry is served to four clients: the window, the built-in agent, a CLI and an MCP
  server. CLI help, MCP tool list, agent tools and `docs/COMMANDS.md` are **generated** from it.
- A new feature lands as a command first, then the UI calls it. No private UI-only handlers for
  anything a script could want.
- Edits made by any client go into **the same undo history**. A batch of edits from an agent is
  one undo step.
- An overview command returns the whole document in one call (`session.overview` in ryolune).

## 2. Same shape of tools in every app

| Piece | Name | Notes |
| --- | --- | --- |
| App | `<app>` | The desktop window. |
| CLI | `<app>-cli` (or `<app>` subcommands when the app has no window binary) | `--file <doc>` works on a file without the app; without it, talks to the running app. |
| MCP | `<app>-mcp` | `--live` connects to the running app; otherwise works on a file. Installable with one line: `claude mcp add <app> -- <path>/<app>-mcp --live`. |
| Bridge | local loopback, token-protected | Token in the app's data folder, 0600. Never listens beyond 127.0.0.1. |
| Agent | built-in panel | Uses the model the person already has (Claude Code, Codex, API keys, local models). Shows one card per command, a list of changes with revert. |
| Permissions | `settings.agent.permissions` | Enforced for every agent and MCP request in one place; off means off for both. |

Ship `docs/AI_CONTROL.md` (how to drive the app) and the generated `docs/COMMANDS.md`.

## 3. Automatic updates

- The app checks for an update when it starts and offers it in one click (or installs it in the
  background where the platform allows), from **GitHub Releases** of its own repository.
- Updates are **signed** (Ed25519 / minisign or the Tauri updater key) and verified before
  anything is replaced; the previous copy is kept until the new one starts.
- `<APP>_NO_UPDATE=1` and a setting turn the check off. "Check for updates…" is also a command.
- Release assets use stable names per platform so `lsuite.xyz/<app>/download/<platform>` can
  find them (see `server.js` in the lsuite repo): macOS arm64 and x86_64 (signed and notarized),
  Windows x86_64, Linux x86_64.

## 4. Apps work together

The apps are separate programs but must be usable as one suite, by a person and by an agent.

- **Discovery.** Each app writes `~/.lsuite/apps/<app>.json` when it starts (version, path of
  the app, of `<app>-cli` and `<app>-mcp`, bridge port when running, data folder). zenith and the
  other apps read that folder to know what is installed and how to drive it. (To be designed in
  detail by the first app that implements it; keep the format small and versioned.)
- **Open formats, real files.** Documents are files on disk in a documented format (ryolune: one
  `.ryolune` JSON file with audio inside; kimchi: project JSON). Media goes between apps as plain
  files (WAV/FLAC/MP4…), never through a cloud.
- **Hand-offs** (first targets):
  - ryolune → kimchi: export a mix or stems straight onto a kimchi project's audio track.
  - kimchi → ryolune: send a cut's audio, length and markers to ryolune to score it.
  - zenith: shows each installed lsuite app (version, update available, open documents when
    running) and can delegate work to it through its MCP server.
- Shared vocabulary in command names where the concept is the same (`history.undo`,
  `history.redo`, `app.version`, `app.checkUpdates`, `session.overview` / `project.overview`,
  `export.*`).

## 5. Identity and site

- Name in lowercase everywhere. README starts with "Part of [lsuite](https://lsuite.xyz)" and links
  `https://lsuite.xyz/<app>`. GitHub "website" field is that page.
- Support/donate links go to `https://lsuite.xyz/<app>/support` (redirects to GitHub Sponsors).
  Donations only: nothing is ever sold, no account, no telemetry.
- The app's page lives in the lsuite repo (`<app>/index.html`). **Each release updates it**:
  version, what changed, screenshots (`assets/img/<app>/`), download notes.

## 6. Engineering

- Core in Rust (engine, document model, command registry, CLI, MCP). The UI can be web (Tauri)
  as long as it only renders what Rust returns.
- Offline first; network only for what the person turned on (model providers, updates).
- Secrets in the OS keychain or a 0600 file, never in logs or documents.

## Status (2026-10-01)

| | ryolune | kimchi | zenith |
| --- | --- | --- | --- |
| Command registry, one undo | ✅ | partly: edits are plain data in Rust, no named registry | partly: actions spread over API routes |
| CLI | ✅ `ryolune-cli` | partly: `kimchi generate / render` only | partly: HTTP `/api/context` |
| MCP | ✅ `ryolune-mcp --live` | ❌ | partly: read + a few actions |
| Built-in agent | ✅ | partly: generation only | ✅ |
| Signed auto-update | ✅ | ✅ (Tauri updater) | partly: updates from GitHub, no signed binary release |
| Release binaries, all platforms | ✅ notarized macOS | ✅ (macOS not notarized) | ❌ runs from source |
| Rust core | ✅ | ✅ | ❌ (Next.js; Rust port started on a branch) |
| Discovery (`~/.lsuite/apps`) | ❌ | ❌ | ❌ |
| Hand-offs | ❌ | ❌ | ❌ |
| README / site / support links | ✅ | ✅ | ✅ |
