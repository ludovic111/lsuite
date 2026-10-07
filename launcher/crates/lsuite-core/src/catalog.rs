//! The lsuite apps the launcher can install, and how each one's releases are signed.
//!
//! The list is built in, with each app's release key, so nothing the network says can make the
//! launcher install from another repository or trust another key. The site's `GET /api/apps`
//! may name apps this build doesn't know yet: those are shown with a link to their page only.
//!
//! Two signing schemes are in use:
//! - **Release manifest** (kimchi, nori, folio): `latest.json` in the Tauri updater's format, each
//!   platform's file signed with the app's minisign (Ed25519) key; the signature's trusted
//!   comment names the version.
//! - **Signed checksums** (ryolune, zenith): `SHA256SUMS` and `SHA256SUMS.sig`, a raw Ed25519
//!   signature of the checksums file (`<app>-ed25519 <base64>`); the file's SHA-256 must be listed.

use serde::Serialize;

use crate::platform::{Os, Platform};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signing {
    /// Base64 of a minisign public key file.
    Manifest { public_key: &'static str },
    /// Hex Ed25519 public key; the signature line starts with `prefix`.
    Checksums { public_key_hex: &'static str, prefix: &'static str },
}

#[derive(Clone, Copy, Debug)]
pub struct App {
    pub id: &'static str,
    /// The app's name, always lowercase.
    pub name: &'static str,
    /// `music`, `video`, `code`, `image`, `office` (as in the discovery files).
    pub kind: &'static str,
    /// The line under its name.
    pub summary: &'static str,
    /// GitHub `owner/name`.
    pub repo: &'static str,
    pub signing: Signing,
    /// Checksums apps: the release file for each platform key. Manifest apps pick from
    /// `latest.json` ([`manifest_keys`]).
    pub files: &'static [(&'static str, &'static str)],
    /// What the app is for, in a few words ("Music", "Video"…).
    pub kind_label: &'static str,
}

impl App {
    pub fn page(&self) -> String {
        format!("{}/{}", crate::account::server(), self.id)
    }

    pub fn releases_page(&self) -> String {
        format!("{}/{}/releases/latest", crate::release::github(), self.repo)
    }

    /// Whether the app has a build for this platform at all (before asking the network).
    pub fn supports(&self, p: Platform) -> bool {
        match self.signing {
            Signing::Manifest { .. } => matches!(p.key(), "macos-arm64" | "macos-x86_64" | "linux-x86_64" | "windows-x86_64"),
            Signing::Checksums { .. } => self.files.iter().any(|(k, _)| *k == p.key()),
        }
    }
}

/// The keys to look for in a `latest.json`, best first: macOS takes the app bundle archive,
/// Linux the AppImage, Windows the installer.
pub fn manifest_keys(p: Platform) -> &'static [&'static str] {
    match p.key() {
        "macos-arm64" => &["darwin-aarch64-app", "darwin-aarch64"],
        "macos-x86_64" => &["darwin-x86_64-app", "darwin-x86_64"],
        "linux-x86_64" => &["linux-x86_64-appimage", "linux-x86_64"],
        "windows-x86_64" => &["windows-x86_64-nsis", "windows-x86_64"],
        _ => &[],
    }
}

pub const APPS: &[App] = &[
    App {
        id: "ryolune",
        name: "ryolune",
        kind: "music",
        kind_label: "Music",
        summary: "The DAW your AI can drive.",
        repo: "ludovic111/ryolune",
        signing: Signing::Checksums { public_key_hex: "33725ffc8e298d46c23e598825a4f54e6baad5b3680b986a3bb9b6e9717adc1f", prefix: "ryolune-ed25519" },
        files: &[
            ("macos-arm64", "ryolune-macos-arm64.zip"),
            ("macos-x86_64", "ryolune-macos-x86_64.zip"),
            ("linux-x86_64", "ryolune-linux-x86_64.tar.gz"),
            ("windows-x86_64", "ryolune-windows-x86_64.zip"),
        ],
    },
    App {
        id: "kimchi",
        name: "kimchi",
        kind: "video",
        kind_label: "Video",
        summary: "A video editor where generation is part of the cut.",
        repo: "ludovic111/kimchi",
        signing: Signing::Manifest {
            public_key: "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDRDMDcxNTc4RjA1N0Q5QTcKUldTbjJWZndlQlVIVEZyRWNtU2tqeWxCaFNvTGQxbU1qczFIMGY5cUYzVVdBWkxOL0FvalJocEkK",
        },
        files: &[],
    },
    App {
        id: "zenith",
        name: "zenith",
        kind: "code",
        kind_label: "Code",
        summary: "An app for coding with agents.",
        repo: "ludovic111/zenith",
        signing: Signing::Checksums { public_key_hex: "865bfc7a83722e492a0967e2ced7493fdbc78d9dc3389f0d57ac9cd2423a2c44", prefix: "zenith-ed25519" },
        files: &[("macos-arm64", "zenith-macos-arm64.zip"), ("macos-x86_64", "zenith-macos-x86_64.zip"), ("linux-x86_64", "zenith-linux-x86_64.tar.gz")],
    },
    App {
        id: "nori",
        name: "nori",
        kind: "image",
        kind_label: "Image and design",
        summary: "Pixels, vectors and pages in one document.",
        repo: "ludovic111/nori",
        signing: Signing::Manifest {
            public_key: "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEM5NTY4RDlBRDU2M0MzRjQKUldUMHcyUFZtbzFXeVM0ZkcyaHpzWDBDYTgxNWd4eTRZRVAvdWQxcnl0c1QvRTNRUkVmbmpYbHoK",
        },
        files: &[],
    },
    App {
        id: "folio",
        name: "folio",
        kind: "office",
        kind_label: "Office",
        summary: "Documents, spreadsheets and slides in one app.",
        repo: "ludovic111/folio",
        signing: Signing::Manifest {
            public_key: "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEM0RjRDRkFEREE5RTRBQkEKUldTNlNwN2FyYy8weEF6UzRHSSsvRGo3amRsRVZ4akljdm5iRmxWNVlKRnFEU0xCc2FqZkdTTFgK",
        },
        files: &[],
    },
];

pub fn get(id: &str) -> Option<&'static App> {
    let id = id.trim().to_ascii_lowercase();
    APPS.iter().find(|a| a.id == id)
}

/// `get`, or an error naming the apps there are.
pub fn find(id: &str) -> Result<&'static App, String> {
    get(id).ok_or_else(|| format!("There's no lsuite app called {id:?}. The apps: {}.", APPS.iter().map(|a| a.id).collect::<Vec<_>>().join(", ")))
}

/// What `apps.list` says about an app before its install status is added.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppCard {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub kind_label: String,
    pub summary: String,
    pub repo: String,
    pub page: String,
}

impl From<&App> for AppCard {
    fn from(a: &App) -> Self {
        AppCard { id: a.id.into(), name: a.name.into(), kind: a.kind.into(), kind_label: a.kind_label.into(), summary: a.summary.into(), repo: a.repo.into(), page: a.page() }
    }
}

/// The executable's name inside an app's folder or bundle.
pub fn exe_name(id: &str, os: Os) -> String {
    if os == Os::Windows { format!("{id}.exe") } else { id.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_has_a_key_and_a_build() {
        for a in APPS {
            assert_eq!(a.id, a.name.to_lowercase());
            match a.signing {
                Signing::Manifest { public_key } => {
                    let text = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, public_key).unwrap();
                    minisign_verify::PublicKey::decode(std::str::from_utf8(&text).unwrap()).unwrap();
                }
                Signing::Checksums { public_key_hex, prefix } => {
                    assert_eq!(public_key_hex.len(), 64);
                    assert!(prefix.starts_with(a.id));
                    assert!(!a.files.is_empty());
                }
            }
            assert!(a.supports(Platform::parse("macos-arm64").unwrap()), "{} has no Mac build", a.id);
        }
        assert!(!get("zenith").unwrap().supports(Platform::parse("windows-x86_64").unwrap()));
        assert!(find("Folio").is_ok());
        assert!(find("photoshop").unwrap_err().contains("ryolune"));
    }
}
