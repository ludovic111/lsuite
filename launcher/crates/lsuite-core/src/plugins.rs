//! The `plugins.*` commands: the lsuite plugins installed for each app (lsuite's PLUGINS.md).
//!
//! An installed plugin is a bundle folder, `~/.lsuite/plugins/<app>/<id>/` (`LSUITE_HOME`
//! replaces `~/.lsuite`), holding `plugin.toml` and the library. The apps make them (their
//! `plugin.*` commands, usually driven by an agent) and load them; the launcher lists them and
//! removes them, then asks a running app to `plugin.rescan` through its CLI.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::catalog::{self, APPS};
use crate::{CmdResult, Launcher, install, paths};

/// `~/.lsuite/plugins/<app>/`.
pub fn plugins_dir(app: &str) -> PathBuf {
    paths::lsuite_home().join("plugins").join(app)
}

/// The fields of `plugin.toml` the launcher shows.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub description: String,
}

/// A bundle's `plugin.toml`, if it has a readable one.
pub fn read_manifest(bundle: &Path) -> Option<Manifest> {
    toml::from_str(&std::fs::read_to_string(bundle.join("plugin.toml")).ok()?).ok()
}

/// The plugins in one app's folder: `(folder, manifest)`, by name. Folders starting with `.`
/// (an app's staging) and folders without a `plugin.toml` aren't plugins.
fn bundles_in(dir: &Path) -> Vec<(PathBuf, Manifest)> {
    let mut out: Vec<(PathBuf, Manifest)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .filter_map(|p| {
            let mut m = read_manifest(&p)?;
            let folder = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if m.id.is_empty() {
                m.id = folder.clone();
            }
            if m.name.is_empty() {
                m.name = folder;
            }
            Some((p, m))
        })
        .collect();
    out.sort_by_key(|(_, m)| m.name.to_lowercase());
    out
}

fn describe(path: &Path, m: &Manifest) -> Value {
    json!({ "id": m.id, "name": m.name, "version": m.version, "kind": m.kind, "description": m.description, "path": path })
}

/// The installed plugins of every app (or one), each app with whether it is installed.
pub fn list(l: &Launcher, app: Option<&str>) -> CmdResult<Value> {
    let only = match app.filter(|a| !a.trim().is_empty()) {
        Some(a) => Some(catalog::find(a)?.id),
        None => None,
    };
    let apps: Vec<Value> = APPS
        .iter()
        .filter(|a| only.is_none_or(|o| o == a.id))
        .map(|a| {
            let dir = plugins_dir(a.id);
            let plugins: Vec<Value> = bundles_in(&dir).iter().map(|(p, m)| describe(p, m)).collect();
            json!({ "app": a.id, "name": a.name, "installed": install::find(a.id, l.platform.os).is_some(), "dir": dir, "plugins": plugins })
        })
        .collect();
    let count: usize = apps.iter().map(|a| a["plugins"].as_array().map_or(0, Vec::len)).sum();
    Ok(json!({ "apps": apps, "count": count }))
}

/// The folder of `id` in `dir`: the folder named so, else the one whose `plugin.toml` says so.
fn find_bundle(dir: &Path, id: &str) -> Option<(PathBuf, Manifest)> {
    bundles_in(dir).into_iter().find(|(p, m)| p.file_name().is_some_and(|n| n == id) || m.id == id)
}

/// Removes an installed plugin's folder.
pub async fn remove(l: &Launcher, app: &str, id: &str) -> CmdResult<Value> {
    let app = catalog::find(app)?;
    let dir = plugins_dir(app.id);
    let (path, m) = find_bundle(&dir, id.trim()).ok_or_else(|| format!("{} has no plugin {id:?} installed (plugins.list shows them).", app.name))?;
    std::fs::remove_dir_all(&path).map_err(|e| format!("Couldn't remove {} ({e}).", path.display()))?;
    let rescanned = rescan(app.id).await;
    l.hub.changed("plugins");
    let message = if rescanned { format!("{} is removed and unloaded from {}.", m.name, app.name) } else { format!("{} is removed. {} lets go of it when it next starts.", m.name, app.name) };
    Ok(json!({ "removed": m.id, "app": app.id, "path": path, "rescanned": rescanned, "message": message }))
}

/// Asks the app, if it is running, to load its plugins again. True when it answered.
async fn rescan(app: &str) -> bool {
    let Some(d) = install::discovery(app) else { return false };
    if install::running_pid(app).is_none() {
        return false;
    }
    let Some(cli) = d["cli"].as_str().map(PathBuf::from).filter(|p| p.is_file()) else { return false };
    let run = tokio::process::Command::new(cli).arg("plugin.rescan").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    matches!(tokio::time::timeout(Duration::from_secs(20), run).await, Ok(Ok(s)) if s.success())
}

/// What the "Build a plugin" action asks the lsuite agent.
pub fn build_request(app: &str, description: &str) -> String {
    format!("Build a {app} plugin: {}", description.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_are_read_from_their_manifests() {
        let d = tempfile::tempdir().unwrap();
        let warm = d.path().join("com.example.warm");
        std::fs::create_dir_all(&warm).unwrap();
        std::fs::write(warm.join("plugin.toml"), "id = \"com.example.warm\"\nname = \"Warm\"\nversion = \"1.0.0\"\napp = \"ryolune\"\nkind = \"effect\"\nabi = 1\ndescription = \"Tape.\"\n\n[library]\nlinux = \"libwarm.so\"\n").unwrap();
        let bare = d.path().join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("plugin.toml"), "version = \"0.1.0\"\n").unwrap();
        // Not plugins: staging, a folder without a manifest, a file.
        std::fs::create_dir_all(d.path().join(".lsuite-plugin-x")).unwrap();
        std::fs::write(d.path().join(".lsuite-plugin-x/plugin.toml"), "id = \"x\"\n").unwrap();
        std::fs::create_dir_all(d.path().join("empty")).unwrap();
        std::fs::write(d.path().join("notes.txt"), "x").unwrap();
        let found = bundles_in(d.path());
        let names: Vec<&str> = found.iter().map(|(_, m)| m.name.as_str()).collect();
        assert_eq!(names, ["bare", "Warm"]);
        assert_eq!(found[1].1, Manifest { id: "com.example.warm".into(), name: "Warm".into(), version: "1.0.0".into(), kind: "effect".into(), description: "Tape.".into() });
        assert_eq!(found[0].1.id, "bare");
        assert_eq!(find_bundle(d.path(), "com.example.warm").unwrap().0, warm);
        assert!(find_bundle(d.path(), "../etc").is_none());
        assert!(bundles_in(&d.path().join("missing")).is_empty());
        assert_eq!(build_request("nori", " a halftone filter "), "Build a nori plugin: a halftone filter");
    }
}
