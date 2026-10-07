//! The command registry: every action of the launcher, for every client.
//!
//! The window, `lsuite-cli` and `lsuite-mcp` all go through [`call`]; parameters are checked
//! here against each command's [`Spec`], and agents (MCP) are held to
//! `settings.agent` ([`Perm`]). `docs/COMMANDS.md` and the MCP tool list are generated from
//! [`COMMANDS`], so they can't drift from what is accepted.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::{CmdResult, Launcher, account, apps, catalog, cloud, settings, util};

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
    CloudWrite,
    CloudDelete,
    Account,
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

const APP: Param = req("app", Kind::Str, "The app: ryolune, kimchi, zenith, nori or folio.");

pub const COMMANDS: &[Spec] = &[
    Spec { name: "apps.list", summary: "Every lsuite app: installed or not, its version, the latest one, whether it is open. `refresh` asks GitHub again (otherwise the last check is reused for 6 hours).", params: &[opt("refresh", Kind::Bool, "Look for new versions now.")], perm: Perm::Read },
    Spec { name: "apps.check", summary: "Looks for the latest release of every app now.", params: &[], perm: Perm::Read },
    Spec { name: "apps.install", summary: "Downloads the app's latest release, checks its signature and installs it (or updates it if an older version is installed).", params: &[APP], perm: Perm::Install },
    Spec { name: "apps.update", summary: "Updates an installed app to its latest release (it must be closed).", params: &[APP], perm: Perm::Install },
    Spec { name: "apps.updateAll", summary: "Updates every installed app that has a newer release and isn't open.", params: &[], perm: Perm::Install },
    Spec { name: "apps.uninstall", summary: "Removes the app (it must be closed). Its documents, settings and data folders stay.", params: &[APP], perm: Perm::Remove },
    Spec { name: "apps.open", summary: "Opens the app, with files if given.", params: &[APP, opt("files", Kind::List, "Files to open in it.")], perm: Perm::Read },
    Spec { name: "apps.reveal", summary: "Shows the installed app in the file manager.", params: &[APP], perm: Perm::Read },
    Spec { name: "apps.page", summary: "Opens the app's page on lsuite.xyz in the browser.", params: &[APP], perm: Perm::Read },
    Spec { name: "account.status", summary: "The lsuite AI account on this computer: plan, allowance used, cloud storage.", params: &[], perm: Perm::Read },
    Spec { name: "account.signIn", summary: "Signs in to lsuite (every app on this computer with it): opens the browser, or takes a key (lsk_…) made on the account page.", params: &[opt("key", Kind::Str, "A key from lsuite.xyz/account (for terminals).")], perm: Perm::Account },
    Spec { name: "account.cancelSignIn", summary: "Stops waiting for a browser sign-in.", params: &[], perm: Perm::Read },
    Spec { name: "account.signOut", summary: "Signs out here and on the server (every app on this computer with it).", params: &[], perm: Perm::Account },
    Spec { name: "account.plans", summary: "lsuite AI's plans: prices, models, monthly allowance and cloud storage.", params: &[], perm: Perm::Read },
    Spec { name: "account.manage", summary: "Opens the account page in the browser (plan, keys, connected apps).", params: &[], perm: Perm::Read },
    Spec { name: "cloud.status", summary: "lsuite Cloud: the plan's storage and how much is used.", params: &[], perm: Perm::Read },
    Spec { name: "cloud.list", summary: "A folder of lsuite Cloud: its folders and files. `all` lists every file instead.", params: &[opt("path", Kind::Str, "The folder (the root when left out)."), opt("all", Kind::Bool, "Every file and folder, flat.")], perm: Perm::Read },
    Spec { name: "cloud.upload", summary: "Uploads a file or a whole folder from this computer into a cloud folder, under its own name.", params: &[req("source", Kind::Str, "The file or folder on this computer."), opt("into", Kind::Str, "The cloud folder (the root when left out)."), opt("overwrite", Kind::Bool, "Replace files already there (default false).")], perm: Perm::CloudWrite },
    Spec { name: "cloud.download", summary: "Downloads a cloud file or folder into a folder on this computer.", params: &[req("path", Kind::Str, "The file or folder in the cloud."), req("into", Kind::Str, "The folder on this computer."), opt("overwrite", Kind::Bool, "Replace files already there (default false).")], perm: Perm::Read },
    Spec { name: "cloud.mkdir", summary: "Creates a folder in lsuite Cloud.", params: &[req("path", Kind::Str, "The new folder's path.")], perm: Perm::CloudWrite },
    Spec { name: "cloud.move", summary: "Moves a cloud file or folder (with its content) to another path.", params: &[req("from", Kind::Str, "What to move."), req("to", Kind::Str, "Its new path."), opt("overwrite", Kind::Bool, "Replace what is there (default false).")], perm: Perm::CloudWrite },
    Spec { name: "cloud.rename", summary: "Renames a cloud file or folder in place.", params: &[req("path", Kind::Str, "What to rename."), req("name", Kind::Str, "The new name.")], perm: Perm::CloudWrite },
    Spec { name: "cloud.delete", summary: "Deletes a cloud file, or a folder and everything in it.", params: &[req("path", Kind::Str, "The file or folder.")], perm: Perm::CloudDelete },
    Spec { name: "cloud.syncList", summary: "The folders kept in step with the cloud, and how their last sync went.", params: &[], perm: Perm::Read },
    Spec { name: "cloud.syncAdd", summary: "Keeps a folder on this computer in step with a cloud folder, both ways (changes go either way; a file changed on both sides is kept twice). Syncs it once at once.", params: &[req("local", Kind::Str, "The folder on this computer."), opt("remote", Kind::Str, "The cloud folder (default: the local folder's name at the top of the cloud).")], perm: Perm::CloudWrite },
    Spec { name: "cloud.syncNow", summary: "Syncs one synced folder now, or all of them.", params: &[opt("id", Kind::Str, "The synced folder (all when left out)."), opt("force", Kind::Bool, "Go on even if more than half of a folder would be deleted.")], perm: Perm::CloudWrite },
    Spec { name: "cloud.syncRemove", summary: "Stops syncing a folder. Its files stay on this computer and in the cloud.", params: &[req("id", Kind::Str, "The synced folder.")], perm: Perm::CloudWrite },
    Spec { name: "settings.get", summary: "The launcher's settings.", params: &[], perm: Perm::Read },
    Spec { name: "settings.set", summary: "Changes one setting (`theme`, `checkOnStart`, `autoUpdate`, `reduceTransparency`, `syncEveryMinutes`, `agent.*`). Agents can't change `agent.*`.", params: &[req("key", Kind::Str, "The setting's dotted name."), req("value", Kind::Any, "Its new value.")], perm: Perm::Read },
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
        Perm::CloudWrite => a.cloud_write,
        Perm::CloudDelete => a.cloud_delete,
        Perm::Account => a.account,
    }
}

/// Runs one command.
pub async fn call(l: &Arc<Launcher>, source: Source, name: &str, params: Value) -> CmdResult<Value> {
    let s = spec(name).ok_or_else(|| format!("There's no command {name:?}. `app.commands` lists them."))?;
    let p = check_params(s, &params)?;
    if source == Source::Mcp && !allowed(s.perm, &settings::load()) {
        return Err(format!("{name} is turned off for agents. The person can allow it in the lsuite launcher (Settings › Agents) or with `lsuite-cli settings.set key=agent.… value=true`."));
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
        "account.status" => account::status().await,
        "account.signIn" => {
            let key = str_("key");
            let r = if !key.is_empty() {
                account::sign_in_key(&key).await
            } else {
                let started = account::sign_in_browser(true).await?;
                *l.sign_in.lock() = Some(started.done.abort_handle());
                l.hub.emit(crate::Event::Progress { task: "signIn".into(), label: format!("Waiting for the browser: {}", started.url), done: 0, total: None });
                let r = match started.done.await {
                    Ok(r) => r,
                    Err(e) if e.is_cancelled() => Err("Sign-in cancelled.".to_string()),
                    Err(e) => Err(e.to_string()),
                };
                l.sign_in.lock().take();
                l.hub.emit(crate::Event::TaskDone { task: "signIn".into(), ok: r.is_ok(), message: r.as_ref().err().cloned().unwrap_or_else(|| "Signed in.".into()) });
                r
            };
            l.hub.changed("account");
            l.hub.changed("cloud");
            r
        }
        "account.cancelSignIn" => {
            let had = l.sign_in.lock().take().map(|h| h.abort()).is_some();
            Ok(json!({ "cancelled": had }))
        }
        "account.signOut" => {
            let r = account::sign_out().await;
            l.hub.changed("account");
            l.hub.changed("cloud");
            r
        }
        "account.plans" => account::plans().await,
        "account.manage" => {
            let url = account::manage_url();
            util::open_external(&url).map_err(|e| format!("Couldn't open the browser ({e}). The page: {url}"))?;
            Ok(json!({ "url": url }))
        }
        "cloud.status" => cloud::status().await,
        "cloud.list" => {
            let all = cloud::list_all().await?;
            if flag("all") {
                return Ok(all);
            }
            Ok(cloud::folder_view(&all, &cloud::clean(&str_("path"), true)?))
        }
        "cloud.upload" => {
            let source = PathBuf::from(str_("source"));
            cloud::upload(&source, &str_("into"), flag("overwrite"), &l.hub).await
        }
        "cloud.download" => {
            let into = PathBuf::from(str_("into"));
            cloud::download(&str_("path"), &into, flag("overwrite"), &l.hub).await
        }
        "cloud.mkdir" => changed(l, cloud::mkdir(&str_("path")).await),
        "cloud.move" => changed(l, cloud::move_to(&str_("from"), &str_("to"), flag("overwrite")).await),
        "cloud.rename" => {
            let path = cloud::clean(&str_("path"), false)?;
            let name = str_("name");
            if name.contains('/') {
                return Err("A name can't contain /. Use cloud.move to put it in another folder.".into());
            }
            let to = cloud::clean(&cloud::join(cloud::parent_of(&path), name.trim()), false)?;
            changed(l, cloud::move_to(&path, &to, false).await)
        }
        "cloud.delete" => changed(l, cloud::delete(&str_("path")).await),
        "cloud.syncList" => Ok(crate::sync::list(l)),
        "cloud.syncAdd" => {
            let local = PathBuf::from(str_("local"));
            let remote = match str_("remote") {
                r if r.trim().is_empty() => local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                r => r,
            };
            let pair = crate::sync::add(&local, &remote)?;
            l.hub.changed("sync");
            let first = crate::sync::sync_pair(l, &pair.id, false).await;
            Ok(json!({ "pair": pair, "firstSync": match first { Ok(v) => v, Err(e) => json!({ "error": e }) } }))
        }
        "cloud.syncNow" => {
            let id = str_("id");
            crate::sync::sync_all(l, (!id.is_empty()).then_some(id.as_str()), flag("force")).await
        }
        "cloud.syncRemove" => {
            let r = crate::sync::remove(&str_("id"));
            l.hub.changed("sync");
            r
        }
        "app.checkUpdates" => crate::selfupdate::check(l).await.map(|s| crate::selfupdate::to_value(&s)),
        "app.installUpdate" => crate::selfupdate::install(l).await.map(|s| crate::selfupdate::to_value(&s)),
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
        "app.version" => Ok(json!({ "app": "lsuite", "version": crate::VERSION, "platform": l.platform.key(), "appsDir": crate::paths::apps_dir(), "lsuiteHome": crate::paths::lsuite_home(), "server": account::server() })),
        "app.commands" => Ok(json!({ "commands": COMMANDS.iter().map(describe).collect::<Vec<_>>() })),
        _ => Err(format!("{name} isn't handled (a bug).")),
    }
}

fn changed(l: &Launcher, r: CmdResult<Value>) -> CmdResult<Value> {
    l.hub.changed("cloud");
    r
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
                Perm::CloudWrite => "agent.cloudWrite",
                Perm::CloudDelete => "agent.cloudDelete",
                Perm::Account => "agent.account",
                Perm::Read => "",
            };
            out.push_str(&format!("\nAgents: needs `{setting}`.\n"));
        }
    }
    out
}
