//! The `apps.*` commands: each app's status (installed, running, latest version), release checks
//! and the install, update and removal tasks.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::catalog::{self, APPS, App, AppCard};
use crate::install::{self, Found};
use crate::release::{self, is_newer};
use crate::{CmdResult, Launcher, paths, util};

/// Checks are reused for this long before `apps.list` looks again.
pub const RECHECK_SECS: i64 = 6 * 3600;

/// What the last release check found for one app.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Latest {
    pub version: Option<String>,
    pub error: Option<String>,
    pub notes: Option<String>,
    pub checked_at: chrono::DateTime<chrono::Utc>,
}

fn latest_file() -> std::path::PathBuf {
    paths::launcher_dir().join("latest.json")
}

pub(crate) fn load_latest() -> BTreeMap<String, Latest> {
    std::fs::read(latest_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_latest(l: &BTreeMap<String, Latest>) {
    if let Ok(b) = serde_json::to_vec_pretty(l) {
        let _ = util::write_private(&latest_file(), &b);
    }
}

/// One app as `apps.list` shows it.
pub fn status(l: &Launcher, app: &App) -> Value {
    let found: Option<Found> = install::find(app.id, l.platform.os);
    let latest = l.latest.lock().get(app.id).cloned();
    let pid = found.as_ref().and_then(|_| install::running_pid(app.id));
    let installed_version = found.as_ref().and_then(|f| f.version.clone());
    let latest_version = latest.as_ref().and_then(|x| x.version.clone());
    let update = matches!((&installed_version, &latest_version), (Some(i), Some(n)) if is_newer(n, i));
    let disc = install::discovery(app.id);
    let mut v = serde_json::to_value(AppCard::from(app)).unwrap_or_default();
    let o = v.as_object_mut().expect("a card is an object");
    o.insert("available".into(), json!(app.supports(l.platform)));
    o.insert("installed".into(), json!(found.is_some()));
    o.insert("version".into(), json!(installed_version));
    o.insert("path".into(), json!(found.as_ref().map(|f| f.path.clone())));
    o.insert("executable".into(), json!(found.as_ref().map(|f| f.executable.clone())));
    o.insert("managed".into(), json!(found.as_ref().is_some_and(|f| f.managed)));
    // Whether the launcher may update and remove this copy.
    o.insert("canManage".into(), json!(found.as_ref().is_none_or(|f| f.managed || f.standard)));
    o.insert("running".into(), json!(pid.is_some()));
    o.insert("pid".into(), json!(pid));
    o.insert("latest".into(), json!(latest_version));
    o.insert("latestError".into(), json!(latest.as_ref().and_then(|x| x.error.clone())));
    o.insert("notes".into(), json!(latest.as_ref().and_then(|x| x.notes.clone())));
    o.insert("checkedAt".into(), json!(latest.as_ref().map(|x| x.checked_at)));
    o.insert("updateAvailable".into(), json!(update));
    o.insert("busy".into(), json!(l.busy.lock().contains(app.id)));
    o.insert("cli".into(), disc.as_ref().map(|d| d["cli"].clone()).unwrap_or(Value::Null));
    o.insert("mcp".into(), disc.as_ref().map(|d| d["mcp"].clone()).unwrap_or(Value::Null));
    v
}

pub fn list(l: &Launcher) -> Value {
    let apps: Vec<Value> = APPS.iter().map(|a| status(l, a)).collect();
    let installed = apps.iter().filter(|a| a["installed"] == true).count();
    let updates = apps.iter().filter(|a| a["updateAvailable"] == true).count();
    let others = l.others.lock().clone();
    json!({ "platform": l.platform.key(), "os": l.platform.os_name(), "appsDir": paths::apps_dir(), "installed": installed, "updates": updates, "apps": apps, "others": others })
}

/// Apps the site lists (`GET /api/apps`) that this build of the launcher doesn't know: shown
/// with their page only, never installed (their keys aren't built in).
async fn others() -> Vec<Value> {
    let url = format!("{}/api/apps", util::server());
    let Ok(r) = util::client().get(url).send().await else { return vec![] };
    let Ok(v) = r.json::<Value>().await else { return vec![] };
    v["apps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["id"].as_str().is_some_and(|id| catalog::get(id).is_none()))
        .map(|a| json!({ "id": a["id"], "name": a["name"], "summary": a["summary"], "page": a["page"], "kind": a["kind"] }))
        .collect()
}

/// Whether the last check is old enough to look again.
pub fn stale(l: &Launcher) -> bool {
    let latest = l.latest.lock();
    APPS.iter().any(|a| latest.get(a.id).is_none_or(|x| (chrono::Utc::now() - x.checked_at).num_seconds() > RECHECK_SECS))
}

/// Asks lsuite.xyz for every app's latest release (all at once).
pub async fn check(l: &Arc<Launcher>) -> Value {
    let checks = APPS.iter().map(|a| async move { (a.id, release::latest(a, l.platform).await) });
    let (results, others) = futures::future::join(futures::future::join_all(checks), others()).await;
    *l.others.lock() = others;
    {
        let mut latest = l.latest.lock();
        for (id, r) in results {
            let prev = latest.get(id).cloned();
            let entry = match r {
                Ok(rel) => Latest { version: Some(rel.version), error: None, notes: rel.notes, checked_at: chrono::Utc::now() },
                // Offline: keep the version found last time, and say why it is old.
                Err(e) => Latest { version: prev.as_ref().and_then(|p| p.version.clone()), error: Some(e), notes: prev.and_then(|p| p.notes), checked_at: chrono::Utc::now() },
            };
            latest.insert(id.to_string(), entry);
        }
        save_latest(&latest);
    }
    l.hub.changed("apps");
    list(l)
}

/// Marks an app busy for the life of the guard.
struct Busy<'a>(&'a Launcher, &'static str);

impl<'a> Busy<'a> {
    fn take(l: &'a Launcher, app: &App) -> CmdResult<Self> {
        if !l.busy.lock().insert(app.id.to_string()) {
            return Err(format!("{} is already being installed or removed.", app.name));
        }
        l.hub.changed("apps");
        Ok(Busy(l, app.id))
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.busy.lock().remove(self.1);
        self.0.hub.changed("apps");
    }
}

/// Installs the latest release, or updates to it (`update_only`: only if installed and older).
pub async fn install(l: &Arc<Launcher>, id: &str, update_only: bool) -> CmdResult<Value> {
    let app = catalog::find(id)?;
    if !app.supports(l.platform) {
        return Err(format!("{} has no {} build yet. Its page: {}", app.name, l.platform.os_name(), app.page()));
    }
    let found = install::find(app.id, l.platform.os);
    if let Some(f) = &found
        && !f.managed
        && !f.standard
    {
        return Err(format!("{} is installed at {}, which the launcher doesn't manage (a build from source?). Update it there.", app.name, f.path.display()));
    }
    if update_only && found.is_none() {
        return Err(format!("{} isn't installed. Install it first.", app.name));
    }
    let _busy = Busy::take(l, app)?;
    let verb = if found.is_some() { "Updating" } else { "Installing" };
    let mut progress = l.hub.progress(format!("install:{}", app.id), format!("{verb} {}", app.name));
    let result = async {
        progress.stage(format!("Looking for {}'s latest release", app.name), 0, None);
        let rel = release::latest(app, l.platform).await?;
        l.latest.lock().insert(app.id.into(), Latest { version: Some(rel.version.clone()), error: None, notes: rel.notes.clone(), checked_at: chrono::Utc::now() });
        save_latest(&l.latest.lock());
        if let Some(v) = found.as_ref().and_then(|f| f.version.clone())
            && !is_newer(&rel.version, &v)
        {
            return Ok(json!({ "app": app.id, "version": v, "upToDate": true, "message": format!("{} {v} is the latest version.", app.name) }));
        }
        let record = install::install(app, &rel, l.platform, &mut progress).await?;
        Ok(json!({ "app": app.id, "version": record.version, "path": record.path, "executable": record.executable, "updated": found.is_some(), "message": format!("{} {} is installed.", app.name, record.version) }))
    }
    .await;
    let done = result.as_ref().ok().and_then(|v| v["message"].as_str().map(str::to_string)).unwrap_or_default();
    progress.finish(&result, done);
    result
}

/// Updates every installed app that has a newer version and isn't open.
pub async fn update_all(l: &Arc<Launcher>) -> CmdResult<Value> {
    check(l).await;
    let todo: Vec<&App> = APPS.iter().filter(|a| status(l, a)["updateAvailable"] == true).collect();
    let mut updated = vec![];
    let mut skipped = vec![];
    for a in todo {
        match install(l, a.id, true).await {
            Ok(v) => updated.push(v),
            Err(e) => skipped.push(json!({ "app": a.id, "error": e })),
        }
    }
    Ok(json!({ "updated": updated, "skipped": skipped }))
}

pub async fn uninstall(l: &Arc<Launcher>, id: &str) -> CmdResult<Value> {
    let app = catalog::find(id)?;
    let _busy = Busy::take(l, app)?;
    let os = l.platform.os;
    let a = *app;
    let r = tokio::task::spawn_blocking(move || install::uninstall(&a, os)).await.map_err(|e| e.to_string())?;
    l.hub.changed("apps");
    r
}
