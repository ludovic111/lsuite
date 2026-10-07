# Changelog

## 0.2.0

- **The lsuite agent**: one agent for jobs that span the apps (Agent area, `agent.*`). It reads
  each app's expert brief and skills, does the work through the app's tools, looks at the result
  and fixes it before reporting. Runs on Claude Code (no key needed), lsuite AI (with lsuite Pass)
  or the Anthropic API.
- **The lsuite Marketplace**: browse plugins for every app, install and update them (with lsuite
  Pass), publish your own (`market.*`); every version is reviewed before it's listed.
- **Apps come through lsuite**: installs and updates come from lsuite.xyz with your free lsuite
  account (signatures checked as before).
- lsuite AI's subscription is now **lsuite Pass** (AI, Cloud and the Marketplace).

## 0.1.1

- The cloud's demo limits come from the server (now 100 MB per account and 25 MB per file),
  instead of a fixed line in the window.
- Synced folders show their path from your home folder (`~/…`).

## 0.1.0

First release: install, update, open and remove ryolune, kimchi, zenith, nori and folio from
their signed releases; the lsuite AI account every app shares; lsuite Cloud with uploads,
downloads, folders and two-way synced folders; updates of the launcher itself; `lsuite-cli` and
`lsuite-mcp` (`lsuite mcp`) with agent permissions. macOS (signed and notarized), Windows
(installer or portable zip) and Linux (AppImage or tar.gz).
