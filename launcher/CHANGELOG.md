# Changelog

## 0.3.0 (unreleased)

- **lsuite is fully free, with no account.** The apps install and update from lsuite.xyz without
  signing in (`LSUITE_SERVER` picks another server; signatures are checked as before). The
  Account area, sign-in and sign-out, `account.*`, plans and lsuite Pass are gone; an old
  `~/.lsuite/account.json` is ignored and left alone.
- **lsuite Cloud is gone**: the Cloud area, `cloud.*`, synced folders and their settings
  (`syncEveryMinutes`, `agent.cloudWrite`, `agent.cloudDelete`). Files you synced stay on this
  computer.
- **Plugins replace the Marketplace**: the Plugins area lists the plugins installed for each app
  (`~/.lsuite/plugins/<app>/<id>/`, from their `plugin.toml`) with Remove, and **Build a plugin**
  asks the lsuite agent to make one with the app's own plugin tools. `plugins.list` and
  `plugins.remove` replace `market.*`; publishing and installing from a store are gone, and so is
  `agent.publish`. Removing a plugin needs `agent.remove` for agents.
- **The lsuite agent** runs on Claude Code or the Anthropic API (`ANTHROPIC_API_KEY`); the lsuite
  AI option is gone.
- Shortcuts: Ctrl/Cmd-1 Agent, -2 Apps, -3 Plugins.
- **zenith is no longer an lsuite app**: the launcher no longer lists, installs or updates it.
  lsuite is now ryolune, kimchi, nori and folio. A copy of zenith that is already installed is
  left where it is.

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
