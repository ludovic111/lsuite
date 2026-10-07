//! lsuite Cloud (lsuite's CLOUD.md): the storage that comes with an lsuite AI plan.
//!
//! A client of `<server>/api/cloud`, authenticated with the account's token. Files are named by
//! `/`-separated paths without a leading slash; each segment is percent-encoded in URLs.
//! Uploads stream from disk with their SHA-256 (the server checks it) and downloads are checked
//! against the server's `ETag` before they replace anything on disk. Folders go up and come down
//! whole, one file at a time.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::events::{Hub, Progress};
use crate::{CmdResult, account, util};

/// Everything but unreserved characters is encoded in a path segment.
const SEGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'%').add(b'/').add(b'<').add(b'>').add(b'?').add(b'`').add(b'{').add(b'}').add(b'\\').add(b'^').add(b'|').add(b'[').add(b']').add(b'+').add(b'&').add(b'=').add(b';').add(b':').add(b'@').add(b'$').add(b',').add(b'\'');

/// A path as the server names it: segments joined by `/`, checked against CLOUD.md's rules.
/// `""` (the root) is allowed only where `root_ok`.
pub fn clean(path: &str, root_ok: bool) -> CmdResult<String> {
    let p = path.trim().trim_matches('/');
    if p.is_empty() {
        return if root_ok { Ok(String::new()) } else { Err("Give a path in your cloud (for example Projects/demo.folio).".into()) };
    }
    if p.len() > 1024 {
        return Err("That path is longer than 1024 bytes.".into());
    }
    let mut out = vec![];
    for seg in p.split('/') {
        if seg.is_empty() {
            continue;
        }
        if seg == "." || seg == ".." {
            return Err(format!("{path:?}: a path can't contain . or .. as a name."));
        }
        if seg.len() > 255 {
            return Err(format!("{path:?}: a name is longer than 255 bytes."));
        }
        if seg.contains('\\') || seg.chars().any(char::is_control) {
            return Err(format!("{path:?}: names can't contain \\ or control characters."));
        }
        if seg != seg.trim() {
            return Err(format!("{path:?}: names can't start or end with a space."));
        }
        out.push(seg);
    }
    Ok(out.join("/"))
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") }
}

/// The last segment.
pub fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The folder a path is in (`""` at the root).
pub fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn encode(path: &str) -> String {
    path.split('/').map(|s| utf8_percent_encode(s, SEGMENT).to_string()).collect::<Vec<_>>().join("/")
}

fn auth() -> CmdResult<(String, String)> {
    let acc = account::read().ok_or("Sign in to lsuite first: lsuite Cloud comes with an lsuite AI plan.")?;
    Ok((acc.server, acc.token))
}

async fn fail(r: reqwest::Response) -> String {
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    let mut line = account::error_line(status, &body);
    if matches!(body["error"]["type"].as_str(), Some("storage_full" | "plan_required")) && !line.contains("/account") {
        line.push_str(&format!(" Manage plan: {}", body["error"]["manage_url"].as_str().map(str::to_string).unwrap_or_else(account::manage_url)));
    }
    line
}

async fn get_json(path: &str) -> CmdResult<Value> {
    let (server, token) = auth()?;
    let r = util::client().get(format!("{server}{path}")).bearer_auth(token).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
    if !r.status().is_success() {
        return Err(fail(r).await);
    }
    r.json().await.map_err(|e| format!("The server's answer isn't JSON ({e})."))
}

async fn send_json(method: reqwest::Method, path: &str, body: Value) -> CmdResult<Value> {
    let (server, token) = auth()?;
    let r = util::client().request(method, format!("{server}{path}")).bearer_auth(token).json(&body).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
    if !r.status().is_success() {
        return Err(fail(r).await);
    }
    r.json().await.map_err(|e| format!("The server's answer isn't JSON ({e})."))
}

/// `GET /api/cloud`: the plan's storage and what is used.
pub async fn status() -> CmdResult<Value> {
    let mut v = get_json("/api/cloud").await?;
    let (used, quota) = (v["used"].as_u64().unwrap_or(0), v["quota"].as_u64().unwrap_or(0));
    v["summary"] = json!(if quota == 0 { format!("{} used · no storage on this plan", util::bytes(used)) } else { format!("{} of {} used", util::bytes(used), util::bytes(quota)) });
    Ok(v)
}

/// Everything in the cloud, flat (`files`, `folders`, `used`, `quota`).
pub async fn list_all() -> CmdResult<Value> {
    get_json("/api/cloud/files").await
}

/// One folder's content: its subfolders (implied by files, or created on their own) and files.
pub fn folder_view(all: &Value, dir: &str) -> Value {
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    let mut folders = std::collections::BTreeMap::<String, (u64, u64)>::new();
    let mut files = vec![];
    for f in all["files"].as_array().into_iter().flatten() {
        let Some(p) = f["path"].as_str() else { continue };
        let Some(rest) = p.strip_prefix(&prefix) else { continue };
        match rest.split_once('/') {
            Some((sub, _)) => {
                let e = folders.entry(sub.to_string()).or_default();
                e.0 += 1;
                e.1 += f["size"].as_u64().unwrap_or(0);
            }
            None => files.push(f.clone()),
        }
    }
    for f in all["folders"].as_array().into_iter().flatten() {
        let Some(p) = f["path"].as_str() else { continue };
        if let Some(rest) = p.strip_prefix(&prefix) {
            let sub = rest.split('/').next().unwrap_or(rest);
            if !sub.is_empty() {
                folders.entry(sub.to_string()).or_default();
            }
        }
    }
    let folders: Vec<Value> = folders.into_iter().map(|(name, (count, size))| json!({ "name": name, "path": join(dir, &name), "files": count, "size": size })).collect();
    json!({ "path": dir, "folders": folders, "files": files, "used": all["used"], "quota": all["quota"] })
}

pub async fn mkdir(path: &str) -> CmdResult<Value> {
    send_json(reqwest::Method::POST, "/api/cloud/folders", json!({ "path": clean(path, false)? })).await
}

pub async fn move_to(from: &str, to: &str, overwrite: bool) -> CmdResult<Value> {
    send_json(reqwest::Method::POST, "/api/cloud/move", json!({ "from": clean(from, false)?, "to": clean(to, false)?, "overwrite": overwrite })).await
}

pub async fn delete(path: &str) -> CmdResult<Value> {
    let (server, token) = auth()?;
    let p = clean(path, false)?;
    let r = util::client().delete(format!("{server}/api/cloud/files/{}", encode(&p))).bearer_auth(token).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
    if !r.status().is_success() {
        return Err(fail(r).await);
    }
    r.json().await.map_err(|e| e.to_string())
}

async fn sha256_file(path: &Path) -> CmdResult<String> {
    let mut f = tokio::fs::File::open(path).await.map_err(|e| format!("Couldn't read {} ({e}).", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Uploads one file to `remote` (a file path), replacing a file there unless `!overwrite`.
pub async fn upload_file(local: &Path, remote: &str, overwrite: bool, progress: &mut Progress) -> CmdResult<Value> {
    let (server, token) = auth()?;
    let remote = clean(remote, false)?;
    let meta = tokio::fs::metadata(local).await.map_err(|e| format!("Couldn't read {} ({e}).", local.display()))?;
    if !meta.is_file() {
        return Err(format!("{} isn't a file.", local.display()));
    }
    let size = meta.len();
    let sha = sha256_file(local).await?;
    let modified = meta.modified().ok().map(chrono::DateTime::<chrono::Utc>::from).unwrap_or_else(chrono::Utc::now);
    let file = tokio::fs::File::open(local).await.map_err(|e| format!("Couldn't read {} ({e}).", local.display()))?;
    let (tx, rx) = tokio::sync::mpsc::channel::<u64>(64);
    let body = futures::stream::unfold((file, 0u64, tx), |(mut file, sent, tx)| async move {
        let mut buf = vec![0u8; 1 << 16];
        match file.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                let sent = sent + n as u64;
                let _ = tx.try_send(sent);
                Some((Ok::<_, std::io::Error>(buf), (file, sent, tx)))
            }
            Err(e) => Some((Err(e), (file, sent, tx))),
        }
    });
    let mut req = util::transfer_client()
        .put(format!("{server}/api/cloud/files/{}", encode(&remote)))
        .bearer_auth(token)
        .header(reqwest::header::CONTENT_LENGTH, size)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header("x-lsuite-sha256", &sha)
        .header("x-lsuite-modified", modified.to_rfc3339())
        .body(reqwest::Body::wrap_stream(body));
    if !overwrite {
        req = req.header(reqwest::header::IF_NONE_MATCH, "*");
    }
    progress.update(0, Some(size));
    let send = req.send();
    tokio::pin!(send);
    let mut rx = rx;
    let r = loop {
        tokio::select! {
            r = &mut send => break r,
            Some(n) = rx.recv() => progress.update(n, Some(size)),
        }
    }
    .map_err(|e| format!("The upload of {} failed ({e}).", local.display()))?;
    if r.status().as_u16() == 412 {
        return Err(format!("{remote} is already in your cloud. Replace it, or upload under another name."));
    }
    if !r.status().is_success() {
        return Err(fail(r).await);
    }
    progress.update(size, Some(size));
    r.json().await.map_err(|e| e.to_string())
}

/// Files on disk with their path in the cloud, and the empty folders.
type Walked = (Vec<(PathBuf, String)>, Vec<String>);

/// The files under `local` (a folder), relative paths with `/`, and the empty folders.
fn walk(local: &Path) -> CmdResult<Walked> {
    let mut files = vec![];
    let mut empty = vec![];
    let mut stack = vec![(local.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        let mut any = false;
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("Couldn't read {} ({e}).", dir.display()))?;
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // Leave out what systems scatter in folders.
            if matches!(name.as_str(), ".DS_Store" | "Thumbs.db" | "desktop.ini") {
                continue;
            }
            any = true;
            let r = join(&rel, &name);
            let ft = e.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                stack.push((e.path(), r));
            } else if ft.is_file() {
                files.push((e.path(), r));
            }
        }
        if !any && !rel.is_empty() {
            empty.push(rel);
        }
    }
    files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok((files, empty))
}

/// Uploads a file or a whole folder into the cloud folder `into` (`""`: the root), under its
/// own name. Returns what was uploaded.
pub async fn upload(local: &Path, into: &str, overwrite: bool, hub: &Hub) -> CmdResult<Value> {
    let into = clean(into, true)?;
    let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| format!("{} has no name.", local.display()))?;
    let target = clean(&join(&into, &name), false)?;
    let task = format!("upload:{target}");
    let mut progress = hub.progress(task, format!("Uploading {name}"));
    let meta = std::fs::metadata(local).map_err(|e| format!("Couldn't read {} ({e}).", local.display()));
    let result = async {
        let meta = meta?;
        if meta.is_file() {
            let f = upload_file(local, &target, overwrite, &mut progress).await?;
            return Ok(json!({ "uploaded": 1, "path": target, "file": f["file"] }));
        }
        let (files, empty) = walk(local)?;
        let total: u64 = files.iter().filter_map(|(p, _)| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
        let count = files.len();
        let mut done = 0u64;
        if files.is_empty() {
            mkdir(&target).await?;
        }
        for d in &empty {
            mkdir(&join(&target, d)).await?;
        }
        for (i, (path, rel)) in files.iter().enumerate() {
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let mut inner = Hub::default().progress("", "");
            progress.stage(format!("Uploading {name} ({}/{count})", i + 1), done, Some(total));
            upload_file(path, &join(&target, rel), overwrite, &mut inner).await.map_err(|e| format!("{rel}: {e}"))?;
            done += size;
            progress.update(done, Some(total));
        }
        Ok(json!({ "uploaded": count, "path": target, "size": total }))
    }
    .await;
    progress.finish(&result, format!("Uploaded {name}"));
    hub.changed("cloud");
    result
}

/// Downloads one file to `dest` (a file path), checked against the server's SHA-256.
pub async fn download_file(remote: &str, dest: &Path, overwrite: bool, progress: &mut Progress) -> CmdResult<u64> {
    let (server, token) = auth()?;
    let remote = clean(remote, false)?;
    if dest.exists() && !overwrite {
        return Err(format!("{} already exists. Replace it, or choose another place.", dest.display()));
    }
    let r = util::transfer_client().get(format!("{server}/api/cloud/files/{}", encode(&remote))).bearer_auth(token).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
    if !r.status().is_success() {
        return Err(fail(r).await);
    }
    let want = r.headers().get(reqwest::header::ETAG).and_then(|v| v.to_str().ok()).map(|s| s.trim_matches('"').to_ascii_lowercase());
    let total = r.content_length();
    if let Some(d) = dest.parent() {
        tokio::fs::create_dir_all(d).await.map_err(|e| format!("Couldn't create {} ({e}).", d.display()))?;
    }
    let tmp = dest.with_file_name(format!(".{}.lsuite-download", dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    let result = async {
        let mut out = tokio::fs::File::create(&tmp).await.map_err(|e| format!("Couldn't write {} ({e}).", tmp.display()))?;
        let mut h = Sha256::new();
        let mut got = 0u64;
        let mut stream = r.bytes_stream();
        loop {
            let next = tokio::time::timeout(Duration::from_secs(60), stream.next()).await.map_err(|_| "The download stalled; try again.".to_string())?;
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|e| format!("The download failed ({e})."))?;
            got += chunk.len() as u64;
            h.update(&chunk);
            out.write_all(&chunk).await.map_err(|e| e.to_string())?;
            progress.update(got, total);
        }
        out.flush().await.map_err(|e| e.to_string())?;
        drop(out);
        let sum: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if want.as_ref().is_some_and(|w| w.len() == 64 && *w != sum) || total.is_some_and(|t| t != got) {
            return Err(format!("{remote} didn't arrive whole; nothing was written. Try again."));
        }
        tokio::fs::rename(&tmp, dest).await.map_err(|e| format!("Couldn't write {} ({e}).", dest.display()))?;
        Ok(got)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    result
}

/// Downloads a file or a whole folder from the cloud into the local folder `into`.
pub async fn download(remote: &str, into: &Path, overwrite: bool, hub: &Hub) -> CmdResult<Value> {
    let remote = clean(remote, false)?;
    let name = name_of(&remote).to_string();
    let mut progress = hub.progress(format!("download:{remote}"), format!("Downloading {name}"));
    let result = async {
        let all = list_all().await?;
        let files = all["files"].as_array().cloned().unwrap_or_default();
        let dest = into.join(&name);
        if let Some(f) = files.iter().find(|f| f["path"].as_str() == Some(remote.as_str())) {
            progress.update(0, f["size"].as_u64());
            let n = download_file(&remote, &dest, overwrite, &mut progress).await?;
            return Ok(json!({ "downloaded": 1, "to": dest, "size": n }));
        }
        let prefix = format!("{remote}/");
        let inside: Vec<&Value> = files.iter().filter(|f| f["path"].as_str().is_some_and(|p| p.starts_with(&prefix))).collect();
        let is_folder = !inside.is_empty() || all["folders"].as_array().into_iter().flatten().any(|f| f["path"].as_str().is_some_and(|p| p == remote || p.starts_with(&prefix)));
        if !is_folder {
            return Err(format!("There's no {remote} in your cloud."));
        }
        if dest.exists() && !overwrite {
            return Err(format!("{} already exists. Replace it, or choose another folder.", dest.display()));
        }
        let total: u64 = inside.iter().filter_map(|f| f["size"].as_u64()).sum();
        let mut done = 0u64;
        std::fs::create_dir_all(&dest).map_err(|e| format!("Couldn't create {} ({e}).", dest.display()))?;
        for f in all["folders"].as_array().into_iter().flatten() {
            if let Some(rel) = f["path"].as_str().and_then(|p| p.strip_prefix(&prefix)) {
                let _ = std::fs::create_dir_all(dest.join(rel));
            }
        }
        let count = inside.len();
        for (i, f) in inside.iter().enumerate() {
            let p = f["path"].as_str().unwrap_or_default();
            let rel = &p[prefix.len()..];
            progress.stage(format!("Downloading {name} ({}/{count})", i + 1), done, Some(total));
            let mut inner = Hub::default().progress("", "");
            done += download_file(p, &dest.join(rel), overwrite, &mut inner).await.map_err(|e| format!("{rel}: {e}"))?;
            progress.update(done, Some(total));
        }
        Ok(json!({ "downloaded": count, "to": dest, "size": total }))
    }
    .await;
    progress.finish(&result, format!("Downloaded {name}"));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_follow_the_rules() {
        assert_eq!(clean("/Projects//ryolune/demo.ryolune/", false).unwrap(), "Projects/ryolune/demo.ryolune");
        assert_eq!(clean("", true).unwrap(), "");
        assert!(clean("", false).is_err());
        assert!(clean("a/../b", false).is_err());
        assert!(clean("a/ b", false).is_err());
        assert!(clean("a\\b", false).is_err());
        assert!(clean(&"x".repeat(256), false).is_err());
        assert_eq!(encode("Mes projets/été #1?.folio"), "Mes%20projets/%C3%A9t%C3%A9%20%231%3F.folio");
        assert_eq!(parent_of("a/b/c"), "a/b");
        assert_eq!(parent_of("c"), "");
        assert_eq!(name_of("a/b/c"), "c");
    }

    #[test]
    fn folders_show_their_direct_content() {
        let all = json!({
            "files": [
                { "path": "a.txt", "size": 1 },
                { "path": "Projects/x.folio", "size": 10 },
                { "path": "Projects/sub/y.folio", "size": 5 },
                { "path": "Music/z.ryolune", "size": 7 }
            ],
            "folders": [{ "path": "Empty" }, { "path": "Projects/Later" }],
            "used": 23, "quota": 100
        });
        let root = folder_view(&all, "");
        let names: Vec<&str> = root["folders"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Empty", "Music", "Projects"]);
        assert_eq!(root["files"].as_array().unwrap().len(), 1);
        let p = folder_view(&all, "Projects");
        assert_eq!(p["files"][0]["path"], "Projects/x.folio");
        let names: Vec<&str> = p["folders"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Later", "sub"]);
        assert_eq!(p["folders"][1]["size"], 5);
    }
}
