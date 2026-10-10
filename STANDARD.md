# The lsuite standard

lsuite (always lowercase) combines every kind of creative and office tool, free and open source:
**ryolune** (music), **kimchi** (video), **nori** (image and design: photo editing, vector illustration and page layout in one document)
and **folio** (office: documents, spreadsheets and presentations). Like the suites you pay for,
but MIT licensed, written in Rust, and **every app can be driven end to end by an AI agent**.

| App | Repository | Local folder (next to this repo) |
| --- | --- | --- |
| ryolune · music · the reference implementation | [ludovic111/ryolune](https://github.com/ludovic111/ryolune) | `../ryolune` |
| kimchi · video | [ludovic111/kimchi](https://github.com/ludovic111/kimchi) | `../kimchi` |
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
  background where the platform allows), **through lsuite.xyz**, with nothing to sign in to
  (DISTRIBUTION.md, decided 2026-10-07, free since 2026-10-10): kimchi, nori and folio read
  `<server>/api/apps/<app>/latest.json`, ryolune `<server>/api/apps/<app>/releases/latest`
  and its `SHA256SUMS(.sig)`. `<server>` is `LSUITE_SERVER`, else
  `https://lsuite.xyz`; no token is sent. The builds live in the private `ludovic111/lsuite-builds` (tag
  `<app>-v<version>`), where the release workflow uploads them.
- Updates are **signed** (Ed25519 / minisign or the Tauri updater key) and verified before
  anything is replaced; the previous copy is kept until the new one starts.
- `<APP>_NO_UPDATE=1` and a setting turn the check off. "Check for updates…" is also a command.
- Release assets use stable names per platform so the lsuite app finds them (`launcher/` in the
  lsuite repo): Linux x86_64 and macOS (arm64 and x86_64, built on the Mac mini's runner, signed
  and notarized when the Apple secrets are set) during the beta; Windows x86_64 is coming soon,
  and no builds are made for it until then. The
  existing `<APP>_UPDATE_URL` overrides keep working for tests.

## 4. Apps work together

The apps are separate programs but must be usable as one suite, by a person and by an agent.

- **Discovery.** Each app writes `~/.lsuite/apps/<app>.json` when it starts, and the others read
  that folder to know what is installed and how to drive it (`LSUITE_HOME` replaces `~/.lsuite`).
  Format 1 (kimchi's `kimchi-control/src/discovery.rs`):
  `format` (1), `app`, `version`, `kind` (`music`, `video`, `image`, `office`), absolute paths `appPath`,
  `executable`, `cli`, `mcp` (the MCP server's program, run with `--live`), `dataDir`, optional
  `documents` (`{extensions, description}`), `running` (`{pid, port?, controlFile?, since}` while
  the app runs, else `null`; check the pid is alive) and `updatedAt`. Readers ignore unknown
  fields and files with another `format`.
- **Open formats, real files.** Documents are files on disk in a documented format (ryolune: one
  `.ryolune` JSON file with audio inside; kimchi: project JSON). Media goes between apps as plain
  files (WAV/FLAC/MP4…), never through a cloud.
- **Hand-offs** (first targets):
  - ryolune → kimchi: export a mix or stems straight onto a kimchi project's audio track.
  - kimchi → ryolune: send a cut's audio, length and markers to ryolune to score it.
- Shared vocabulary in command names where the concept is the same (`history.undo`,
  `history.redo`, `app.version`, `app.checkUpdates`, `session.overview` / `project.overview`,
  `export.*`).

## 5. Identity and site

- Name in lowercase everywhere. README starts with "Part of [lsuite](https://lsuite.xyz)" and links
  `https://lsuite.xyz/<app>`. GitHub "website" field is that page.
- Support/donate links go to `https://lsuite.xyz/<app>/support` (redirects to GitHub Sponsors).
- **The apps are free**, every feature, for everyone. They are **downloaded through lsuite**: the
  ready-made builds come only through the lsuite app (section 10), with **no account**
  (DISTRIBUTION.md); the source stays open, and anyone may build an app from it. The app's page
  says "Get <app> in the lsuite app" and `lsuite.xyz/<app>/download` leads to `/launcher`. lsuite sells
  nothing (since 2026-10-10: no plan, no account). Still **no telemetry**: an app talks to lsuite
  only to check for updates (section 3).
- The app's page lives in the lsuite repo (`<app>/index.html`). **Each release updates it**:
  version, what changed, screenshots (`assets/img/<app>/`), notes on getting it.

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
- ryolune wears v2 since 0.14; nori and folio are born in v2.

## 8. Plugins

Every app has plugins: its stock plugins, the plugin formats of its trade it can really load
(each shown with its maker's logo), and plugins written in Rust that a person gets by asking their
agent. One **Plugins** area per app (Stock, Installed, Formats, Build with your agent), the same
`plugin.*` commands in every app, a frozen `repr(C)` SDK per app, bundles in
`~/.lsuite/plugins/<app>/`. The contract: [PLUGINS.md](PLUGINS.md). A bundle is a folder: people share it
like any file and install it with `plugin.install`; the launcher lists and removes the installed
ones (the app picks changes up on `plugin.rescan`).

## 9. No account, no plan

Since 2026-10-10 lsuite is entirely free: no lsuite account, no lsuite Pass, no lsuite AI provider,
no lsuite Cloud, no marketplace. An app has no `account.*` commands, no sign-in, and its agent's
providers are the person's own (Claude Code, Codex, API keys, local models). An old
`~/.lsuite/account.json` is ignored, never deleted.

## 10. The launcher

**lsuite** (`launcher/` in the lsuite repo, started 2026-10-07) is the suite's own native app, in
Rust and GPUI like the apps: it installs, updates, opens and removes the four apps from their
signed builds, the only way to get them (through lsuite.xyz, no account, DISTRIBUTION.md; the
release keys are built in), and, from 0.3.0, lists every app's installed plugins and hands plugin
requests to the right app's agent. It reads the discovery files of section 4 and never
touches a copy it didn't install outside the usual places. It meets this standard's shape: one
command registry, `lsuite-cli`, `lsuite-mcp` (agents held to `settings.agent`), design v2. Released
as `launcher-vX.Y.Z` in the lsuite repo (0.1.1 is the latest), signed with its own key.

## Status (2026-10-07)

All four apps have public beta releases: ryolune 0.15.3, kimchi 0.10.0, nori 0.1.0 and
folio 0.1.0, with plugins. Those releases still carry the lsuite account and lsuite AI sign-in;
the next ones remove them (lsuite is free since 2026-10-10).

| | ryolune | kimchi | nori | folio |
| --- | --- | --- | --- | --- |
| Command registry, one undo | ✅ 239 commands | ✅ 250 commands | ✅ 156 commands | ✅ 150 commands |
| CLI | ✅ `ryolune-cli` | ✅ `kimchi-cli` | ✅ `nori-cli` | ✅ `folio-cli` |
| MCP | ✅ `ryolune-mcp --live` | ✅ `kimchi-mcp --live` | ✅ `nori-mcp --live` | ✅ `folio-mcp --live` |
| Built-in agent | ✅ Agent panel | ✅ Agent panel | ✅ Agent panel | ✅ Agent panel |
| Discovery | ✅ `~/.lsuite/apps/ryolune.json` | ✅ `~/.lsuite/apps/kimchi.json` | ✅ `~/.lsuite/apps/nori.json` | ✅ `~/.lsuite/apps/folio.json` |
| Open files | ✅ JSON, DAWproject | ✅ JSON projects | ✅ `.nori` ZIP, PSD, OpenRaster, SVG, IDML, PDF-compatible AI, 8-bit XCF | ✅ `.folio` ZIP, Office and OpenDocument |
| Design v2 | ✅ | ✅ | ✅ | ✅ |
| Plugins | ✅ 35 stock, CLAP, VST3, AU, Rust SDK | ✅ audio, frei0r, LUTs, Rust SDK | ✅ tile filters, Rust SDK | ✅ spreadsheet functions, Rust SDK |
| No account (free) | 🟡 next release | 🟡 next release | 🟡 next release | 🟡 next release |
| Updates | Release updater | Release updater | Signed release updater | Signed release updater |
| Updates through lsuite.xyz | 🟡 0.16.0 in review | 🟡 0.11.0 in review | 🟡 0.2.0 in review | 🟡 0.2.0 in review |
| Agent harness ([HARNESS.md](HARNESS.md)) | 🟡 13 skills, looks with loudness ([#34](https://github.com/ludovic111/ryolune/pull/34)) | 🟡 13 skills, frame sheets with loudness ([#16](https://github.com/ludovic111/kimchi/pull/16)) | 🟡 11 skills, contrast, bleed and resolution checks ([#1](https://github.com/ludovic111/nori/pull/1)) | 🟡 12 skills, pages, slides and sheets as images ([#1](https://github.com/ludovic111/folio/pull/1)) |
| Evals | 🟡 13 jobs; 2 run, passed | 🟡 12 jobs; 3 run, passed | 🟡 12 jobs; 2 run, passed | 🟡 11 jobs; 2 run, passed |

While lsuite is in beta, every app ships for Linux and macOS (Linux only from 2026-10-08, macOS
back on 2026-10-10): AppImages update in place, macOS app bundles are replaced from their
`.app.tar.gz`, and tarballs and Linux system packages link to verified release downloads. Windows
is coming soon. nori’s physical tablet and Adobe interoperability checks
remain beta validation limits, detailed in its release notes.
