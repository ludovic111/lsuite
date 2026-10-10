# Plugins in lsuite apps

Decided by the owner on 2026-10-06: every lsuite app has plugins: its own stock plugins, the
plugin formats of its trade it can load, and **plugins written in Rust that a person gets by asking
their agent**. This file is the contract; each app's CLAUDE.md says how it meets it.

## What a person sees

One **Plugins** area per app (a window or a panel, titled like every area), four parts:

| Part | Shows |
| --- | --- |
| **Stock** | The plugins that ship with the app, by kind (each with a one-line description). |
| **Installed** | Plugins found on this machine: lsuite plugins and the compatible formats, each with its format's real logo, vendor, version, an on/off switch always in view, and Remove for lsuite plugins. Rescan. |
| **Formats** | What the app can load, with each format's or maker's real logo (ryolune: CLAP, VST3, Audio Units; kimchi: frei0r, `.cube` LUTs, ryolune effects; …) and where it looks. Nothing listed that the app can't actually load. |
| **Build with your agent** | One field: "Describe the plugin you want". Sending it starts the app's agent on the plugin recipe below; progress shows as the agent's cards; the new plugin appears in Installed, loaded, without a restart. Also shows whether Rust is installed and offers to install it (rustup, with the person's consent). |

## Commands (same names in every app)

| Command | Does |
| --- | --- |
| `plugin.list` | Stock, installed and disabled plugins with id, name, kind, format, version, path, enabled. |
| `plugin.info {id}` | One plugin: parameters, description, where it came from. |
| `plugin.enable` / `plugin.disable {id}` | The switch (a setting; never deletes). |
| `plugin.rescan` | Scans the folders again and reloads lsuite plugins that changed (hot reload). |
| `plugin.install {path}` | Copies a built plugin bundle (folder with `plugin.toml` and the library) into the app's plugin folder and loads it. |
| `plugin.remove {id}` | Removes an installed lsuite plugin (stock and other formats can only be disabled). |
| `plugin.guide` | Markdown for an agent: the SDK, the kinds, the manifest, an example, the rules, the recipe below. Kept in step with the SDK by a test. |
| `plugin.toolchain` | `{cargo, rustc, version, ok, installHint}`. |
| `plugin.new {name, kind}` | Scaffolds a crate from the SDK template in `~/.lsuite/plugins-src/<app>/<name>/`, returns its path and files. |
| `plugin.writeSource {name, path, contents}` | Writes one file inside that crate only (so agents without file tools can write plugins; paths outside the crate are refused). |
| `plugin.build {name}` | `cargo build --release` in the crate; returns `ok` and the compiler's errors as `{file, line, message}` (rendered), never a wall of text. |
| `plugin.publishLocal {name}` | Build output → plugin bundle → `plugin.install`. |

Agent permissions: building and installing plugins are a permission of their own
(`settings.agent.permissions.plugins`), off for API agents until the person allows it.

## The recipe an agent follows

1. `plugin.guide`, `plugin.toolchain` (offer the install when Rust is missing).
2. `plugin.new {name, kind}`.
3. Write the code (`plugin.writeSource`, or the agent's own file tools inside the crate).
4. `plugin.build` until it is green; fix from the structured errors.
5. `plugin.publishLocal`, then try it (apply it to the selection, render a frame or a bar, look).

## Files

- A plugin bundle is a folder: `plugin.toml` + the library (`.dylib` / `.so` / `.dll`).
  `plugin.toml`: `id` (reverse-DNS), `name`, `version`, `app`, `kind`, `abi`, `description`,
  `authors`, `[library] macos/linux/windows` file names.
- Installed lsuite plugins live in `~/.lsuite/plugins/<app>/<id>/` (`LSUITE_HOME` replaces
  `~/.lsuite`); sources the agent writes in `~/.lsuite/plugins-src/<app>/<name>/`.
- The SDK is a crate in the app's repository (`ryolune-plugin` in `sdk/`, `kimchi-plugin`,
  `nori-plugin`, `folio-plugin`) with a frozen `repr(C)` ABI (an ABI version in the entry point,
  checked before anything is called, calls panic-guarded) — ryolune's `sdk/src/ffi.rs` is the
  reference. The template's `Cargo.toml` points at the SDK by git URL and tag, so a plugin builds
  without the app's sources.
- A plugin never blocks the interface: it runs where the app's own effects run (audio thread,
  compositor, filter worker), and Rust panics caught by the guarded callbacks disable the plugin.
