//! Two-way sync between a folder on this computer and a folder in lsuite Cloud.
//!
//! Each synced pair keeps a snapshot of what both sides held after the last sync
//! (`~/.lsuite/launcher/sync/<id>.json`: path → SHA-256, plus the local size and time to skip
//! hashing unchanged files). A sync compares each side with that snapshot ([`plan`]):
//! - changed on one side only → copied to the other (a deletion too);
//! - changed on both sides → **both kept**: the local version becomes
//!   `name (conflict <host> <date>).ext` on both sides, the cloud version keeps the name;
//! - deleted on one side and changed on the other → the change wins.
//!
//! Nothing is ever deleted on a first sync (no snapshot: both sides are merged). Files deleted
//! on this computer because they were deleted in the cloud go to
//! `~/.lsuite/launcher/sync-trash/<id>/` instead of disappearing. A sync that would delete more
//! than half of a folder (over 10 files) stops and asks for `force`, in case a disk went missing.
//! Files only: empty folders aren't synced.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{CmdResult, Launcher, cloud, paths, util};

/// A synced pair.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pair {
    pub id: String,
    pub local: PathBuf,
    pub remote: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    pub last_sync: Option<chrono::DateTime<chrono::Utc>>,
    /// One line about the last sync ("3 up, 1 down" or the error).
    #[serde(default)]
    pub last_result: Option<String>,
    #[serde(default)]
    pub last_ok: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Entry {
    sha256: String,
    /// The local file's size and modification time when it was last hashed.
    size: u64,
    mtime: i128,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Snapshot {
    format: u32,
    files: BTreeMap<String, Entry>,
}

fn pairs_file() -> PathBuf {
    paths::launcher_dir().join("syncs.json")
}

fn snapshot_file(id: &str) -> PathBuf {
    paths::launcher_dir().join("sync").join(format!("{id}.json"))
}

pub fn pairs() -> Vec<Pair> {
    std::fs::read(pairs_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_pairs(p: &[Pair]) -> CmdResult<()> {
    util::write_private(&pairs_file(), &serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?).map_err(|e| format!("Couldn't save the synced folders ({e})."))
}

fn load_snapshot(id: &str) -> Snapshot {
    std::fs::read(snapshot_file(id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_snapshot(id: &str, s: &Snapshot) -> CmdResult<()> {
    util::write_private(&snapshot_file(id), &serde_json::to_vec(s).map_err(|e| e.to_string())?).map_err(|e| format!("Couldn't save the sync state ({e})."))
}

/// Starts syncing `local` with the cloud folder `remote`.
pub fn add(local: &Path, remote: &str) -> CmdResult<Pair> {
    let local = local.canonicalize().map_err(|_| format!("{} isn't a folder on this computer.", local.display()))?;
    if !local.is_dir() {
        return Err(format!("{} isn't a folder.", local.display()));
    }
    let remote = cloud::clean(remote, false).map_err(|_| "Choose a cloud folder to sync with (not the top of your cloud).".to_string())?;
    let mut all = pairs();
    for p in &all {
        if p.local.starts_with(&local) || local.starts_with(&p.local) {
            return Err(format!("{} is already synced (with {}).", p.local.display(), p.remote));
        }
        let (a, b) = (format!("{}/", p.remote), format!("{remote}/"));
        if a.starts_with(&b) || b.starts_with(&a) {
            return Err(format!("The cloud folder {} is already synced (with {}).", p.remote, p.local.display()));
        }
    }
    let pair = Pair { id: util::random_token().chars().filter(char::is_ascii_alphanumeric).take(12).collect(), local, remote, created_at: chrono::Utc::now(), last_sync: None, last_result: None, last_ok: false };
    all.push(pair.clone());
    save_pairs(&all)?;
    Ok(pair)
}

/// Stops syncing a pair. Files stay where they are, on both sides.
pub fn remove(id: &str) -> CmdResult<Value> {
    let mut all = pairs();
    let before = all.len();
    all.retain(|p| p.id != id);
    if all.len() == before {
        return Err(format!("No synced folder {id}."));
    }
    save_pairs(&all)?;
    let _ = std::fs::remove_file(snapshot_file(id));
    Ok(json!({ "removed": id, "kept": "The files stay on this computer and in your cloud." }))
}

/// What a sync does to one path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Upload,
    Download,
    DeleteLocal,
    DeleteRemote,
    /// Changed on both sides: both kept.
    Conflict,
    /// The same on both sides (the snapshot is brought up to date).
    Same,
}

/// Decides, path by path, from the snapshot (`base`) and each side's current SHA-256.
pub fn plan(base: &BTreeMap<String, String>, local: &BTreeMap<String, String>, remote: &BTreeMap<String, String>) -> BTreeMap<String, Action> {
    let all: BTreeSet<&String> = base.keys().chain(local.keys()).chain(remote.keys()).collect();
    let mut out = BTreeMap::new();
    for p in all {
        let (b, l, r) = (base.get(p), local.get(p), remote.get(p));
        let a = if l == r {
            Action::Same
        } else if b == l {
            if r.is_none() { Action::DeleteLocal } else { Action::Download }
        } else if b == r {
            if l.is_none() { Action::DeleteRemote } else { Action::Upload }
        } else {
            match (l, r) {
                (Some(_), Some(_)) => Action::Conflict,
                // Deleted here, changed there (or the reverse): the change wins.
                (None, Some(_)) => Action::Download,
                (Some(_), None) => Action::Upload,
                (None, None) => Action::Same,
            }
        };
        if !(a == Action::Same && l.is_none()) {
            out.insert(p.clone(), a);
        }
    }
    out
}

fn ignored(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini") || name.ends_with(".lsuite-download") || name.starts_with(".~lock.") || name.starts_with("~$")
}

fn mtime_of(m: &std::fs::Metadata) -> i128 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i128).unwrap_or(0)
}

fn sha_file(p: &Path) -> CmdResult<String> {
    let mut f = std::fs::File::open(p).map_err(|e| format!("Couldn't read {} ({e}).", p.display()))?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| format!("Couldn't read {} ({e}).", p.display()))?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The local folder's files (relative paths with `/`), hashed unless unchanged since the snapshot.
fn scan(root: &Path, snap: &Snapshot) -> CmdResult<BTreeMap<String, Entry>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        for e in std::fs::read_dir(&dir).map_err(|e| format!("Couldn't read {} ({e}).", dir.display()))?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if ignored(&name) {
                continue;
            }
            let r = cloud::join(&rel, &name);
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push((e.path(), r));
            } else if ft.is_file() {
                let Ok(m) = e.metadata() else { continue };
                let (size, mtime) = (m.len(), mtime_of(&m));
                let sha = match snap.files.get(&r) {
                    Some(s) if s.size == size && s.mtime == mtime && !s.sha256.is_empty() => s.sha256.clone(),
                    _ => sha_file(&e.path())?,
                };
                out.insert(r, Entry { sha256: sha, size, mtime });
            }
        }
    }
    Ok(out)
}

/// `report.pdf` → `report (conflict ludovic-thinkpad 2026-10-07 1215).pdf`.
fn conflict_name(rel: &str) -> String {
    let host = std::env::var("HOSTNAME").ok().or_else(|| std::fs::read_to_string("/etc/hostname").ok()).or_else(|| std::env::var("COMPUTERNAME").ok()).map(|h| h.trim().to_string()).filter(|h| !h.is_empty()).unwrap_or_else(|| "this computer".into());
    let when = chrono::Local::now().format("%Y-%m-%d %H%M");
    let (dir, name) = match rel.rsplit_once('/') {
        Some((d, n)) => (format!("{d}/"), n),
        None => (String::new(), rel),
    };
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    format!("{dir}{stem} (conflict {host} {when}){ext}")
}

fn local_path(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, s| p.join(s))
}

/// Syncs one pair now.
pub async fn sync_pair(l: &Arc<Launcher>, id: &str, force: bool) -> CmdResult<Value> {
    let pair = pairs().into_iter().find(|p| p.id == id).ok_or_else(|| format!("No synced folder {id}."))?;
    if !l.syncing.lock().insert(id.to_string()) {
        return Err(format!("{} is already syncing.", pair.local.display()));
    }
    let name = pair.local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| pair.remote.clone());
    let mut progress = l.hub.progress(format!("sync:{id}"), format!("Syncing {name}"));
    let result = run(&pair, force, &mut progress).await;
    l.syncing.lock().remove(id);
    let line = match &result {
        Ok(v) => v["summary"].as_str().unwrap_or("").to_string(),
        Err(e) => e.clone(),
    };
    let mut all = pairs();
    if let Some(p) = all.iter_mut().find(|p| p.id == id) {
        p.last_sync = Some(chrono::Utc::now());
        p.last_result = Some(line.clone());
        p.last_ok = result.is_ok();
    }
    let _ = save_pairs(&all);
    // Quiet when nothing moved; say it otherwise.
    let quiet = result.as_ref().is_ok_and(|v| v["changes"] == 0);
    if !quiet {
        progress.finish(&result, format!("{name}: {line}"));
    } else {
        progress.finish(&Ok::<(), String>(()), "");
    }
    l.hub.changed("cloud");
    l.hub.changed("sync");
    result
}

async fn run(pair: &Pair, force: bool, progress: &mut crate::events::Progress) -> CmdResult<Value> {
    if !pair.local.is_dir() {
        return Err(format!("{} isn't there any more (a disk not connected?). Nothing was changed.", pair.local.display()));
    }
    let mut snap = load_snapshot(&pair.id);
    progress.stage(format!("Looking at {}", pair.local.display()), 0, None);
    let (root, snap_for_scan) = (pair.local.clone(), Snapshot { format: 1, files: snap.files.clone() });
    let local = tokio::task::spawn_blocking(move || scan(&root, &snap_for_scan)).await.map_err(|e| e.to_string())??;
    let all = cloud::list_all().await?;
    let prefix = format!("{}/", pair.remote);
    let remote: BTreeMap<String, String> = all["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| Some((f["path"].as_str()?.strip_prefix(&prefix)?.to_string(), f["sha256"].as_str()?.to_string())))
        .collect();
    let base: BTreeMap<String, String> = snap.files.iter().map(|(k, v)| (k.clone(), v.sha256.clone())).collect();
    let local_sha: BTreeMap<String, String> = local.iter().map(|(k, v)| (k.clone(), v.sha256.clone())).collect();
    let actions = plan(&base, &local_sha, &remote);

    let count = |a: Action| actions.values().filter(|x| **x == a).count();
    let (del_l, del_r) = (count(Action::DeleteLocal), count(Action::DeleteRemote));
    let size = local.len().max(remote.len()).max(1);
    if !force && (del_l > 10 && del_l * 2 > size || del_r > 10 && del_r * 2 > size) {
        return Err(format!("This sync would delete {} files here and {} in the cloud: more than half of the folder. Nothing was changed; sync again with force if that is what you want.", del_l, del_r));
    }
    let todo: Vec<(&String, &Action)> = actions.iter().filter(|(_, a)| **a != Action::Same).collect();
    let total = todo.len() as u64;
    let trash = paths::launcher_dir().join("sync-trash").join(&pair.id).join(chrono::Local::now().format("%Y-%m-%d %H%M%S").to_string());
    let mut quiet = crate::Hub::default().progress("", "");
    let (mut up, mut down, mut gone_l, mut gone_r, mut conflicts) = (0, 0, 0, 0, vec![]);
    for (i, (rel, action)) in todo.iter().enumerate() {
        progress.stage(format!("Syncing {} ({}/{total})", rel, i + 1), i as u64, Some(total));
        let lp = local_path(&pair.local, rel);
        let rp = cloud::join(&pair.remote, rel);
        let step: CmdResult<()> = async {
            match action {
                Action::Upload => {
                    cloud::upload_file(&lp, &rp, true, &mut quiet).await?;
                    up += 1;
                }
                Action::Download => {
                    cloud::download_file(&rp, &lp, true, &mut quiet).await?;
                    down += 1;
                }
                Action::DeleteRemote => {
                    match cloud::delete(&rp).await {
                        Ok(_) => {}
                        Err(e) if e.contains("Nothing in lsuite Cloud") => {}
                        Err(e) => return Err(e),
                    }
                    gone_r += 1;
                }
                Action::DeleteLocal => {
                    let to = local_path(&trash, rel);
                    if let Some(d) = to.parent() {
                        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                    }
                    if std::fs::rename(&lp, &to).is_err() {
                        std::fs::copy(&lp, &to).map_err(|e| format!("Couldn't keep {} aside ({e}).", lp.display()))?;
                        std::fs::remove_file(&lp).map_err(|e| format!("Couldn't remove {} ({e}).", lp.display()))?;
                    }
                    gone_l += 1;
                }
                Action::Conflict => {
                    let other = conflict_name(rel);
                    let other_path = local_path(&pair.local, &other);
                    std::fs::rename(&lp, &other_path).map_err(|e| format!("Couldn't rename {} ({e}).", lp.display()))?;
                    cloud::upload_file(&other_path, &cloud::join(&pair.remote, &other), false, &mut quiet).await?;
                    cloud::download_file(&rp, &lp, true, &mut quiet).await?;
                    let m = std::fs::metadata(&other_path).map_err(|e| e.to_string())?;
                    snap.files.insert(other.clone(), Entry { sha256: sha_file(&other_path)?, size: m.len(), mtime: mtime_of(&m) });
                    conflicts.push(other);
                }
                Action::Same => {}
            }
            Ok(())
        }
        .await;
        step.map_err(|e| format!("{rel}: {e} (the rest of the sync stopped; files already done stay done)"))
            .inspect_err(|_| {
                // What went through before the error is recorded, so it isn't redone wrongly.
                let _ = save_snapshot(&pair.id, &snap);
            })?;
        // The snapshot follows each step.
        match action {
            Action::DeleteLocal | Action::DeleteRemote => {
                snap.files.remove(*rel);
            }
            _ => {
                if let Ok(m) = std::fs::metadata(&lp) {
                    let sha = remote.get(*rel).filter(|_| matches!(action, Action::Download | Action::Conflict)).cloned().or_else(|| local_sha.get(*rel).cloned()).unwrap_or_default();
                    snap.files.insert((*rel).clone(), Entry { sha256: sha, size: m.len(), mtime: mtime_of(&m) });
                }
            }
        }
    }
    // Paths the same on both sides: bring the snapshot up to date.
    for (rel, a) in &actions {
        if *a == Action::Same
            && let Some(e) = local.get(rel)
        {
            snap.files.insert(rel.clone(), e.clone());
        }
    }
    snap.files.retain(|k, _| local_path(&pair.local, k).is_file());
    snap.format = 1;
    save_snapshot(&pair.id, &snap)?;
    let changes = up + down + gone_l + gone_r + conflicts.len();
    let mut parts = vec![];
    if up > 0 {
        parts.push(format!("{up} up"));
    }
    if down > 0 {
        parts.push(format!("{down} down"));
    }
    if gone_l + gone_r > 0 {
        parts.push(format!("{} deleted", gone_l + gone_r));
    }
    if !conflicts.is_empty() {
        parts.push(format!("{} conflict{} (both kept)", conflicts.len(), if conflicts.len() == 1 { "" } else { "s" }));
    }
    let summary = if parts.is_empty() { "Up to date".to_string() } else { parts.join(", ") };
    Ok(json!({ "id": pair.id, "local": pair.local, "remote": pair.remote, "uploaded": up, "downloaded": down, "deletedHere": gone_l, "deletedInCloud": gone_r, "conflicts": conflicts, "changes": changes, "summary": summary, "trash": if gone_l > 0 { Some(trash) } else { None } }))
}

/// Syncs every pair (or one), one after the other.
pub async fn sync_all(l: &Arc<Launcher>, id: Option<&str>, force: bool) -> CmdResult<Value> {
    let ids: Vec<String> = match id {
        Some(i) => vec![i.to_string()],
        None => pairs().into_iter().map(|p| p.id).collect(),
    };
    if ids.is_empty() {
        return Ok(json!({ "synced": [], "message": "No folder is synced yet." }));
    }
    let mut out = vec![];
    for i in ids {
        out.push(match sync_pair(l, &i, force).await {
            Ok(v) => v,
            Err(e) => json!({ "id": i, "error": e }),
        });
    }
    if id.is_some()
        && let Some(e) = out[0]["error"].as_str()
    {
        return Err(e.to_string());
    }
    Ok(json!({ "synced": out }))
}

pub fn list(l: &Launcher) -> Value {
    let syncing = l.syncing.lock().clone();
    let pairs: Vec<Value> = pairs()
        .into_iter()
        .map(|p| {
            let mut v = serde_json::to_value(&p).unwrap_or_default();
            v["syncing"] = json!(syncing.contains(&p.id));
            v["present"] = json!(p.local.is_dir());
            v
        })
        .collect();
    json!({ "pairs": pairs })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn plans_each_case() {
        let base = m(&[("same", "a"), ("lchg", "a"), ("rchg", "a"), ("ldel", "a"), ("rdel", "a"), ("both", "a"), ("ldel-rchg", "a"), ("lchg-rdel", "a"), ("both-gone", "a"), ("same-change", "a")]);
        let local = m(&[("same", "a"), ("lchg", "b"), ("rchg", "a"), ("rdel", "a"), ("both", "b"), ("lchg-rdel", "b"), ("new-l", "x"), ("same-new", "z"), ("same-change", "q")]);
        let remote = m(&[("same", "a"), ("lchg", "a"), ("rchg", "c"), ("ldel", "a"), ("both", "c"), ("ldel-rchg", "c"), ("new-r", "y"), ("same-new", "z"), ("same-change", "q")]);
        let p = plan(&base, &local, &remote);
        let get = |k: &str| p.get(k).cloned();
        assert_eq!(get("same"), Some(Action::Same));
        assert_eq!(get("lchg"), Some(Action::Upload));
        assert_eq!(get("rchg"), Some(Action::Download));
        assert_eq!(get("ldel"), Some(Action::DeleteRemote));
        assert_eq!(get("rdel"), Some(Action::DeleteLocal));
        assert_eq!(get("both"), Some(Action::Conflict));
        assert_eq!(get("ldel-rchg"), Some(Action::Download));
        assert_eq!(get("lchg-rdel"), Some(Action::Upload));
        assert_eq!(get("both-gone"), None);
        assert_eq!(get("new-l"), Some(Action::Upload));
        assert_eq!(get("new-r"), Some(Action::Download));
        assert_eq!(get("same-new"), Some(Action::Same));
        assert_eq!(get("same-change"), Some(Action::Same));
    }

    #[test]
    fn a_first_sync_never_deletes() {
        let p = plan(&BTreeMap::new(), &m(&[("a", "1"), ("b", "2")]), &m(&[("b", "3"), ("c", "4")]));
        assert!(p.values().all(|a| !matches!(a, Action::DeleteLocal | Action::DeleteRemote)));
        assert_eq!(p["a"], Action::Upload);
        assert_eq!(p["b"], Action::Conflict);
        assert_eq!(p["c"], Action::Download);
    }

    #[test]
    fn conflict_names_keep_the_extension() {
        let n = conflict_name("Projects/report.final.pdf");
        assert!(n.starts_with("Projects/report.final (conflict "), "{n}");
        assert!(n.ends_with(").pdf"));
        assert!(conflict_name("Makefile").starts_with("Makefile (conflict "));
        assert!(ignored(".DS_Store") && ignored("~$doc.docx") && !ignored("doc.docx"));
    }
}
