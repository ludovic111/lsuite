# lsuite launcher commands

Generated from the registry (`cargo run -p lsuite-cli -- docs`); don't edit by hand.
Every command runs the same from the window, `lsuite-cli <command> key=value…` and `lsuite-mcp`
(tools are named with `_` for `.`: `apps_install`). Agents are held to `settings.agent`.

## apps

### `apps.list`

Every lsuite app: installed or not, its version, the latest one, whether it is open. `refresh` asks GitHub again (otherwise the last check is reused for 6 hours).

- `refresh` (true/false): Look for new versions now.

### `apps.check`

Looks for the latest release of every app now.

### `apps.install`

Downloads the app's latest release, checks its signature and installs it (or updates it if an older version is installed).

- `app` (text, required): The app: ryolune, kimchi, zenith, nori or folio.

Agents: needs `agent.install`.

### `apps.update`

Updates an installed app to its latest release (it must be closed).

- `app` (text, required): The app: ryolune, kimchi, zenith, nori or folio.

Agents: needs `agent.install`.

### `apps.updateAll`

Updates every installed app that has a newer release and isn't open.

Agents: needs `agent.install`.

### `apps.uninstall`

Removes the app (it must be closed). Its documents, settings and data folders stay.

- `app` (text, required): The app: ryolune, kimchi, zenith, nori or folio.

Agents: needs `agent.remove`.

### `apps.open`

Opens the app, with files if given.

- `app` (text, required): The app: ryolune, kimchi, zenith, nori or folio.
- `files` (list): Files to open in it.

### `apps.reveal`

Shows the installed app in the file manager.

- `app` (text, required): The app: ryolune, kimchi, zenith, nori or folio.

### `apps.page`

Opens the app's page on lsuite.xyz in the browser.

- `app` (text, required): The app: ryolune, kimchi, zenith, nori or folio.

## account

### `account.status`

The lsuite AI account on this computer: plan, allowance used, cloud storage.

### `account.signIn`

Signs in to lsuite (every app on this computer with it): opens the browser, or takes a key (lsk_…) made on the account page.

- `key` (text): A key from lsuite.xyz/account (for terminals).

Agents: needs `agent.account`.

### `account.cancelSignIn`

Stops waiting for a browser sign-in.

### `account.signOut`

Signs out here and on the server (every app on this computer with it).

Agents: needs `agent.account`.

### `account.plans`

lsuite AI's plans: prices, models, monthly allowance and cloud storage.

### `account.manage`

Opens the account page in the browser (plan, keys, connected apps).

## cloud

### `cloud.status`

lsuite Cloud: the plan's storage and how much is used.

### `cloud.list`

A folder of lsuite Cloud: its folders and files. `all` lists every file instead.

- `path` (text): The folder (the root when left out).
- `all` (true/false): Every file and folder, flat.

### `cloud.upload`

Uploads a file or a whole folder from this computer into a cloud folder, under its own name.

- `source` (text, required): The file or folder on this computer.
- `into` (text): The cloud folder (the root when left out).
- `overwrite` (true/false): Replace files already there (default false).

Agents: needs `agent.cloudWrite`.

### `cloud.download`

Downloads a cloud file or folder into a folder on this computer.

- `path` (text, required): The file or folder in the cloud.
- `into` (text, required): The folder on this computer.
- `overwrite` (true/false): Replace files already there (default false).

### `cloud.mkdir`

Creates a folder in lsuite Cloud.

- `path` (text, required): The new folder's path.

Agents: needs `agent.cloudWrite`.

### `cloud.move`

Moves a cloud file or folder (with its content) to another path.

- `from` (text, required): What to move.
- `to` (text, required): Its new path.
- `overwrite` (true/false): Replace what is there (default false).

Agents: needs `agent.cloudWrite`.

### `cloud.rename`

Renames a cloud file or folder in place.

- `path` (text, required): What to rename.
- `name` (text, required): The new name.

Agents: needs `agent.cloudWrite`.

### `cloud.delete`

Deletes a cloud file, or a folder and everything in it.

- `path` (text, required): The file or folder.

Agents: needs `agent.cloudDelete`.

### `cloud.syncList`

The folders kept in step with the cloud, and how their last sync went.

### `cloud.syncAdd`

Keeps a folder on this computer in step with a cloud folder, both ways (changes go either way; a file changed on both sides is kept twice). Syncs it once at once.

- `local` (text, required): The folder on this computer.
- `remote` (text): The cloud folder (default: the local folder's name at the top of the cloud).

Agents: needs `agent.cloudWrite`.

### `cloud.syncNow`

Syncs one synced folder now, or all of them.

- `id` (text): The synced folder (all when left out).
- `force` (true/false): Go on even if more than half of a folder would be deleted.

Agents: needs `agent.cloudWrite`.

### `cloud.syncRemove`

Stops syncing a folder. Its files stay on this computer and in the cloud.

- `id` (text, required): The synced folder.

Agents: needs `agent.cloudWrite`.

## settings

### `settings.get`

The launcher's settings.

### `settings.set`

Changes one setting (`theme`, `checkOnStart`, `autoUpdate`, `reduceTransparency`, `syncEveryMinutes`, `agent.*`). Agents can't change `agent.*`.

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
