//! The lsuite Marketplace (lsuite's MARKETPLACE.md): plugins for the apps, made by the people who
//! use lsuite and by lsuite itself, every version reviewed before it can be installed.
//!
//! Browsing needs nothing; installing needs a paid lsuite Pass plan (the server says so with
//! `plan_required`). An install checks the file's SHA-256 against the listing, unpacks the bundle
//! (one top folder: `plugin.toml` and the library) into `~/.lsuite/plugins/<app>/<id>/`
//! (PLUGINS.md), records it in `~/.lsuite/launcher/plugins.json` and asks a running app to
//! `plugin.rescan` through its CLI. Publishing packs a bundle folder (what `plugin.publishLocal`
//! makes) for this computer's platform and submits it for review.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::events::Hub;
use crate::{CmdResult, Launcher, account, catalog, cloud, install, paths, util};

/// A plugin the launcher installed from the marketplace.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub id: String,
    pub app: String,
    pub version: String,
    pub sha256: String,
    pub path: PathBuf,
    pub installed_at: chrono::DateTime<chrono::Utc>,
}

fn records_file() -> PathBuf {
    paths::launcher_dir().join("plugins.json")
}

pub fn installed() -> BTreeMap<String, Installed> {
    std::fs::read(records_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(all: &BTreeMap<String, Installed>) -> CmdResult<()> {
    util::write_private(&records_file(), &serde_json::to_vec_pretty(all).map_err(|e| e.to_string())?).map_err(|e| format!("Couldn't save the installed plugins ({e})."))
}

/// `~/.lsuite/plugins/<app>/`.
pub fn plugins_dir(app: &str) -> PathBuf {
    paths::lsuite_home().join("plugins").join(app)
}

fn server() -> String {
    account::server()
}

fn token() -> Option<String> {
    account::read().map(|a| a.token)
}

async fn get_json(path: &str, auth: bool) -> CmdResult<Value> {
    let server = server();
    let mut req = util::client().get(format!("{server}{path}"));
    if auth && let Some(t) = token() {
        req = req.bearer_auth(t);
    }
    let r = req.send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    if status >= 400 {
        return Err(line(status, &body));
    }
    Ok(body)
}

/// One line from an error, with the Pass page when the plan is missing.
fn line(status: u16, body: &Value) -> String {
    let mut l = account::error_line(status, body);
    if body["error"]["type"] == "plan_required" && !l.contains("/pass") && !l.contains("/account") {
        l.push_str(&format!(" {}/pass", server()));
    }
    l
}

/// The listings (approved only), each with what is installed here.
pub async fn list(app: Option<&str>) -> CmdResult<Value> {
    let q = app.filter(|a| !a.is_empty()).map(|a| format!("?app={a}")).unwrap_or_default();
    let mut v = get_json(&format!("/api/marketplace{q}"), true).await?;
    let here = installed();
    let platform = crate::Platform::current().key();
    if let Some(list) = v["plugins"].as_array_mut() {
        for p in list.iter_mut() {
            let id = p["id"].as_str().unwrap_or("").to_string();
            let mine = here.get(&id);
            let latest = p["version"].as_str().unwrap_or("").to_string();
            p["installed"] = json!(mine.map(|m| m.version.clone()));
            p["updateAvailable"] = json!(mine.is_some_and(|m| crate::release::is_newer(&latest, &m.version)));
            p["availableHere"] = json!(p["platforms"].get(platform).is_some());
        }
    }
    v["platform"] = json!(platform);
    v["signedIn"] = json!(account::read().is_some());
    Ok(v)
}

/// The account's own submissions.
pub async fn mine() -> CmdResult<Value> {
    if token().is_none() {
        return Err("Sign in to lsuite to see your plugins.".into());
    }
    get_json("/api/marketplace/mine", true).await
}

/// Installs (or updates) a plugin for this computer.
pub async fn install(l: &Launcher, id: &str) -> CmdResult<Value> {
    let tok = token().ok_or("Sign in to lsuite first: the marketplace comes with lsuite Pass.")?;
    let listing = get_json(&format!("/api/marketplace/plugins/{}", enc(id)), true).await?;
    let app = listing["app"].as_str().ok_or("The listing names no app.")?.to_string();
    catalog::find(&app)?;
    let platform = crate::Platform::current().key();
    let want = listing["platforms"][platform]["sha256"].as_str().map(str::to_ascii_lowercase).ok_or_else(|| format!("{} has no build for this computer ({platform}) yet.", listing["name"].as_str().unwrap_or(id)))?;
    let version = listing["version"].as_str().unwrap_or("").to_string();
    let name = listing["name"].as_str().unwrap_or(id).to_string();
    let mut progress = l.hub.progress(format!("plugin:{id}"), format!("Downloading {name} {version}"));
    let result = async {
        let server = server();
        let url = format!("{server}/api/marketplace/plugins/{}/download?platform={platform}&version={}", enc(id), enc(&version));
        let r = util::transfer_client().get(url).bearer_auth(&tok).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
        if !r.status().is_success() {
            let status = r.status().as_u16();
            let body: Value = r.json().await.unwrap_or(Value::Null);
            return Err(line(status, &body));
        }
        let total = r.content_length();
        let dir = paths::downloads_dir();
        tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
        let file = dir.join(format!("plugin-{}-{version}.tar.gz", util::random_token().get(..8).unwrap_or("x")));
        let mut out = tokio::fs::File::create(&file).await.map_err(|e| e.to_string())?;
        let mut h = Sha256::new();
        let (mut got, mut stream) = (0u64, r.bytes_stream());
        while let Some(chunk) = tokio::time::timeout(Duration::from_secs(60), stream.next()).await.map_err(|_| "The download stalled; try again.".to_string())? {
            let chunk = chunk.map_err(|e| format!("The download failed ({e})."))?;
            got += chunk.len() as u64;
            h.update(&chunk);
            out.write_all(&chunk).await.map_err(|e| e.to_string())?;
            progress.update(got, total);
        }
        out.flush().await.map_err(|e| e.to_string())?;
        drop(out);
        let sum: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if sum != want {
            let _ = std::fs::remove_file(&file);
            return Err(format!("{name} doesn't match the marketplace's checksum; it was not installed."));
        }
        progress.stage(format!("Installing {name} {version}"), 0, None);
        let (f, a, i) = (file.clone(), app.clone(), id.to_string());
        let path = tokio::task::spawn_blocking(move || put_in_place(&f, &a, &i)).await.map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&file);
        let path = path??;
        let mut all = installed();
        all.insert(id.to_string(), Installed { id: id.to_string(), app: app.clone(), version: version.clone(), sha256: sum, path: path.clone(), installed_at: chrono::Utc::now() });
        save(&all)?;
        let rescanned = rescan(&app).await;
        Ok(json!({ "id": id, "app": app, "version": version, "path": path, "rescanned": rescanned, "message": if rescanned { format!("{name} {version} is installed and loaded in {app}.") } else { format!("{name} {version} is installed. {app} loads it when it next starts (or Plugins › Rescan).") } }))
    }
    .await;
    let done = result.as_ref().ok().and_then(|v| v["message"].as_str().map(str::to_string)).unwrap_or_default();
    progress.finish(&result, done);
    l.hub.changed("market");
    result
}

/// Unpacks a bundle archive beside where it goes and swaps it in.
fn put_in_place(archive: &Path, app: &str, id: &str) -> CmdResult<PathBuf> {
    put_in_place_at(&paths::lsuite_home().join("plugins"), archive, app, id)
}

/// The same into `<root>/<app>/<id>/`.
fn put_in_place_at(root: &Path, archive: &Path, app: &str, id: &str) -> CmdResult<PathBuf> {
    let dir = root.join(app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {} ({e}).", dir.display()))?;
    let staging = dir.join(format!(".lsuite-plugin-{}", util::random_token().get(..10).unwrap_or("x")));
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        install::untar(archive, &staging)?;
        let tops: Vec<PathBuf> = std::fs::read_dir(&staging).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
        let [top] = tops.as_slice() else { return Err("The plugin archive doesn't hold one folder.".to_string()) };
        let manifest = read_manifest(top)?;
        if manifest.id != id || manifest.app != app {
            return Err(format!("The plugin archive is {} for {}, not {id} for {app}; it was not installed.", manifest.id, manifest.app));
        }
        let dest = dir.join(id);
        install::swap_in(top, &dest)?;
        Ok(dest)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
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

/// Removes a plugin the launcher installed.
pub async fn remove(l: &Launcher, id: &str) -> CmdResult<Value> {
    let mut all = installed();
    let Some(rec) = all.remove(id) else { return Err(format!("{id} wasn't installed from the marketplace.")) };
    if rec.path.starts_with(paths::lsuite_home().join("plugins")) && rec.path.exists() {
        std::fs::remove_dir_all(&rec.path).map_err(|e| format!("Couldn't remove {} ({e}).", rec.path.display()))?;
    }
    save(&all)?;
    let rescanned = rescan(&rec.app).await;
    l.hub.changed("market");
    Ok(json!({ "removed": id, "app": rec.app, "rescanned": rescanned }))
}

/// `plugin.toml`'s fields the marketplace needs.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub app: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub abi: Option<i64>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub library: BTreeMap<String, String>,
}

pub fn read_manifest(bundle: &Path) -> CmdResult<Manifest> {
    let text = std::fs::read_to_string(bundle.join("plugin.toml")).map_err(|_| format!("{} has no plugin.toml: give the plugin's bundle folder (what plugin.publishLocal makes).", bundle.display()))?;
    toml::from_str(&text).map_err(|e| format!("plugin.toml isn't valid ({e})."))
}

/// The `[library]` key for this computer.
fn library_key() -> &'static str {
    match crate::Platform::current().os {
        crate::platform::Os::Macos => "macos",
        crate::platform::Os::Linux => "linux",
        crate::platform::Os::Windows => "windows",
    }
}

/// Packs a bundle folder as `<id>/…` in a `.tar.gz`.
fn pack(bundle: &Path, id: &str, out: &Path) -> CmdResult<()> {
    let f = std::fs::File::create(out).map_err(|e| e.to_string())?;
    let gz = flate2::write::GzEncoder::new(f, flate2::Compression::default());
    let mut t = tar::Builder::new(gz);
    t.follow_symlinks(false);
    t.append_dir_all(id, bundle).map_err(|e| format!("Couldn't pack {} ({e}).", bundle.display()))?;
    t.into_inner().and_then(|gz| gz.finish()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Submits a bundle folder for review, for this computer's platform.
pub async fn publish(hub: &Hub, bundle: &Path, notes: &str) -> CmdResult<Value> {
    let tok = token().ok_or("Sign in to lsuite first (Account) to publish on the marketplace.")?;
    let m = read_manifest(bundle)?;
    catalog::find(&m.app)?;
    let lib = m.library.get(library_key()).ok_or_else(|| format!("plugin.toml has no [library] {} entry: build the plugin on this computer first.", library_key()))?;
    if !bundle.join(lib).is_file() {
        return Err(format!("{lib} isn't in {}: build the plugin first (plugin.build, then plugin.publishLocal).", bundle.display()));
    }
    let platform = crate::Platform::current().key();
    let mut progress = hub.progress(format!("publish:{}", m.id), format!("Publishing {} {}", m.name, m.version));
    let result = async {
        let server = server();
        let body = json!({ "id": m.id, "app": m.app, "name": m.name, "kind": m.kind, "version": m.version, "abi": m.abi, "description": m.description, "notes": notes });
        let r = util::client().post(format!("{server}/api/marketplace/submit")).bearer_auth(&tok).json(&body).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(line(status, &v));
        }
        let archive = paths::downloads_dir().join(format!("{}-{}-{platform}.tar.gz", m.id, m.version));
        std::fs::create_dir_all(paths::downloads_dir()).map_err(|e| e.to_string())?;
        let (b, id, a) = (bundle.to_path_buf(), m.id.clone(), archive.clone());
        tokio::task::spawn_blocking(move || pack(&b, &id, &a)).await.map_err(|e| e.to_string())??;
        let bytes = std::fs::read(&archive).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&archive);
        let sha: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        progress.update(0, Some(bytes.len() as u64));
        let size = bytes.len() as u64;
        let r = util::transfer_client()
            .put(format!("{server}/api/marketplace/submit/{}/{}/{platform}", enc(&m.id), enc(&m.version)))
            .bearer_auth(&tok)
            .header(reqwest::header::CONTENT_TYPE, "application/gzip")
            .header("x-lsuite-sha256", &sha)
            .body(bytes)
            .send()
            .await
            .map_err(|e| format!("The upload failed ({e})."))?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if status >= 400 {
            return Err(line(status, &v));
        }
        progress.update(size, Some(size));
        let state = v["submission"]["status"].as_str().unwrap_or("pending").to_string();
        let message = if state == "approved" { format!("{} {} is published on the marketplace.", m.name, m.version) } else { format!("{} {} ({platform}) is submitted: lsuite reviews every version before it's listed.", m.name, m.version) };
        Ok(json!({ "id": m.id, "version": m.version, "platform": platform, "status": state, "submission": v["submission"], "message": message }))
    }
    .await;
    let done = result.as_ref().ok().and_then(|v| v["message"].as_str().map(str::to_string)).unwrap_or_default();
    progress.finish(&result, done);
    hub.changed("market");
    result
}

fn enc(s: &str) -> String {
    cloud::encode_segment(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_and_packing() {
        let d = tempfile::tempdir().unwrap();
        let b = d.path().join("bundle");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("plugin.toml"), "id = \"com.example.warm\"\nname = \"Warm\"\nversion = \"1.0.0\"\napp = \"ryolune\"\nkind = \"effect\"\nabi = 1\ndescription = \"Tape.\"\n\n[library]\nmacos = \"libwarm.dylib\"\nlinux = \"libwarm.so\"\nwindows = \"warm.dll\"\n").unwrap();
        std::fs::write(b.join("libwarm.so"), b"ELF").unwrap();
        let m = read_manifest(&b).unwrap();
        assert_eq!((m.id.as_str(), m.app.as_str(), m.abi), ("com.example.warm", "ryolune", Some(1)));
        assert_eq!(m.library["linux"], "libwarm.so");
        let archive = d.path().join("warm.tar.gz");
        pack(&b, &m.id, &archive).unwrap();
        // Unpacking it the way an install does gives one folder holding the bundle.
        let root = d.path().join("plugins");
        let dest = put_in_place_at(&root, &archive, "ryolune", "com.example.warm").unwrap();
        assert_eq!(dest, root.join("ryolune/com.example.warm"));
        assert!(dest.join("plugin.toml").is_file() && dest.join("libwarm.so").is_file());
        // A second install replaces it in place.
        put_in_place_at(&root, &archive, "ryolune", "com.example.warm").unwrap();
        assert_eq!(std::fs::read_dir(root.join("ryolune")).unwrap().count(), 1);
        assert!(put_in_place_at(&root, &archive, "kimchi", "com.example.warm").unwrap_err().contains("not com.example.warm for kimchi"));
        assert!(read_manifest(d.path()).unwrap_err().contains("plugin.toml"));
    }
}
