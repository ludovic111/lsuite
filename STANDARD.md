# The lsuite standard

lsuite (always lowercase) combines every kind of creative and office tool, free and open source:
**ryolune** (music), **kimchi** (video), **zenith** (code: an app for coding with agents),
**nori** (image and design: photo editing, vector illustration and page layout in one document)
and **folio** (office: documents, spreadsheets and presentations). Like the suites you pay for,
but MIT licensed, written in Rust, and **every app can be driven end to end by an AI agent**.

| App | Repository | Local folder (next to this repo) |
| --- | --- | --- |
| ryolune · music · the reference implementation | [ludovic111/ryolune](https://github.com/ludovic111/ryolune) | `../ryolune` |
| kimchi · video | [ludovic111/kimchi](https://github.com/ludovic111/kimchi) | `../kimchi` |
| zenith · code | [ludovic111/zenith](https://github.com/ludovic111/zenith) | `../zenith` |
| nori · image and design | `ludovic111/nori` (not published yet) | `../nori` |
| folio · office | `ludovic111/folio` (not published yet) | `../folio` |

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
  `format` (1), `app`, `version`, `kind` (`music`, `video`, `code`, `image`, `office`), absolute paths `appPath`,
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
- **The apps are free**, every feature, for everyone, with no account. The one thing lsuite sells
  is optional: an **lsuite AI** subscription (section 9). Still **no telemetry**: an app talks to
  lsuite only when the person signed in to lsuite AI and asked the agent something.
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

- **v2 (2026-10-06): black and white, cut square, with grain.** The accent is the ink of the mode
  for every app (a chosen thing is inverted), red only for what destroys or records, zero radii,
  hard offset shadows, film grain and dithered light behind the chrome, solid work surfaces.
- Every area titled like a sidebar, tools boxed by kind, switches always in view.
- Chakra Petch + IBM Plex Mono, shared spacing and motion, dark and light, tested contrast.
- App icons: the mark in one ink, white on a near-black tile with a corner of dithered light.
- ryolune wears v2 since 0.14; zenith moves to v2 in 0.4 (its new mark is a Z); nori and folio are
  born in v2.

## 8. Plugins

Every app has plugins: its stock plugins, the plugin formats of its trade it can really load
(each shown with its maker's logo), and plugins written in Rust that a person gets by asking their
agent. One **Plugins** area per app (Stock, Installed, Formats, Build with your agent), the same
`plugin.*` commands in every app, a frozen `repr(C)` SDK per app, bundles in
`~/.lsuite/plugins/<app>/`. The contract: [PLUGINS.md](PLUGINS.md).

## 9. lsuite AI

One account for the whole suite (`~/.lsuite/account.json`), signed in through the browser with a
loopback redirect, and an Anthropic-compatible endpoint on lsuite.xyz, so an app reaches it with
the Anthropic provider it already has. lsuite AI is the first provider in every agent ("No setup.
Sign in and your agent works."), with `account.*` commands; bringing your own stays free and is
never pushed aside, and a used-up allowance is said in one line, never a silent switch. A demo for
now: no payment is taken. The contract: [AI.md](AI.md).

## Status (2026-10-07)

All five apps are in beta on the site. nori and folio are being built (first builds coming); the
plugin and lsuite AI rows describe tonight's work in progress until each app's release says
otherwise.

| | ryolune | kimchi | zenith | nori | folio |
| --- | --- | --- | --- | --- | --- |
| Command registry, one undo | ✅ 239 commands | ✅ 250 commands | ✅ 82 commands; undo per turn | ✅ 147 commands | ✅ 150 commands |
| CLI | ✅ `ryolune-cli` | ✅ `kimchi-cli` | ✅ `zenith-cli` | ✅ `nori-cli` | ✅ `folio-cli` |
| MCP | ✅ `ryolune-mcp --live` | ✅ `kimchi-mcp --live` | ✅ `zenith-mcp --live` | ✅ `nori-mcp --live` | ✅ `folio-mcp --live` |
| Built-in agent | ✅ Agent panel | ✅ Agent panel | ✅ Claude Code and Codex threads | ✅ Agent panel | ✅ Agent panel |
| Discovery | ✅ `~/.lsuite/apps/ryolune.json` | ✅ `~/.lsuite/apps/kimchi.json` | ✅ `~/.lsuite/apps/zenith.json` | ✅ `~/.lsuite/apps/nori.json` | ✅ `~/.lsuite/apps/folio.json` |
| Open files | ✅ JSON, DAWproject | ✅ JSON projects | ✅ Git repositories | ✅ `.nori` ZIP, PSD, OpenRaster, SVG | ✅ `.folio` ZIP, Office and OpenDocument |
| Design v2 | ✅ | ✅ | ✅ native and web | ✅ | ✅ |
| Plugins | ✅ 35 stock, CLAP, VST3, AU, Rust SDK | ✅ audio, frei0r, LUTs, Rust SDK | ✅ Rust MCP tools | ✅ tile filters, Rust SDK | ✅ spreadsheet functions, Rust SDK |
| lsuite AI | ✅ demo | ✅ demo | ✅ demo through Claude Code | ✅ demo | ✅ demo |
| Updates | Release updater | Release updater | Release updater | Check and download; in-place updates pending | Check and download; in-place updates pending |
