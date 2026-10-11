//! The command registry: every action of the launcher, for every client.
//!
//! The window, `lsuite-cli` and `lsuite-mcp` all go through [`call`]; parameters are checked
//! here against each command's [`Spec`], and agents (MCP) are held to
//! `settings.agent` ([`Perm`]). `docs/COMMANDS.md` and the MCP tool list are generated from
//! [`COMMANDS`], so they can't drift from what is accepted.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::{CmdResult, Launcher, apps, catalog, settings, util};

/// Who runs a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The window (the person).
    Window,
    /// `lsuite-cli` (the person, in a terminal).
    Cli,
    /// `lsuite-mcp` (an agent): held to `settings.agent`.
    Mcp,
}

/// What a command may change, for agents' permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Perm {
    Read,
    Install,
    Remove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Str,
    Bool,
    Any,
    List,
}

#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub doc: &'static str,
}

const fn req(name: &'static str, kind: Kind, doc: &'static str) -> Param {
    Param { name, kind, required: true, doc }
}

const fn opt(name: &'static str, kind: Kind, doc: &'static str) -> Param {
    Param { name, kind, required: false, doc }
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub name: &'static str,
    pub summary: &'static str,
    pub params: &'static [Param],
    pub perm: Perm,
}

const APP: Param = req("app", Kind::Str, "The app: ryolune, kimchi, nori or folio.");

pub const COMMANDS: &[Spec] = &[
    Spec { name: "apps.list", summary: "Every lsuite app: installed or not, its version, the latest one, whether it is open. `refresh` asks lsuite.xyz again (otherwise the last check is reused for 6 hours). No account is needed.", params: &[opt("refresh", Kind::Bool, "Look for new versions now.")], perm: Perm::Read },
    Spec { name: "apps.check", summary: "Looks for the latest release of every app now.", params: &[], perm: Perm::Read },
    Spec { name: "apps.install", summary: "Downloads the app's latest release, checks its signature and installs it (or updates it if an older version is installed).", params: &[APP], perm: Perm::Install },
    Spec { name: "apps.update", summary: "Updates an installed app to its latest release (it must be closed).", params: &[APP], perm: Perm::Install },
    Spec { name: "apps.updateAll", summary: "Updates every installed app that has a newer release and isn't open.", params: &[], perm: Perm::Install },
    Spec { name: "apps.uninstall", summary: "Removes the app (it must be closed). Its documents, settings and data folders stay.", params: &[APP], perm: Perm::Remove },
    Spec { name: "apps.open", summary: "Opens the app, with files if given.", params: &[APP, opt("files", Kind::List, "Files to open in it.")], perm: Perm::Read },
    Spec { name: "apps.reveal", summary: "Shows the installed app in the file manager.", params: &[APP], perm: Perm::Read },
    Spec { name: "apps.page", summary: "Opens the app's page on lsuite.xyz in the browser.", params: &[APP], perm: Perm::Read },
    Spec { name: "plugins.list", summary: "The lsuite plugins installed for each app (`~/.lsuite/plugins/<app>/<id>/`): id, name, version, kind, description, from each bundle's plugin.toml. To make one, ask the lsuite agent (agent.run) to build it with the app's plugin.* commands.", params: &[opt("app", Kind::Str, "Only this app's plugins.")], perm: Perm::Read },
    Spec { name: "plugins.remove", summary: "Removes an installed plugin (deletes its folder) and asks the app, if it is open, to let go of it.", params: &[APP, req("id", Kind::Str, "The plugin's id (from plugins.list).")], perm: Perm::Remove },
    Spec { name: "agent.run", summary: "Asks the lsuite agent to do a job, across the apps if it needs to (it reads each app's brief and skills, does the work, looks at it, reports). Waits until it's done.", params: &[req("prompt", Kind::Str, "The job, in your words."), opt("provider", Kind::Str, "claude-code or anthropic (the first ready one when left out).")], perm: Perm::Read },
    Spec { name: "agent.stop", summary: "Stops the lsuite agent.", params: &[], perm: Perm::Read },
    Spec { name: "agent.log", summary: "The lsuite agent's conversation: what was asked, the tools it used, what it answered.", params: &[], perm: Perm::Read },
    Spec { name: "agent.clear", summary: "Starts a new conversation with the lsuite agent.", params: &[], perm: Perm::Read },
    Spec { name: "agent.providers", summary: "The ways the lsuite agent can run on this computer (Claude Code, Anthropic API).", params: &[], perm: Perm::Read },
    Spec { name: "settings.get", summary: "The launcher's settings.", params: &[], perm: Perm::Read },
    Spec { name: "settings.set", summary: "Changes one setting (`theme`, `checkOnStart`, `autoUpdate`, `reduceTransparency`, `agent.*`). Agents can't change `agent.*`.", params: &[req("key", Kind::Str, "The setting's dotted name."), req("value", Kind::Any, "Its new value.")], perm: Perm::Read },
    Spec { name: "app.version", summary: "The launcher's version, the computer's platform and where apps go.", params: &[], perm: Perm::Read },
    Spec { name: "app.checkUpdates", summary: "Looks for a newer version of the launcher itself.", params: &[], perm: Perm::Read },
    Spec { name: "app.installUpdate", summary: "Downloads the newer launcher, checks its signature and installs it; restart the launcher to use it.", params: &[], perm: Perm::Install },
    Spec { name: "app.commands", summary: "Every command, with its parameters.", params: &[], perm: Perm::Read },
];

pub fn spec(name: &str) -> Option<&'static Spec> {
    COMMANDS.iter().find(|s| s.name == name)
}

fn check_params(s: &Spec, params: &Value) -> CmdResult<Map<String, Value>> {
    let map = match params {
        Value::Null => Map::new(),
        Value::Object(m) => m.clone(),
        _ => return Err(format!("{}: parameters are a JSON object.", s.name)),
    };
    for k in map.keys() {
        if !s.params.iter().any(|p| p.name == k) {
            let known: Vec<&str> = s.params.iter().map(|p| p.name).collect();
            return Err(format!("{}: there's no parameter {k:?}{}.", s.name, if known.is_empty() { String::new() } else { format!(" (it takes {})", known.join(", ")) }));
        }
    }
    for p in s.params {
        match map.get(p.name) {
            None | Some(Value::Null) if p.required => return Err(format!("{}: {} is required ({})", s.name, p.name, p.doc)),
            Some(v) if !v.is_null() => {
                let ok = match p.kind {
                    Kind::Str => v.is_string(),
                    Kind::Bool => v.is_boolean(),
                    Kind::List => v.is_array() || v.is_string(),
                    Kind::Any => true,
                };
                if !ok {
                    return Err(format!("{}: {} takes {}.", s.name, p.name, match p.kind {
                        Kind::Str => "text",
                        Kind::Bool => "true or false",
                        Kind::List => "a list",
                        Kind::Any => "a value",
                    }));
                }
            }
            _ => {}
        }
    }
    Ok(map)
}

fn allowed(perm: Perm, s: &settings::Settings) -> bool {
    let a = &s.agent;
    match perm {
        Perm::Read => true,
        Perm::Install => a.install,
        Perm::Remove => a.remove,
    }
}

/// [`call`] behind a boxed future: what the lsuite agent uses, so the agent's future doesn't
/// contain itself (it can call commands, and `agent.run` is one).
pub fn call_boxed(l: Arc<Launcher>, source: Source, name: String, params: Value) -> futures::future::BoxFuture<'static, CmdResult<Value>> {
    Box::pin(async move { call(&l, source, &name, params).await })
}

/// Runs one command.
pub async fn call(l: &Arc<Launcher>, source: Source, name: &str, params: Value) -> CmdResult<Value> {
    let s = spec(name).ok_or_else(|| format!("There's no command {name:?}. `app.commands` lists them."))?;
    let p = check_params(s, &params)?;
    if source == Source::Mcp && !allowed(s.perm, &settings::load()) {
        return Err(format!("{name} is turned off for agents. The person can allow it in the lsuite launcher (Settings › Agents) or with `lsuite-cli settings.set key=agent.… value=true`."));
    }
    if source == Source::Mcp && name.starts_with("agent.") {
        return Err("The lsuite agent is driven from the window or lsuite-cli, not by another agent.".into());
    }
    let str_ = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string).unwrap_or_default();
    let flag = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    match name {
        "apps.list" => {
            if flag("refresh") {
                return Ok(apps::check(l).await);
            }
            Ok(apps::list(l))
        }
        "apps.check" => Ok(apps::check(l).await),
        "apps.install" => apps::install(l, &str_("app"), false).await,
        "apps.update" => apps::install(l, &str_("app"), true).await,
        "apps.updateAll" => apps::update_all(l).await,
        "apps.uninstall" => apps::uninstall(l, &str_("app")).await,
        "apps.open" => {
            let app = catalog::find(&str_("app"))?;
            let files: Vec<PathBuf> = match p.get("files") {
                Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(PathBuf::from).collect(),
                Some(Value::String(s)) => vec![PathBuf::from(s)],
                _ => vec![],
            };
            let r = crate::install::open(app, l.platform.os, &files);
            l.hub.changed("apps");
            r
        }
        "apps.reveal" => crate::install::reveal(catalog::find(&str_("app"))?, l.platform.os),
        "apps.page" => {
            let app = catalog::find(&str_("app"))?;
            let url = app.page();
            util::open_external(&url).map_err(|e| format!("Couldn't open the browser ({e}). The page: {url}"))?;
            Ok(json!({ "url": url }))
        }
        "plugins.list" => crate::plugins::list(l, Some(str_("app").as_str())),
        "plugins.remove" => crate::plugins::remove(l, &str_("app"), &str_("id")).await,
        "app.checkUpdates" => crate::selfupdate::check(l).await.map(|s| crate::selfupdate::to_value(&s)),
        "app.installUpdate" => crate::selfupdate::install(l).await.map(|s| crate::selfupdate::to_value(&s)),
        "agent.run" => {
            let provider = str_("provider");
            crate::agent::run(l, &str_("prompt"), (!provider.is_empty()).then_some(provider.as_str())).await
        }
        "agent.stop" => Ok(json!({ "stopped": crate::agent::stop(l) })),
        "agent.log" => Ok(crate::agent::log(l)),
        "agent.clear" => {
            crate::agent::clear(l);
            Ok(json!({ "cleared": true }))
        }
        "agent.providers" => Ok(crate::agent::providers()),
        "settings.get" => Ok(serde_json::to_value(settings::load()).unwrap_or_default()),
        "settings.set" => {
            let key = str_("key");
            if source == Source::Mcp && key.starts_with("agent") {
                return Err("Agents can't change their own permissions.".into());
            }
            let s = settings::set(&key, p.get("value").cloned().unwrap_or(Value::Null))?;
            l.hub.changed("settings");
            Ok(serde_json::to_value(s).unwrap_or_default())
        }
        "app.version" => Ok(json!({ "app": "lsuite", "version": crate::VERSION, "platform": l.platform.key(), "appsDir": crate::paths::apps_dir(), "lsuiteHome": crate::paths::lsuite_home(), "server": util::server() })),
        "app.commands" => Ok(json!({ "commands": COMMANDS.iter().map(describe).collect::<Vec<_>>() })),
        _ => Err(format!("{name} isn't handled (a bug).")),
    }
}

fn kind_schema(k: Kind) -> Value {
    match k {
        Kind::Str => json!({ "type": "string" }),
        Kind::Bool => json!({ "type": "boolean" }),
        Kind::List => json!({ "type": "array", "items": { "type": "string" } }),
        Kind::Any => json!({}),
    }
}

/// A command as `app.commands` and MCP's `tools/list` show it.
pub fn describe(s: &Spec) -> Value {
    json!({ "name": s.name, "description": s.summary, "inputSchema": input_schema(s) })
}

pub fn input_schema(s: &Spec) -> Value {
    let mut props = Map::new();
    for p in s.params {
        let mut v = kind_schema(p.kind);
        v["description"] = json!(p.doc);
        props.insert(p.name.into(), v);
    }
    let required: Vec<&str> = s.params.iter().filter(|p| p.required).map(|p| p.name).collect();
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

/// `docs/COMMANDS.md`.
pub fn markdown() -> String {
    let mut out = String::from("# lsuite launcher commands\n\nGenerated from the registry (`cargo run -p lsuite-cli -- docs`); don't edit by hand.\nEvery command runs the same from the window, `lsuite-cli <command> key=value…` and `lsuite-mcp`\n(tools are named with `_` for `.`: `apps_install`). Agents are held to `settings.agent`.\n");
    let mut family = "";
    for s in COMMANDS {
        let f = s.name.split('.').next().unwrap_or("");
        if f != family {
            family = f;
            out.push_str(&format!("\n## {f}\n"));
        }
        out.push_str(&format!("\n### `{}`\n\n{}\n", s.name, s.summary));
        if !s.params.is_empty() {
            out.push('\n');
            for p in s.params {
                let k = match p.kind {
                    Kind::Str => "text",
                    Kind::Bool => "true/false",
                    Kind::List => "list",
                    Kind::Any => "any",
                };
                out.push_str(&format!("- `{}` ({k}{}): {}\n", p.name, if p.required { ", required" } else { "" }, p.doc));
            }
        }
        if s.perm != Perm::Read {
            let setting = match s.perm {
                Perm::Install => "agent.install",
                Perm::Remove => "agent.remove",
                Perm::Read => "",
            };
            out.push_str(&format!("\nAgents: needs `{setting}`.\n"));
        }
    }
    out
}
