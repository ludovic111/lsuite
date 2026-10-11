# lsuite launcher commands

Generated from the registry (`cargo run -p lsuite-cli -- docs`); don't edit by hand.
Every command runs the same from the window, `lsuite-cli <command> key=value…` and `lsuite-mcp`
(tools are named with `_` for `.`: `apps_install`). Agents are held to `settings.agent`.

## apps

### `apps.list`

Every lsuite app: installed or not, its version, the latest one, whether it is open. `refresh` asks lsuite.xyz again (otherwise the last check is reused for 6 hours). No account is needed.

- `refresh` (true/false): Look for new versions now.

### `apps.check`

Looks for the latest release of every app now.

### `apps.install`

Downloads the app's latest release, checks its signature and installs it (or updates it if an older version is installed).

- `app` (text, required): The app: ryolune, kimchi, nori or folio.

Agents: needs `agent.install`.

### `apps.update`

Updates an installed app to its latest release (it must be closed).

- `app` (text, required): The app: ryolune, kimchi, nori or folio.

Agents: needs `agent.install`.

### `apps.updateAll`

Updates every installed app that has a newer release and isn't open.

Agents: needs `agent.install`.

### `apps.uninstall`

Removes the app (it must be closed). Its documents, settings and data folders stay.

- `app` (text, required): The app: ryolune, kimchi, nori or folio.

Agents: needs `agent.remove`.

### `apps.open`

Opens the app, with files if given.

- `app` (text, required): The app: ryolune, kimchi, nori or folio.
- `files` (list): Files to open in it.

### `apps.reveal`

Shows the installed app in the file manager.

- `app` (text, required): The app: ryolune, kimchi, nori or folio.

### `apps.page`

Opens the app's page on lsuite.xyz in the browser.

- `app` (text, required): The app: ryolune, kimchi, nori or folio.

## plugins

### `plugins.list`

The lsuite plugins installed for each app (`~/.lsuite/plugins/<app>/<id>/`): id, name, version, kind, description, from each bundle's plugin.toml. To make one, ask the lsuite agent (agent.run) to build it with the app's plugin.* commands.

- `app` (text): Only this app's plugins.

### `plugins.remove`

Removes an installed plugin (deletes its folder) and asks the app, if it is open, to let go of it.

- `app` (text, required): The app: ryolune, kimchi, nori or folio.
- `id` (text, required): The plugin's id (from plugins.list).

Agents: needs `agent.remove`.

## agent

### `agent.run`

Asks the lsuite agent to do a job, across the apps if it needs to (it reads each app's brief and skills, does the work, looks at it, reports). Waits until it's done.

- `prompt` (text, required): The job, in your words.
- `provider` (text): claude-code or anthropic (the first ready one when left out).

### `agent.stop`

Stops the lsuite agent.

### `agent.log`

The lsuite agent's conversation: what was asked, the tools it used, what it answered.

### `agent.clear`

Starts a new conversation with the lsuite agent.

### `agent.providers`

The ways the lsuite agent can run on this computer (Claude Code, Anthropic API).

## settings

### `settings.get`

The launcher's settings.

### `settings.set`

Changes one setting (`theme`, `checkOnStart`, `autoUpdate`, `reduceTransparency`, `agent.*`). Agents can't change `agent.*`.

- `key` (text, required): The setting's dotted name.
- `value` (any, required): Its new value.

## app

### `app.version`

The launcher's version, the computer's platform and where apps go.

### `app.checkUpdates`

Looks for a newer version of the launcher itself.

### `app.installUpdate`

Downloads the newer launcher, checks its signature and installs it; restart the launcher to use it.

Agents: needs `agent.install`.

### `app.commands`

Every command, with its parameters.
