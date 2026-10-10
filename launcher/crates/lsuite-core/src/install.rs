//! Installing, updating, removing and opening the apps.
//!
//! What is installed is worked out from three sources, best first:
//! 1. `~/.lsuite/launcher/installed.json`: what the launcher installed itself, and where.
//! 2. The app's discovery file `~/.lsuite/apps/<app>.json`, written by the app when it starts
//!    (STANDARD.md section 4): its version, path and whether it is running.
//! 3. The usual places: `<app>.app` in `/Applications` or `~/Applications` (macOS), the
//!    launcher's own folder (Linux, Windows portable), the installer's folder (Windows).
//!
//! Installing downloads the release verified (`release.rs`), unpacks it next to where it goes,
//! and swaps it in: the copy already there is moved aside and removed only once the new one is
//! in place, so a failure leaves the old app working. A running app is never replaced or
//! removed (quit it first). Removing an app removes the app only: its documents, settings and
//! data folders stay. Copies the launcher didn't install, outside the usual places (a build
//! from source), are never touched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::catalog::{self, App};
use crate::events::Progress;
use crate::platform::{Os, Platform};
use crate::release::{self, FileKind, Release};
use crate::{CmdResult, paths, util};

/// One app the launcher installed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub app: String,
    pub version: String,
    /// What was put in place: the `.app` bundle, the app's folder (which holds the AppImage or
    /// the binaries), or the installer's folder on Windows.
    pub path: PathBuf,
    pub executable: PathBuf,
    pub kind: FileKind,
    pub file: String,
    pub installed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RecordsFile {
    format: u32,
    #[serde(default)]
    apps: BTreeMap<String, Record>,
}

pub fn records() -> BTreeMap<String, Record> {
    std::fs::read(paths::installed_file())
        .ok()
        .and_then(|b| serde_json::from_slice::<RecordsFile>(&b).ok())
        .filter(|f| f.format == 1)
        .map(|f| f.apps)
        .unwrap_or_default()
}

fn save_records(apps: BTreeMap<String, Record>) -> CmdResult<()> {
    let bytes = serde_json::to_vec_pretty(&RecordsFile { format: 1, apps }).map_err(|e| e.to_string())?;
    util::write_private(&paths::installed_file(), &bytes).map_err(|e| format!("Couldn't write {} ({e}).", paths::installed_file().display()))
}

/// An app's discovery file, if it is format 1.
pub fn discovery(id: &str) -> Option<Value> {
    let v: Value = serde_json::from_slice(&std::fs::read(paths::discovery_dir().join(format!("{id}.json"))).ok()?).ok()?;
    (v["format"].as_u64() == Some(1) && v["app"].as_str() == Some(id)).then_some(v)
}

/// The pid of the app's running copy: from its discovery file, else from the system's process
/// list (a copy started before it wrote one, or another `LSUITE_HOME`).
pub fn running_pid(id: &str) -> Option<u32> {
    if let Some(pid) = discovery(id).and_then(|d| d["running"]["pid"].as_u64()).map(|p| p as u32).filter(|p| util::pid_alive(*p)) {
        return Some(pid);
    }
    let os = crate::Platform::current().os;
    let found = find_quiet(id, os)?;
    process_of(&found.path, &found.executable)
}

/// A process running from `path` (the bundle, the app's folder or the AppImage) or `exe`.
fn process_of(path: &Path, exe: &Path) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        for e in std::fs::read_dir("/proc").ok()?.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
            if let Ok(target) = std::fs::read_link(e.path().join("exe"))
                && (target == exe || target.starts_with(path))
            {
                return Some(pid);
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("ps").args(["-axo", "pid=,comm="]).output().ok()?;
        String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
            let (pid, comm) = l.trim().split_once(char::is_whitespace)?;
            let comm = Path::new(comm.trim());
            (comm == exe || comm.starts_with(path)).then(|| pid.parse().ok()).flatten()
        })
    }
    #[cfg(windows)]
    {
        let name = exe.file_name()?.to_string_lossy().into_owned();
        let out = std::process::Command::new("tasklist").args(["/FO", "CSV", "/NH", "/FI", &format!("IMAGENAME eq {name}")]).output().ok()?;
        let _ = path;
        String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
            let mut cols = l.split("\",\"");
            let image = cols.next()?.trim_start_matches('"');
            let pid = cols.next()?;
            image.eq_ignore_ascii_case(&name).then(|| pid.parse().ok()).flatten()
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (path, exe);
        None
    }
}

/// Where an installed copy is, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub executable: PathBuf,
    pub version: Option<String>,
    /// Installed by the launcher (it may update and remove it).
    pub managed: bool,
    /// In one of the usual places (the launcher may update and remove it too).
    pub standard: bool,
}

/// The `.app` bundle's version from its Info.plist (XML, or binary through `plutil` on macOS).
fn bundle_version(bundle: &Path) -> Option<String> {
    let plist = bundle.join("Contents/Info.plist");
    let Some(text) = std::fs::read(&plist).ok().and_then(|b| String::from_utf8(b).ok()).filter(|t| t.contains("<plist")) else {
        #[cfg(target_os = "macos")]
        if let Ok(o) = std::process::Command::new("plutil").args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"]).arg(&plist).output()
            && o.status.success()
        {
            return Some(String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|v| !v.is_empty());
        }
        return None;
    };
    let at = text.find("<key>CFBundleShortVersionString</key>")?;
    let rest = &text[at..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")?;
    Some(rest[start..start + end].trim().to_string())
}

/// The usual places an app is installed on this computer, best first.
pub fn standard_places(id: &str, os: Os) -> Vec<(PathBuf, PathBuf)> {
    let exe = catalog::exe_name(id, os);
    let mut out = vec![];
    match os {
        Os::Macos => {
            let mut dirs = vec![paths::apps_dir()];
            // `LSUITE_APPS_DIR` set (tests, a separate setup): only that folder counts.
            if !paths::apps_dir_overridden() {
                dirs.push(PathBuf::from("/Applications"));
                if let Some(h) = dirs::home_dir() {
                    dirs.push(h.join("Applications"));
                }
            }
            for d in dirs {
                let b = d.join(format!("{id}.app"));
                let e = b.join("Contents/MacOS").join(&exe);
                if !out.iter().any(|(p, _)| *p == b) {
                    out.push((b, e));
                }
            }
        }
        Os::Linux => {
            let d = paths::apps_dir().join(id);
            out.push((d.clone(), d.join(format!("{id}.AppImage"))));
            out.push((d.clone(), d.join(&exe)));
        }
        Os::Windows => {
            let d = paths::apps_dir().join(id);
            out.push((d.clone(), d.join(&exe)));
            // Installers choose their own folder, whatever `LSUITE_APPS_DIR` says.
            if let Some(local) = dirs::data_local_dir() {
                for sub in [local.join(id), local.join("Programs").join(id)] {
                    out.push((sub.clone(), sub.join(&exe)));
                }
            }
            if let Some(pf) = std::env::var_os("ProgramFiles") {
                let d = PathBuf::from(pf).join(id);
                out.push((d.clone(), d.join(&exe)));
            }
        }
    }
    out
}

fn find_quiet(id: &str, os: Os) -> Option<Found> {
    find(id, os)
}

/// Where the app is installed on this computer, if anywhere.
pub fn find(id: &str, os: Os) -> Option<Found> {
    if let Some(r) = records().remove(id)
        && r.executable.exists()
    {
        return Some(Found { path: r.path, executable: r.executable, version: Some(r.version), managed: true, standard: true });
    }
    let places = standard_places(id, os);
    if let Some(d) = discovery(id) {
        let path = d["appPath"].as_str().map(PathBuf::from);
        let exe = d["executable"].as_str().map(PathBuf::from);
        if let (Some(path), Some(exe)) = (path, exe)
            && exe.exists()
        {
            let version = d["version"].as_str().map(str::to_string);
            // In a usual place: that place is the copy (the bundle, or the app's folder).
            if let Some((place, _)) = places.iter().find(|(p, _)| exe.starts_with(p)) {
                return Some(Found { version, path: place.clone(), executable: exe, managed: false, standard: true });
            }
            return Some(Found { version, path, executable: exe, managed: false, standard: false });
        }
    }
    places.into_iter().find(|(_, e)| e.is_file()).map(|(path, executable)| {
        let version = if os == Os::Macos { bundle_version(&path) } else { None };
        Found { path, executable, version, managed: false, standard: true }
    })
}

// ---- installing -------------------------------------------------------------------------

/// Downloads, verifies and installs `release`. Returns the record written.
pub async fn install(app: &App, r: &Release, p: Platform, progress: &mut Progress) -> CmdResult<Record> {
    if running_pid(app.id).is_some() {
        return Err(format!("{} is open. Quit it, then try again.", app.name));
    }
    let downloads = paths::downloads_dir();
    let file = downloads.join(format!("{}-{}-{}", app.id, r.version, r.file));
    progress.stage(format!("Downloading {} {}", app.name, r.version), 0, None);
    release::download(r, &file, progress).await?;
    progress.stage(format!("Installing {} {}", app.name, r.version), 0, None);
    let (a, rel, f) = (*app, r.clone(), file.clone());
    let result = tokio::task::spawn_blocking(move || put_in_place(&a, &rel, &f, p)).await.map_err(|e| e.to_string()).and_then(|r| r);
    let _ = std::fs::remove_file(&file);
    let (path, executable) = result?;
    let record = Record { app: app.id.into(), version: r.version.clone(), path, executable, kind: r.kind, file: r.file.clone(), installed_at: chrono::Utc::now() };
    let mut all = records();
    all.insert(app.id.into(), record.clone());
    save_records(all)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Err(e) = write_desktop_entry(app, &record.executable) {
        tracing::warn!("couldn't add {} to the app menu: {e}", app.name);
    }
    Ok(record)
}

/// Where a new copy goes: over the managed or usual copy if there is one, else the apps folder.
fn target_for(app: &App, kind: FileKind, os: Os) -> PathBuf {
    let existing = find(app.id, os).filter(|f| f.managed || f.standard).map(|f| f.path);
    match kind {
        FileKind::MacBundleTarGz | FileKind::MacZip => existing.filter(|p| p.extension().is_some_and(|x| x == "app")).unwrap_or_else(|| paths::apps_dir().join(format!("{}.app", app.id))),
        _ => paths::apps_dir().join(app.id),
    }
}

fn put_in_place(app: &App, r: &Release, file: &Path, p: Platform) -> CmdResult<(PathBuf, PathBuf)> {
    let exe = catalog::exe_name(app.id, p.os);
    match r.kind {
        FileKind::MacBundleTarGz | FileKind::MacZip => {
            let bundle = target_for(app, r.kind, p.os);
            let parent = bundle.parent().ok_or("The Applications folder can't be found.")?;
            std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {} ({e}).", parent.display()))?;
            let staging = staging_dir(parent, app.id)?;
            let result = (|| {
                if r.kind == FileKind::MacZip { unzip_ditto(file, &staging)? } else { untar(file, &staging)? }
                let apps: Vec<PathBuf> = read_dir(&staging).into_iter().filter(|p| p.is_dir() && p.extension().is_some_and(|x| x == "app")).collect();
                let [new] = apps.as_slice() else { return Err(format!("{} doesn't hold one app bundle.", r.file)) };
                if !new.join("Contents/MacOS").join(&exe).is_file() {
                    return Err(format!("{} doesn't hold a complete {}.app.", r.file, app.id));
                }
                swap_in(new, &bundle)
            })();
            let _ = std::fs::remove_dir_all(&staging);
            result?;
            Ok((bundle.clone(), bundle.join("Contents/MacOS").join(&exe)))
        }
        FileKind::AppImage => {
            let dir = target_for(app, r.kind, p.os);
            std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {} ({e}).", dir.display()))?;
            let dest = dir.join(format!("{}.AppImage", app.id));
            let staged = dir.join(format!(".{}.AppImage.{}.new", app.id, std::process::id()));
            let result = (|| {
                std::fs::copy(file, &staged).map_err(|e| format!("Couldn't write {} ({e}).", staged.display()))?;
                make_executable(&staged)?;
                swap_in(&staged, &dest)
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&staged);
            }
            result?;
            Ok((dir, dest))
        }
        FileKind::TarGz | FileKind::Zip => {
            let dir = target_for(app, r.kind, p.os);
            let parent = dir.parent().ok_or("The apps folder can't be found.")?;
            std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't create {} ({e}).", parent.display()))?;
            let staging = staging_dir(parent, app.id)?;
            let result = (|| {
                if r.kind == FileKind::Zip { unzip(file, &staging)? } else { untar(file, &staging)? }
                // A single top folder is the app's folder.
                let entries = read_dir(&staging);
                let root = match entries.as_slice() {
                    [only] if only.is_dir() && !only.join(&exe).exists() => only.clone(),
                    _ => staging.clone(),
                };
                if !root.join(&exe).is_file() {
                    return Err(format!("{} doesn't hold {exe}.", r.file));
                }
                make_executable(&root.join(&exe))?;
                swap_in(&root, &dir)
            })();
            let _ = std::fs::remove_dir_all(&staging);
            result?;
            Ok((dir.clone(), dir.join(&exe)))
        }
        FileKind::WindowsInstaller => {
            // The installer (NSIS) installs for this user and chooses its folder; `/S` is silent.
            let status = std::process::Command::new(file).arg("/S").status().map_err(|e| format!("Couldn't run {}'s installer ({e}).", app.name))?;
            if !status.success() {
                return Err(format!("{}'s installer stopped ({status}).", app.name));
            }
            standard_places(app.id, p.os)
                .into_iter()
                .find(|(_, e)| e.is_file())
                .ok_or_else(|| format!("{}'s installer finished, but the launcher can't find where it put {}.", app.name, app.id))
        }
    }
}

fn read_dir(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).map(|it| it.flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("__MACOSX"))).collect()).unwrap_or_default()
}

fn staging_dir(parent: &Path, id: &str) -> CmdResult<PathBuf> {
    let d = parent.join(format!(".lsuite-install-{id}-{}", util::random_token().get(..10).unwrap_or("x")));
    std::fs::create_dir_all(&d).map_err(|e| format!("Couldn't unpack in {} ({e}).", parent.display()))?;
    Ok(d)
}

fn make_executable(p: &Path) -> CmdResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let _ = p;
    Ok(())
}

/// Copies a verified AppImage beside `dest` and swaps it in (the old file is kept aside until the
/// new one starts: a running AppImage stays mounted).
pub fn swap_appimage(new: &Path, dest: &Path) -> CmdResult<()> {
    let staged = dest.with_file_name(format!(".lsuite-update-{}.AppImage", util::random_token().get(..10).unwrap_or("x")));
    let result = (|| {
        std::fs::copy(new, &staged).map_err(|e| format!("Couldn't write {} ({e}).", staged.display()))?;
        make_executable(&staged)?;
        crate::selfupdate::keep_previous_swap(&staged, dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result
}

/// Unpacks a `.tar.gz`, keeping permissions and refusing entries that leave `dest`.
pub fn untar(file: &Path, dest: &Path) -> CmdResult<()> {
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    tar.set_preserve_permissions(true);
    tar.set_overwrite(false);
    // `unpack` refuses `..` and absolute paths itself.
    tar.unpack(dest).map_err(|e| format!("The archive couldn't be unpacked ({e})."))
}

/// Unpacks a zip (Windows portable copies), refusing entries that leave `dest`.
pub fn unzip(file: &Path, dest: &Path) -> CmdResult<()> {
    let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("The archive couldn't be read ({e})."))?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { return Err(format!("The archive has an unsafe path ({}).", entry.name())) };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(d) = out.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let mut w = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut w).map_err(|e| format!("The archive couldn't be unpacked ({e})."))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

/// macOS zips of app bundles hold symlinks and extended attributes: `ditto` keeps them.
fn unzip_ditto(file: &Path, dest: &Path) -> CmdResult<()> {
    if !cfg!(target_os = "macos") {
        return unzip(file, dest);
    }
    let status = std::process::Command::new("ditto").args(["-x", "-k"]).arg(file).arg(dest).status().map_err(|e| format!("Couldn't run ditto ({e})."))?;
    if status.success() { Ok(()) } else { Err(format!("The archive couldn't be unpacked (ditto: {status}).")) }
}

/// Puts `new` at `dest`: the copy already there is moved aside first and put back if the move
/// fails, then removed.
pub fn swap_in(new: &Path, dest: &Path) -> CmdResult<()> {
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let previous = dest.with_file_name(format!(".{name}.previous"));
    remove_any(&previous);
    let had = dest.exists() || dest.is_symlink();
    if had {
        std::fs::rename(dest, &previous).map_err(|e| format!("Couldn't move the installed copy aside ({e}). Is it open?"))?;
    }
    if let Err(e) = std::fs::rename(new, dest) {
        if had {
            let _ = std::fs::rename(&previous, dest);
        }
        return Err(format!("Couldn't put the new copy in place at {} ({e}).", dest.display()));
    }
    remove_any(&previous);
    Ok(())
}

fn remove_any(p: &Path) {
    if p.is_dir() && !p.is_symlink() {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn desktop_ids(app: &App) -> (PathBuf, PathBuf) {
    (paths::desktop_entries_dir().join(format!("xyz.lsuite.{}.desktop", app.id)), paths::icons_dir().join(format!("xyz.lsuite.{}.png", app.id)))
}

/// The app in the system's app menu (freedesktop), with its icon.
#[cfg(all(unix, not(target_os = "macos")))]
fn write_desktop_entry(app: &App, exe: &Path) -> std::io::Result<()> {
    let (entry, icon) = desktop_ids(app);
    if let Some(bytes) = crate::app_icon(app.id) {
        if let Some(d) = icon.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&icon, bytes)?;
    }
    let categories = match app.kind {
        "music" => "AudioVideo;Audio;",
        "video" => "AudioVideo;Video;",
        "image" => "Graphics;",
        _ => "Office;",
    };
    let exec = exe.display().to_string().replace('"', "\\\"");
    let text = format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment={}\nExec=\"{exec}\" %F\nIcon={}\nTerminal=false\nCategories={categories}\nStartupWMClass={}\nX-lsuite-launcher=true\n",
        app.name,
        app.summary,
        icon.display(),
        app.id
    );
    if let Some(d) = entry.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(entry, text)
}

// ---- removing ---------------------------------------------------------------------------

/// Removes the app (never its documents or settings). Returns what was removed.
pub fn uninstall(app: &App, os: Os) -> CmdResult<Value> {
    let Some(found) = find(app.id, os) else { return Err(format!("{} isn't installed.", app.name)) };
    if running_pid(app.id).is_some() {
        return Err(format!("{} is open. Quit it, then remove it.", app.name));
    }
    if !found.managed && !found.standard {
        return Err(format!("{} at {} wasn't installed by the launcher (a build from source?). Remove it yourself if you want to.", app.name, found.path.display()));
    }
    let record = records().remove(app.id);
    let installer_made = record.as_ref().is_some_and(|r| r.kind == FileKind::WindowsInstaller) || (os == Os::Windows && found.path.join("uninstall.exe").is_file());
    if installer_made {
        let un = found.path.join("uninstall.exe");
        if !un.is_file() {
            return Err(format!("{}'s uninstaller isn't in {}. Remove it from Settings › Apps.", app.name, found.path.display()));
        }
        let status = std::process::Command::new(&un).arg("/S").status().map_err(|e| format!("Couldn't run {}'s uninstaller ({e}).", app.name))?;
        if !status.success() {
            return Err(format!("{}'s uninstaller stopped ({status}).", app.name));
        }
    } else {
        // Only the app's own bundle or folder: refuse anything that looks like a shared folder.
        let ok = found.path.file_name().is_some_and(|n| {
            let n = n.to_string_lossy().to_ascii_lowercase();
            n == app.id || n == format!("{}.app", app.id)
        });
        if !ok {
            return Err(format!("The launcher won't remove {}: it isn't {}'s own folder.", found.path.display(), app.name));
        }
        if found.path.is_dir() {
            std::fs::remove_dir_all(&found.path).map_err(|e| format!("Couldn't remove {} ({e}).", found.path.display()))?;
        } else {
            std::fs::remove_file(&found.path).map_err(|e| format!("Couldn't remove {} ({e}).", found.path.display()))?;
        }
    }
    let mut all = records();
    all.remove(app.id);
    save_records(all)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let (entry, icon) = desktop_ids(app);
        let _ = std::fs::remove_file(entry);
        let _ = std::fs::remove_file(icon);
    }
    // The discovery file would still name the removed copy.
    if discovery(app.id).and_then(|d| d["appPath"].as_str().map(PathBuf::from)).is_some_and(|p| p.starts_with(&found.path) || found.path.starts_with(&p)) {
        let _ = std::fs::remove_file(paths::discovery_dir().join(format!("{}.json", app.id)));
    }
    Ok(json!({ "app": app.id, "removed": found.path, "kept": "Documents, settings and data folders were left as they were." }))
}

// ---- opening ----------------------------------------------------------------------------

/// Starts the app (with files to open, if any), detached from the launcher.
pub fn open(app: &App, os: Os, files: &[PathBuf]) -> CmdResult<Value> {
    let Some(found) = find(app.id, os) else { return Err(format!("{} isn't installed. Install it first.", app.name)) };
    if let Some(pid) = running_pid(app.id)
        && files.is_empty()
    {
        // Already open: bring it forward where the system lets us.
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg("-a").arg(&found.path).status();
        return Ok(json!({ "app": app.id, "running": true, "pid": pid }));
    }
    let mut cmd = if os == Os::Macos && found.path.extension().is_some_and(|x| x == "app") {
        let mut c = std::process::Command::new("open");
        c.arg("-a").arg(&found.path);
        if !files.is_empty() {
            c.args(files);
        }
        c
    } else {
        let mut c = std::process::Command::new(&found.executable);
        c.args(files);
        if let Some(d) = found.executable.parent() {
            c.current_dir(d);
        }
        c
    };
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own session: closing the launcher doesn't close the app.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS);
    }
    let child = cmd.spawn().map_err(|e| format!("Couldn't start {} ({e}).", app.name))?;
    Ok(json!({ "app": app.id, "started": true, "pid": child.id() }))
}

/// Shows the installed copy in the file manager.
pub fn reveal(app: &App, os: Os) -> CmdResult<Value> {
    let Some(found) = find(app.id, os) else { return Err(format!("{} isn't installed.", app.name)) };
    let r = match os {
        Os::Macos => std::process::Command::new("open").arg("-R").arg(&found.path).spawn().map(|_| ()),
        Os::Windows => std::process::Command::new("explorer").arg(format!("/select,{}", found.executable.display())).spawn().map(|_| ()),
        Os::Linux => util::open_external(&found.path.display().to_string()),
    };
    r.map_err(|e| format!("Couldn't open the folder ({e})."))?;
    Ok(json!({ "app": app.id, "path": found.path }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_keeps_the_old_copy_when_the_new_one_cannot_go_in() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("folio");
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(dest.join("folio"), "old").unwrap();
        let missing = d.path().join("nothing-here");
        assert!(swap_in(&missing, &dest).is_err());
        assert_eq!(std::fs::read_to_string(dest.join("folio")).unwrap(), "old");
        let new = d.path().join("new");
        std::fs::create_dir(&new).unwrap();
        std::fs::write(new.join("folio"), "new").unwrap();
        swap_in(&new, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("folio")).unwrap(), "new");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1, "nothing left aside");
    }

    #[test]
    fn bundle_versions_come_from_info_plist() {
        let d = tempfile::tempdir().unwrap();
        let b = d.path().join("folio.app");
        std::fs::create_dir_all(b.join("Contents")).unwrap();
        std::fs::write(b.join("Contents/Info.plist"), "<plist><dict><key>CFBundleName</key><string>folio</string><key>CFBundleShortVersionString</key>\n  <string>0.1.0</string></dict></plist>").unwrap();
        assert_eq!(bundle_version(&b).as_deref(), Some("0.1.0"));
    }

    #[test]
    fn zips_cannot_write_outside_their_folder() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("evil.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
            z.start_file("../escaped.txt", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, b"x").unwrap();
            z.finish().unwrap();
        }
        let out = d.path().join("out");
        std::fs::create_dir(&out).unwrap();
        assert!(unzip(&file, &out).unwrap_err().contains("unsafe"));
        assert!(!d.path().join("escaped.txt").exists());
    }
}
