//! Where the launcher keeps things.
//!
//! - `~/.lsuite/` (`LSUITE_HOME`): shared with every lsuite app: `apps/<app>.json` (each app's
//!   discovery file, STANDARD.md section 4) and `plugins/<app>/<id>/` (installed plugins).
//! - `~/.lsuite/launcher/`: the launcher's own `settings.json`, `installed.json` (what it
//!   installed and where) and `downloads/` (partial downloads, removed when done).
//! - Where apps go ([`apps_dir`], `LSUITE_APPS_DIR` replaces it): `/Applications` when it can be
//!   written, else `~/Applications` on macOS; `~/.local/share/lsuite/apps/<app>/` on Linux;
//!   `%LOCALAPPDATA%\lsuite\apps\<app>\` on Windows for portable copies (installers choose
//!   their own folder).

use std::path::PathBuf;

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// `$LSUITE_HOME`, else `~/.lsuite`.
pub fn lsuite_home() -> PathBuf {
    env_dir("LSUITE_HOME").unwrap_or_else(|| dirs::home_dir().unwrap_or_else(std::env::temp_dir).join(".lsuite"))
}

/// The apps' discovery files.
pub fn discovery_dir() -> PathBuf {
    lsuite_home().join("apps")
}

pub fn launcher_dir() -> PathBuf {
    lsuite_home().join("launcher")
}

pub fn installed_file() -> PathBuf {
    launcher_dir().join("installed.json")
}

pub fn settings_file() -> PathBuf {
    launcher_dir().join("settings.json")
}

pub fn downloads_dir() -> PathBuf {
    launcher_dir().join("downloads")
}

/// Whether `LSUITE_APPS_DIR` chose where apps go (then only that folder counts as a usual place).
pub fn apps_dir_overridden() -> bool {
    env_dir("LSUITE_APPS_DIR").is_some()
}

/// Where new apps are installed (see the module's notes).
pub fn apps_dir() -> PathBuf {
    if let Some(d) = env_dir("LSUITE_APPS_DIR") {
        return d;
    }
    #[cfg(target_os = "macos")]
    {
        let system = PathBuf::from("/Applications");
        if writable(&system) { system } else { dirs::home_dir().unwrap_or_else(std::env::temp_dir).join("Applications") }
    }
    #[cfg(not(target_os = "macos"))]
    {
        dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("lsuite").join("apps")
    }
}

/// Linux desktop entries (`~/.local/share/applications`) and icons, so installed apps show in
/// the system's app menu. `LSUITE_APPS_DIR` set (tests, a portable setup) keeps them beside it.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn desktop_entries_dir() -> PathBuf {
    if let Some(d) = env_dir("LSUITE_APPS_DIR") {
        return d.join("applications");
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("applications")
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn icons_dir() -> PathBuf {
    if let Some(d) = env_dir("LSUITE_APPS_DIR") {
        return d.join("icons");
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("icons/hicolor/256x256/apps")
}

/// Whether this user can create files in `dir`.
pub fn writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".lsuite-write-test-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
