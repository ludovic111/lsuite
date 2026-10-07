//! The launcher's settings, `~/.lsuite/launcher/settings.json`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CmdResult, paths, util};

/// What agents (lsuite-mcp) may do. The window and lsuite-cli act for the person and may do
/// everything; off means the command is refused for agents.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentPermissions {
    /// Install and update apps.
    pub install: bool,
    /// Remove apps.
    pub remove: bool,
    /// Upload, create folders, move and rename in lsuite Cloud.
    pub cloud_write: bool,
    /// Delete in lsuite Cloud.
    pub cloud_delete: bool,
    /// Sign in and out.
    pub account: bool,
    /// Publish plugins on the lsuite Marketplace.
    pub publish: bool,
}

impl Default for AgentPermissions {
    fn default() -> Self {
        Self { install: true, remove: false, cloud_write: true, cloud_delete: false, account: false, publish: false }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `system`, `dark` or `light`.
    pub theme: String,
    /// Look for new versions of the apps when the launcher starts (`LSUITE_NO_UPDATE=1` too).
    pub check_on_start: bool,
    /// Update installed apps by themselves when a new version is found (closed apps only).
    pub auto_update: bool,
    /// Ask the system to show fewer translucent surfaces.
    pub reduce_transparency: bool,
    /// How often the window syncs the synced folders (0: only when asked).
    pub sync_every_minutes: u64,
    pub agent: AgentPermissions,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: "system".into(), check_on_start: true, auto_update: false, reduce_transparency: false, sync_every_minutes: 5, agent: AgentPermissions::default() }
    }
}

pub fn load() -> Settings {
    std::fs::read(paths::settings_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(s: &Settings) -> CmdResult<()> {
    let bytes = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    util::write_private(&paths::settings_file(), &bytes).map_err(|e| format!("Couldn't save the settings ({e})."))
}

/// Sets one setting by its dotted name (`theme`, `agent.remove`), checking the type.
pub fn set(key: &str, value: Value) -> CmdResult<Settings> {
    let mut whole = serde_json::to_value(load()).map_err(|e| e.to_string())?;
    let pointer = format!("/{}", key.replace('.', "/"));
    let Some(slot) = whole.pointer_mut(&pointer).filter(|v| !v.is_object()) else {
        return Err(format!("There's no setting called {key}. Settings: {}.", names().join(", ")));
    };
    if std::mem::discriminant(slot) != std::mem::discriminant(&value) {
        return Err(format!("{key} takes {}.", kind(slot)));
    }
    *slot = value;
    let s: Settings = serde_json::from_value(whole).map_err(|e| e.to_string())?;
    if !matches!(s.theme.as_str(), "system" | "dark" | "light") {
        return Err("theme is system, dark or light.".into());
    }
    save(&s)?;
    Ok(s)
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "true or false",
        Value::String(_) => "text",
        Value::Number(_) => "a number",
        _ => "a value",
    }
}

/// Every setting's dotted name.
pub fn names() -> Vec<String> {
    fn walk(prefix: &str, v: &Value, out: &mut Vec<String>) {
        if let Value::Object(m) = v {
            for (k, v) in m {
                let name = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                if v.is_object() { walk(&name, v, out) } else { out.push(name) }
            }
        }
    }
    let mut out = vec![];
    walk("", &serde_json::to_value(Settings::default()).unwrap_or_default(), &mut out);
    out
}
