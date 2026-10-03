# The lsuite standard

lsuite (always lowercase) is a free, open-source creative suite: **ryolune** (music),
**kimchi** (video), **zenith** (code: a Mac app for coding with agents, coming soon). Like a creative suite you pay for,
but MIT licensed, written in Rust, and **every app can be driven end to end by an AI agent**.

| App | Repository | Local folder (next to this repo) |
| --- | --- | --- |
| ryolune · music · the reference implementation | [ludovic111/ryolune](https://github.com/ludovic111/ryolune) | `../ryolune` |
| kimchi · video | [ludovic111/kimchi](https://github.com/ludovic111/kimchi) | `../kimchi` |
| zenith · code · coming soon | [ludovic111/zenith](https://github.com/ludovic111/zenith) | `../zenith` |

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

- **Discovery.** Each app writes `~/.lsuite/apps/<app>.json` when it starts, and the others read
  that folder to know what is installed and how to drive it (`LSUITE_HOME` replaces `~/.lsuite`).
  Format 1 (kimchi's `kimchi-control/src/discovery.rs`, zenith's `zenith-commands/src/lsuite.rs`):
  `format` (1), `app`, `version`, `kind` (`music`, `video`, `code`), absolute paths `appPath`,
  `executable`, `cli`, `mcp` (the MCP server's program, run with `--live`), `dataDir`, optional
  `documents` (`{extensions, description}`), `running` (`{pid, port?, controlFile?, since}` while
  the app runs, else `null`; check the pid is alive) and `updatedAt`. Readers ignore unknown
  fields (zenith adds `bridge: {url, tokenFile}`) and files with another `format`.
- **Open formats, real files.** Documents are files on disk in a documented format (ryolune: one
  `.ryolune` JSON file with audio inside; kimchi: project JSON). Media goes between apps as plain
  files (WAV/FLAC/MP4…), never through a cloud.
- **Hand-offs** (first targets):
  - ryolune → kimchi: export a mix or stems straight onto a kimchi project's audio track.
  - kimchi → ryolune: send a cut's audio, length and markers to ryolune to score it.
  - zenith (decided 2026-10-02): lists the installed lsuite apps in its settings and hands their
    MCP servers to the agents of its threads, so an agent working in zenith can drive them.
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

## 7. One look: the lsuite design system

Every app wears the shared design system in `design/` (spec `design/DESIGN.md`, source
`design/tokens.json`, generated `design/tokens.css`, live preview at lsuite.xyz/design):

- **One signature color per app**, same OKLCH lightness and chroma, only the hue changes:
  ryolune teal (185°), kimchi chili coral (32°), zenith blue (262°); violet, green, amber and pink
  are reserved for the next apps. The accent means "yours or active"; state colors keep their meaning.
- **Frosted glass for the chrome** (three tiers over a backdrop tinted with the app color), solid
  surfaces for the work, native window vibrancy where the OS has it, opaque fallbacks when
  transparency is reduced.
- Manrope + IBM Plex Mono, shared radii, spacing and motion, dark and light, tested contrast.
- One app icon template (squircle in the app's gradient, white glyph, glass sheen).

## Status (2026-10-02)

| | ryolune | kimchi | zenith (coming soon) |
| --- | --- | --- | --- |
| Command registry, one undo | ✅ | ✅ 128 commands, one undo history | ✅ 52 `family.verb` commands (`zenith-commands`) shared by the window, CLI and MCP, git, pull requests and terminals included; undo is per turn (`thread.revert`) |
| CLI | ✅ `ryolune-cli` | ✅ `kimchi-cli` (running app or `--file`) | ✅ `zenith-cli` |
| MCP | ✅ `ryolune-mcp --live` | ✅ `kimchi-mcp --live` | ✅ `zenith-mcp --live`, and `/mcp` handed to each thread's agent |
| Built-in agent | ✅ | ✅ Agent panel (Claude Code, Codex, API keys, Ollama) | ✅ Claude Code and Codex threads |
| Signed auto-update | ✅ | ✅ own updater (same key and `latest.json` as the Tauri builds) | Coming soon: in-app updater, Ed25519-signed `SHA256SUMS`, built in the repo; nothing offered until zenith is ready |
| Release binaries, all platforms | ✅ notarized macOS | ✅ 0.4.0: notarized macOS (Apple Silicon, Intel), Windows, Linux | Coming soon: no download on the site (taken down 2026-10-02) until zenith is ready |
| Rust core | ✅ native window (GPUI) | ✅ native window (GPUI), no web UI | ✅ server (`crates/zenith-code`) and native window (GPUI, `crates/zenith-app`); React interface kept for the browser |
| Discovery (`~/.lsuite/apps`) | ✅ format 1 | ✅ writes `kimchi.json` (format 1) | ✅ writes `zenith.json`, reads the others (format 1) |
| Hand-offs | ✅ to and from kimchi (`export.toKimchi`, `session.scoreCut`, `handoff.inbox`) | ✅ to and from ryolune (`handoff.*`, through ryolune's bridge) | partly: hands the other apps' MCP servers to its agents |
| README / site / support links (site on the design system) | ✅ | ✅ | ✅ page marked Coming soon, no download |
| lsuite design system (`design/`) | ✅ GPUI on the tokens (glass over the window blur, teal, dark/light, contrast test, icon) | ✅ GPUI on the tokens: glass over the macOS window blur, Manrope/Plex, dark and light, contrast test, icon | ✅ native window (tokens, glass over vibrancy, Manrope/Plex, icon) and web interface (default theme on the tokens and glass tiers) |
