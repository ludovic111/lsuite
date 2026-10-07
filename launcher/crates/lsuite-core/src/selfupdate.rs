//! The launcher's own updates, from the `ludovic111/lsuite` releases (tags `launcher-vX.Y.Z`).
//!
//! Every release carries a signed `latest.json` in the Tauri updater's format (written by
//! `lsuite-release manifest`); each file is signed with the launcher's minisign key
//! ([`PUBLIC_KEY`]) and the signature's trusted comment names the version, so an old signed file
//! can't pass for a new release. What gets replaced:
//! - **macOS**: the running `lsuite.app` is moved aside to `.lsuite.app.previous` and the new
//!   bundle moved in; the previous copy is removed once the new one starts ([`finish_pending`]).
//! - **Linux AppImage** (`$APPIMAGE`): the same, file for file.
//! - **Windows**, installed with the installer (`uninstall.exe` beside `lsuite.exe`): the
//!   verified installer runs once the launcher has exited ([`restart`], [`apply_on_quit`]).
//! - Portable copies (Windows zip, Linux tar.gz) and builds from source: the update is announced
//!   with the file to get by hand.
//!
//! `LSUITE_UPDATE_URL` points the check at another `latest.json` (signatures are still checked;
//! debug builds accept `LSUITE_UPDATE_PUBKEY` instead of the built-in key, for tests).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::release::{Check, FileKind, Release};
use crate::{CmdResult, Launcher, util};

/// The launcher's update key (base64 of a minisign public key). The secret half is the
/// `LSUITE_UPDATE_SIGNING_KEY` secret of the release workflow.
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEU1MDYxQzhDQTcxNERDMUEKUldRYTNCU25qQndHNVprS2tvRHJjZ0VGakMveEFzV09NbGFsanBqTjEvNXkrVDRyWlFncnVvcG4K";
pub const MANIFEST_URL: &str = "https://github.com/ludovic111/lsuite/releases/latest/download/latest.json";
pub const RELEASES_URL: &str = "https://github.com/ludovic111/lsuite/releases/latest";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// What the launcher knows about its own updates (`app.checkUpdates`).
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub current: String,
    pub available: Option<String>,
    pub notes: Option<String>,
    /// Installed: restart the launcher to use it.
    pub ready: bool,
    pub busy: bool,
    pub can_install: bool,
    /// Why this copy can't replace itself, when it can't.
    pub install_blocked: Option<String>,
    /// The file to get by hand (or the release page).
    pub download_url: Option<String>,
    pub error: Option<String>,
    pub checked_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Default)]
pub(crate) struct State {
    status: Status,
    found: Option<Release>,
    /// A verified Windows installer waiting for the launcher to exit.
    pending_installer: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    platforms: BTreeMap<String, Asset>,
}

#[derive(Debug, Clone, Deserialize)]
struct Asset {
    signature: String,
    url: String,
}

/// How this copy of the launcher is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    MacBundle(PathBuf),
    AppImage(PathBuf),
    WindowsInstalled(PathBuf),
    WindowsPortable(PathBuf),
    Other,
}

pub fn current_install() -> Install {
    let Ok(exe) = std::env::current_exe().and_then(|p| p.canonicalize()) else { return Install::Other };
    install_of(std::env::consts::OS, &exe, std::env::var_os("APPIMAGE").map(PathBuf::from))
}

/// How a copy is installed, from its OS, its (canonical) executable and `$APPIMAGE`.
pub fn install_of(os: &str, exe: &Path, appimage: Option<PathBuf>) -> Install {
    match os {
        "macos" => bundle_of(exe).map_or(Install::Other, Install::MacBundle),
        "linux" => appimage.filter(|p| p.is_file()).map_or(Install::Other, Install::AppImage),
        "windows" => match exe.parent() {
            Some(dir) if dir.join("uninstall.exe").is_file() => Install::WindowsInstalled(dir.to_path_buf()),
            Some(dir) if dir.join("lsuite-cli.exe").is_file() => Install::WindowsPortable(dir.to_path_buf()),
            _ => Install::Other,
        },
        _ => Install::Other,
    }
}

/// `…/lsuite.app` for an executable at `…/lsuite.app/Contents/MacOS/<exe>`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && app.extension()? == "app").then(|| app.to_path_buf())
}

/// The `latest.json` keys for an install, most specific first.
pub fn keys_for(install: &Install, os: &str, arch: &str) -> Vec<String> {
    let os = if os == "macos" { "darwin" } else { os };
    match install {
        Install::MacBundle(_) => vec![format!("{os}-{arch}-app"), format!("{os}-{arch}")],
        Install::AppImage(_) => vec![format!("{os}-{arch}-appimage"), format!("{os}-{arch}")],
        Install::WindowsInstalled(_) => vec![format!("{os}-{arch}-nsis"), format!("{os}-{arch}")],
        Install::WindowsPortable(_) => vec![format!("{os}-{arch}-portable")],
        Install::Other => vec![format!("{os}-{arch}")],
    }
}

/// Ok when this copy can replace itself, else why not.
fn install_support(install: &Install) -> Result<(), String> {
    let target = match install {
        Install::MacBundle(b) if b.to_string_lossy().contains("/AppTranslocation/") => {
            return Err("lsuite is running from a temporary location (macOS App Translocation). Move lsuite.app to Applications and open it from there.".into());
        }
        Install::MacBundle(b) => b.clone(),
        Install::AppImage(p) => p.clone(),
        Install::WindowsInstalled(dir) => dir.join("lsuite.exe"),
        Install::WindowsPortable(_) => return Err("This portable copy is updated by hand: unpack the new zip over it.".into()),
        Install::Other => return Err("This copy of lsuite (a build from source or the tar.gz) is updated by hand.".into()),
    };
    let dir = target.parent().ok_or("The launcher's folder can't be found.")?;
    if crate::paths::writable(dir) { Ok(()) } else { Err(format!("lsuite can't write to {}, so it can't replace itself.", dir.display())) }
}

fn manifest_url() -> String {
    std::env::var("LSUITE_UPDATE_URL").ok().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| MANIFEST_URL.into())
}

fn public_key() -> String {
    #[cfg(debug_assertions)]
    if let Some(k) = std::env::var("LSUITE_UPDATE_PUBKEY").ok().filter(|k| !k.trim().is_empty()) {
        return k;
    }
    PUBLIC_KEY.to_string()
}

pub fn status(l: &Launcher) -> Status {
    let mut s = l.update.lock().status.clone();
    s.current = CURRENT.into();
    s
}

fn emit(l: &Launcher) {
    l.hub.changed("update");
}

/// Looks for a newer launcher.
pub async fn check(l: &Arc<Launcher>) -> CmdResult<Status> {
    if l.update.lock().status.ready {
        return Ok(status(l));
    }
    let fetched: CmdResult<Manifest> = async {
        let r = util::client().get(manifest_url()).send().await.map_err(|e| format!("Couldn't reach GitHub to check for updates ({e})."))?;
        if r.status().as_u16() == 404 {
            return Err("No launcher release has been published yet.".to_string());
        }
        if !r.status().is_success() {
            return Err(format!("GitHub answered {} to the update check.", r.status().as_u16()));
        }
        r.json::<Manifest>().await.map_err(|e| format!("The update manifest isn't valid ({e})."))
    }
    .await;
    let install = current_install();
    let mut u = l.update.lock();
    u.status.checked_at = Some(chrono::Utc::now());
    match fetched {
        Ok(m) => {
            let newer = crate::release::is_newer(&m.version, CURRENT) && semver::Version::parse(m.version.trim_start_matches('v')).is_ok();
            let asset = keys_for(&install, std::env::consts::OS, std::env::consts::ARCH).iter().find_map(|k| m.platforms.get(k)).cloned();
            let blocked = install_support(&install).err();
            u.found = match (&asset, newer) {
                (Some(a), true) => {
                    let file = a.url.rsplit('/').next().unwrap_or_default().to_string();
                    FileKind::of(&file, crate::Platform::current().os).map(|kind| Release {
                        app: "lsuite".into(),
                        version: m.version.trim_start_matches('v').to_string(),
                        file,
                        url: a.url.clone(),
                        kind,
                        notes: m.notes.clone(),
                        check: Check::Minisign { signature: a.signature.clone(), public_key: public_key() },
                    })
                }
                _ => None,
            };
            let prefix = format!("{}/ludovic111/lsuite/releases/download/", crate::release::github());
            if u.found.as_ref().is_some_and(|f| !f.url.starts_with(&prefix)) && std::env::var("LSUITE_UPDATE_URL").is_err() {
                u.found = None;
                u.status.error = Some("The update manifest points outside the launcher's releases; it was ignored.".into());
            } else {
                u.status.error = None;
            }
            u.status.available = newer.then(|| m.version.trim_start_matches('v').to_string());
            u.status.notes = if newer { m.notes } else { None };
            u.status.can_install = u.found.is_some() && blocked.is_none();
            u.status.install_blocked = if newer { blocked } else { None };
            u.status.download_url = newer.then(|| asset.map_or(RELEASES_URL.to_string(), |a| a.url));
        }
        Err(e) => u.status.error = Some(e),
    }
    drop(u);
    emit(l);
    Ok(status(l))
}

/// Downloads, verifies and installs the update [`check`] found. Restart to use it.
pub async fn install(l: &Arc<Launcher>) -> CmdResult<Status> {
    if l.update.lock().status.ready {
        return Ok(status(l));
    }
    if l.update.lock().found.is_none() {
        check(l).await?;
    }
    let install = current_install();
    let found = {
        let mut u = l.update.lock();
        if u.status.busy {
            return Err("The update is already being installed.".into());
        }
        let Some(found) = u.found.clone() else {
            return Err(match &u.status.available {
                Some(v) => format!("lsuite {v} has no download for this computer yet: {RELEASES_URL}"),
                None => format!("lsuite {CURRENT} is up to date."),
            });
        };
        if let Err(why) = install_support(&install) {
            return Err(format!("{why} Get lsuite {} from {}", found.version, u.status.download_url.clone().unwrap_or_else(|| RELEASES_URL.into())));
        }
        u.status.busy = true;
        found
    };
    emit(l);
    let mut progress = l.hub.progress("update:lsuite", format!("Downloading lsuite {}", found.version));
    let dir = crate::paths::downloads_dir().join(format!("lsuite-{}", found.version));
    let file = dir.join(&found.file);
    let result: CmdResult<()> = async {
        crate::release::download(&found, &file, &mut progress).await?;
        progress.stage(format!("Installing lsuite {}", found.version), 0, None);
        let (file, install) = (file.clone(), install.clone());
        tokio::task::spawn_blocking(move || match install {
            Install::MacBundle(bundle) => install_bundle(&file, &bundle),
            Install::AppImage(path) => crate::install::swap_appimage(&file, &path),
            // The installer can't replace a running lsuite.exe: it runs once the launcher exits.
            Install::WindowsInstalled(_) => Ok(()),
            _ => Err("This copy of lsuite can't replace itself.".into()),
        })
        .await
        .map_err(|e| e.to_string())?
    }
    .await;
    let windows = matches!(install, Install::WindowsInstalled(_));
    {
        let mut u = l.update.lock();
        u.status.busy = false;
        match &result {
            Ok(()) => {
                u.status.ready = true;
                u.status.can_install = false;
                if windows {
                    u.pending_installer = Some(file.clone());
                }
            }
            Err(e) => u.status.error = Some(e.clone()),
        }
    }
    if !windows || result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    progress.finish(&result, format!("lsuite {} is installed. Restart the launcher to use it.", found.version));
    emit(l);
    result.map(|_| status(l))
}

/// Unpacks a verified `.app.tar.gz` beside `bundle` and swaps it in (the old one is kept as
/// `.lsuite.app.previous` until the new one starts).
fn install_bundle(archive: &Path, bundle: &Path) -> CmdResult<()> {
    let parent = bundle.parent().ok_or("The launcher's folder can't be found.")?;
    let staging = parent.join(format!(".lsuite-update-{}", util::random_token().get(..10).unwrap_or("x")));
    std::fs::create_dir(&staging).map_err(|e| format!("Couldn't unpack the update in {} ({e}).", parent.display()))?;
    let result = (|| {
        crate::install::untar(archive, &staging)?;
        let app = staging.join("lsuite.app");
        if !app.join("Contents/Info.plist").is_file() || !app.join("Contents/MacOS/lsuite").is_file() {
            return Err("The update archive doesn't hold a complete lsuite.app.".to_string());
        }
        keep_previous_swap(&app, bundle)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result?;
    let _ = std::process::Command::new("touch").arg(bundle).status();
    Ok(())
}

/// Where the replaced copy waits until the new one has started.
pub fn previous_path(target: &Path) -> Option<PathBuf> {
    let name = target.file_name()?.to_string_lossy();
    Some(target.with_file_name(format!(".{name}.previous")))
}

/// Swaps `new` in for `target`, keeping `target` as [`previous_path`] (the running copy can't be
/// removed while it runs everywhere).
pub(crate) fn keep_previous_swap(new: &Path, target: &Path) -> CmdResult<()> {
    let previous = previous_path(target).ok_or("The launcher's location can't be found.")?;
    if previous.exists() {
        let _ = if previous.is_dir() { std::fs::remove_dir_all(&previous) } else { std::fs::remove_file(&previous) };
    }
    std::fs::rename(target, &previous).map_err(|e| format!("Couldn't move {} aside ({e}).", target.display()))?;
    if let Err(e) = std::fs::rename(new, target) {
        let _ = std::fs::rename(&previous, target);
        return Err(format!("Couldn't put the new version in place ({e}); the launcher is unchanged."));
    }
    Ok(())
}

/// Removes what the last update left beside the running copy. Call once at start.
pub fn finish_pending() {
    if let Install::MacBundle(t) | Install::AppImage(t) = current_install() {
        if let Some(p) = previous_path(&t).filter(|p| p.exists()) {
            let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        }
        if let Some(dir) = t.parent()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with(".lsuite-update-") {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
}

/// Starts the installed copy once this process has exited (on Windows, through the downloaded
/// installer). Call it, then quit.
pub fn restart(l: &Launcher) -> std::io::Result<()> {
    let install = current_install();
    let pending = l.update.lock().pending_installer.take();
    #[cfg(unix)]
    {
        let _ = pending;
        use std::os::unix::process::CommandExt;
        let (target, launch) = match &install {
            Install::MacBundle(b) => (b.clone(), r#"exec /usr/bin/open -n "$0""#),
            Install::AppImage(p) => (p.clone(), r#"exec "$0""#),
            _ => (std::env::current_exe()?, r#"exec "$0""#),
        };
        let script = format!(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; {launch}"#);
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .arg(&target)
            .arg(std::process::id().to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .map(|_| ())
    }
    #[cfg(windows)]
    {
        let _ = install;
        match pending {
            Some(setup) => after_exit(&setup, &["/P", "/R"]),
            None => after_exit(&std::env::current_exe()?, &[]),
        }
    }
}

/// On quit with a downloaded Windows installer waiting: runs it (passive).
pub fn apply_on_quit(l: &Launcher) {
    let pending = l.update.lock().pending_installer.take();
    #[cfg(windows)]
    if let Some(setup) = pending {
        let _ = after_exit(&setup, &["/P"]);
    }
    #[cfg(not(windows))]
    let _ = pending;
}

/// Starts `program` with `args` once this process has exited, from a hidden PowerShell.
#[cfg(windows)]
fn after_exit(program: &Path, args: &[&str]) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let args = if args.is_empty() { String::new() } else { format!(" -ArgumentList {}", args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(",")) };
    let script = format!("try {{ Wait-Process -Id {} -Timeout 120 -ErrorAction SilentlyContinue }} catch {{}}; Start-Process -FilePath {}{args}", std::process::id(), quote(&program.to_string_lossy()));
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

/// `app.checkUpdates` / `app.installUpdate` answers.
pub fn to_value(s: &Status) -> Value {
    let mut v = serde_json::to_value(s).unwrap_or_default();
    v["message"] = json!(match (&s.available, s.ready) {
        (_, true) => "The update is installed: restart the launcher to use it.".to_string(),
        (Some(v), false) => format!("lsuite {v} is available."),
        (None, false) => format!("lsuite {} is up to date.", s.current),
    });
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_the_install_kinds_apart() {
        assert_eq!(install_of("macos", Path::new("/Applications/lsuite.app/Contents/MacOS/lsuite"), None), Install::MacBundle(PathBuf::from("/Applications/lsuite.app")));
        assert_eq!(install_of("linux", Path::new("/home/me/src/target/release/lsuite"), None), Install::Other);
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("lsuite.AppImage");
        std::fs::write(&image, b"").unwrap();
        assert_eq!(install_of("linux", Path::new("/tmp/.mount_x/usr/bin/lsuite"), Some(image.clone())), Install::AppImage(image));
        let exe = dir.path().join("lsuite.exe");
        assert_eq!(install_of("windows", &exe, None), Install::Other);
        std::fs::write(dir.path().join("lsuite-cli.exe"), b"").unwrap();
        assert_eq!(install_of("windows", &exe, None), Install::WindowsPortable(dir.path().to_path_buf()));
        std::fs::write(dir.path().join("uninstall.exe"), b"").unwrap();
        assert_eq!(install_of("windows", &exe, None), Install::WindowsInstalled(dir.path().to_path_buf()));
        assert_eq!(keys_for(&Install::MacBundle(PathBuf::new()), "macos", "aarch64"), ["darwin-aarch64-app", "darwin-aarch64"]);
        assert_eq!(keys_for(&Install::WindowsPortable(PathBuf::new()), "windows", "x86_64"), ["windows-x86_64-portable"]);
    }

    #[test]
    fn the_built_in_key_is_a_minisign_key() {
        use base64::Engine;
        let text = base64::engine::general_purpose::STANDARD.decode(PUBLIC_KEY).unwrap();
        minisign_verify::PublicKey::decode(std::str::from_utf8(&text).unwrap()).unwrap();
    }

    #[test]
    fn previous_copies_sit_beside() {
        assert_eq!(previous_path(Path::new("/Applications/lsuite.app")), Some(PathBuf::from("/Applications/.lsuite.app.previous")));
        let d = tempfile::tempdir().unwrap();
        let (target, new) = (d.path().join("lsuite.AppImage"), d.path().join("new"));
        std::fs::write(&target, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        keep_previous_swap(&new, &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(d.path().join(".lsuite.AppImage.previous")).unwrap(), "old");
    }
}
