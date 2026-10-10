//! Finding an app's latest release for this computer, and downloading it verified.
//!
//! The builds aren't public (lsuite's DISTRIBUTION.md): they come through lsuite.xyz with the
//! account's token (`GET <server>/api/apps/<app>/latest`, then the file route, which sends the
//! download on to a short-lived address). Nothing is installed unless its signature checks
//! against the key built into the launcher (`catalog::APPS`): the minisign signature of the file
//! itself for release-manifest apps (whose trusted comment must name the release's version), or
//! the Ed25519 signature of `SHA256SUMS` plus the file's SHA-256 for checksum apps, so the server
//! can't change a build unnoticed. File URLs must be the server's own file route for that app.
//! `github()` (the launcher's own public releases) honours `LSUITE_GITHUB` for tests.

use std::path::Path;
use std::time::Duration;

use base64::Engine;
use futures::StreamExt;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::CmdResult;
use crate::catalog::{App, Signing, manifest_keys};
use crate::events::Progress;
use crate::platform::{Os, Platform};
use crate::util;

/// Larger downloads are refused (a release file is 60–300 MB).
pub const MAX_DOWNLOAD: u64 = 2 << 30;

/// `https://github.com`, or `LSUITE_GITHUB`.
pub fn github() -> String {
    std::env::var("LSUITE_GITHUB").ok().map(|s| s.trim().trim_end_matches('/').to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "https://github.com".into())
}

/// How the downloaded file becomes an installed app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileKind {
    /// `<app>.app` in a gzipped tar (macOS).
    MacBundleTarGz,
    /// `<app>.app` in a zip (macOS).
    MacZip,
    /// A self-contained Linux AppImage.
    AppImage,
    /// The app's binaries in a gzipped tar (Linux).
    TarGz,
    /// The app's binaries in a zip (Windows portable).
    Zip,
    /// An NSIS installer (Windows).
    WindowsInstaller,
}

impl FileKind {
    pub fn of(name: &str, os: Os) -> Option<Self> {
        let n = name.to_ascii_lowercase();
        Some(if n.ends_with(".app.tar.gz") {
            FileKind::MacBundleTarGz
        } else if n.ends_with(".appimage") {
            FileKind::AppImage
        } else if n.ends_with(".exe") && os == Os::Windows {
            FileKind::WindowsInstaller
        } else if n.ends_with(".zip") {
            if os == Os::Macos { FileKind::MacZip } else { FileKind::Zip }
        } else if n.ends_with(".tar.gz") {
            FileKind::TarGz
        } else {
            return None;
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    /// Base64 of the minisign signature file, and the key it must verify against.
    Minisign { signature: String, public_key: String },
    /// The file's SHA-256 (hex), from a signed `SHA256SUMS`.
    Sha256(String),
}

/// The latest release of one app for one platform.
#[derive(Clone, Debug)]
pub struct Release {
    pub app: String,
    pub version: String,
    pub file: String,
    pub url: String,
    pub kind: FileKind,
    pub notes: Option<String>,
    pub check: Check,
}

fn semver_of(s: &str) -> Option<semver::Version> {
    semver::Version::parse(s.trim().trim_start_matches('v')).ok()
}

/// Whether `latest` is newer than `installed` (unparseable versions compare as text).
pub fn is_newer(latest: &str, installed: &str) -> bool {
    match (semver_of(latest), semver_of(installed)) {
        (Some(a), Some(b)) => a > b,
        _ => latest.trim_start_matches('v') != installed.trim_start_matches('v'),
    }
}

/// Said when the apps are asked for while signed out.
pub const SIGN_IN: &str = "Sign in to lsuite to get the apps: the account is free.";

/// Finds the latest release of `app` for `p`, through lsuite.xyz.
pub async fn latest(app: &App, p: Platform) -> CmdResult<Release> {
    if !app.supports(p) {
        return Err(format!("{} has no {} build yet.", app.name, p.os_name()));
    }
    let acc = crate::account::read().ok_or(SIGN_IN)?;
    let server = crate::account::server();
    let r = util::client()
        .get(format!("{server}/api/apps/{}/latest", app.id))
        .bearer_auth(&acc.token)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach {server} for {}'s latest release ({e}).", app.name))?;
    let status = r.status().as_u16();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if status == 401 {
        return Err(SIGN_IN.into());
    }
    if status != 200 {
        return Err(crate::account::error_line(status, &v));
    }
    let version = v["version"].as_str().ok_or_else(|| format!("{}'s release has no version.", app.name))?.trim_start_matches('v').to_string();
    let tag = v["tag"].as_str().unwrap_or("").to_string();
    let files = format!("{server}/api/apps/{}/files/", app.id);
    match app.signing {
        Signing::Manifest { public_key } => from_manifest(app, p, &v["manifest"], &version, public_key, &files),
        Signing::Checksums { public_key_hex, prefix } => {
            let sums = v["sha256sums"].as_str().ok_or_else(|| format!("{} {version} has no signed checksums; nothing was downloaded.", app.name))?;
            let sig = v["sha256sumsSig"].as_str().ok_or_else(|| format!("{} {version} has no checksums signature; nothing was downloaded.", app.name))?;
            from_checksums(app, p, &version, &tag, sums, sig, public_key_hex, prefix, &files)
        }
    }
}

fn from_manifest(app: &App, p: Platform, m: &Value, version: &str, public_key: &str, files: &str) -> CmdResult<Release> {
    if m.is_null() {
        return Err(format!("{} {version} has no release manifest; nothing was downloaded.", app.name));
    }
    let signed_version = m["version"].as_str().map(|v| v.trim_start_matches('v').to_string()).unwrap_or_else(|| version.to_string());
    let entry = manifest_keys(p).iter().find_map(|k| m["platforms"].get(*k)).ok_or_else(|| format!("{} {version} has no {} build.", app.name, p.os_name()))?;
    let file_url = entry["url"].as_str().unwrap_or_default().to_string();
    let signature = entry["signature"].as_str().unwrap_or_default().to_string();
    if !file_url.starts_with(files) {
        return Err(format!("{}'s release manifest points outside lsuite's downloads; nothing was downloaded.", app.name));
    }
    if signature.is_empty() {
        return Err(format!("{} {version} isn't signed for {}; nothing was downloaded.", app.name, p.os_name()));
    }
    let file = file_url.rsplit('/').next().unwrap_or_default().to_string();
    let kind = FileKind::of(&file, p.os).ok_or_else(|| format!("The launcher can't install {file}."))?;
    // Refuse a signature made for another version before downloading anything.
    check_signed_version(&decode_signature(&signature)?, &signed_version)?;
    Ok(Release { app: app.id.into(), version: signed_version, file, url: file_url, kind, notes: m["notes"].as_str().map(str::to_string), check: Check::Minisign { signature, public_key: public_key.into() } })
}

#[allow(clippy::too_many_arguments)]
fn from_checksums(app: &App, p: Platform, version: &str, tag: &str, sums: &str, sig: &str, key_hex: &str, prefix: &str, files: &str) -> CmdResult<Release> {
    let file = app.files.iter().find(|(k, _)| *k == p.key()).map(|(_, f)| f.to_string()).ok_or_else(|| format!("{} has no {} build yet.", app.name, p.os_name()))?;
    verify_checksums(sums.as_bytes(), sig, key_hex, prefix).map_err(|e| format!("{}: {e}", app.name))?;
    let sha = sums
        .lines()
        .find_map(|l| {
            let (hash, name) = l.trim().split_once(char::is_whitespace)?;
            (name.trim().trim_start_matches('*') == file).then(|| hash.to_ascii_lowercase())
        })
        .ok_or_else(|| format!("{} {version}'s checksums don't list {file}; nothing was downloaded.", app.name))?;
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{}'s checksum for {file} is malformed.", app.name));
    }
    let kind = FileKind::of(&file, p.os).ok_or_else(|| format!("The launcher can't install {file}."))?;
    Ok(Release { app: app.id.into(), version: version.to_string(), url: format!("{files}{tag}/{file}"), file, kind, notes: None, check: Check::Sha256(sha) })
}

/// Checks `SHA256SUMS.sig` (`<prefix> <base64 signature>`) against the hex Ed25519 key.
pub fn verify_checksums(sums: &[u8], sig_text: &str, key_hex: &str, prefix: &str) -> Result<(), String> {
    let mut key = [0u8; 32];
    if key_hex.len() != 64 {
        return Err("the built-in release key is invalid".into());
    }
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&key_hex[i * 2..i * 2 + 2], 16).map_err(|_| "the built-in release key is invalid")?;
    }
    let line = sig_text.lines().map(str::trim).find(|l| l.starts_with(prefix)).ok_or("the release signature has an unknown format; nothing was downloaded")?;
    let raw = base64::engine::general_purpose::STANDARD.decode(line[prefix.len()..].trim()).map_err(|_| "the release signature isn't valid base64; nothing was downloaded")?;
    let sig = ed25519_dalek::Signature::from_slice(&raw).map_err(|_| "the release signature has the wrong length; nothing was downloaded")?;
    let vk = ed25519_dalek::VerifyingKey::from_bytes(&key).map_err(|_| "the built-in release key is invalid")?;
    vk.verify_strict(sums, &sig).map_err(|_| "the release checksums aren't signed by the app's key; nothing was downloaded".into())
}

fn b64_text(what: &str, b64: &str) -> Result<String, String> {
    let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| format!("The {what} isn't valid base64."))?;
    String::from_utf8(raw).map_err(|_| format!("The {what} isn't text."))
}

fn decode_key(b64: &str) -> Result<minisign_verify::PublicKey, String> {
    minisign_verify::PublicKey::decode(&b64_text("release key", b64)?).map_err(|e| format!("The release key is invalid: {e}"))
}

fn decode_signature(b64: &str) -> Result<minisign_verify::Signature, String> {
    minisign_verify::Signature::decode(&b64_text("release signature", b64)?).map_err(|e| format!("The release signature is invalid: {e}"))
}

fn signature_error(e: minisign_verify::Error) -> String {
    match e {
        minisign_verify::Error::UnexpectedKeyId => "The download was signed with another key than the app's; it was not installed.".into(),
        minisign_verify::Error::UnsupportedLegacyMode | minisign_verify::Error::UnexpectedAlgorithm => "The download's signature uses an unsupported algorithm; it was not installed.".into(),
        _ => "The download doesn't match its signature; it was not installed.".into(),
    }
}

/// The trusted comment (covered by the signature) must name the release's version.
fn check_signed_version(sig: &minisign_verify::Signature, version: &str) -> Result<(), String> {
    let signed = sig.trusted_comment().split('\t').find_map(|f| f.strip_prefix("version:")).map(str::trim);
    let Some(signed) = signed else { return Err("The release signature doesn't say which version it was made for; nothing was installed.".into()) };
    let same = match (semver_of(signed), semver_of(version)) {
        (Some(a), Some(b)) => a == b,
        _ => signed == version,
    };
    if same { Ok(()) } else { Err(format!("The release file was signed for version {signed}, not {version}; nothing was installed.")) }
}

enum Verifier<'a> {
    Minisign(Box<minisign_verify::StreamVerifier<'a>>),
    Sha256(Sha256, String),
}

/// Downloads the release's file to `dest`, checking it as it arrives; `dest` is removed when the
/// check fails.
pub async fn download(r: &Release, dest: &Path, progress: &mut Progress) -> CmdResult<()> {
    let result = download_inner(r, dest, progress).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
}

async fn download_inner(r: &Release, dest: &Path, progress: &mut Progress) -> CmdResult<()> {
    // The verifier borrows the key and the signature: they live as long as the download.
    let minisign = match &r.check {
        Check::Minisign { signature, public_key } => Some((decode_key(public_key)?, decode_signature(signature)?)),
        Check::Sha256(_) => None,
    };
    let mut verifier = match (&r.check, &minisign) {
        (_, Some((pk, sig))) => {
            check_signed_version(sig, &r.version)?;
            Verifier::Minisign(Box::new(pk.verify_stream(sig).map_err(signature_error)?))
        }
        (Check::Sha256(hex), None) => Verifier::Sha256(Sha256::new(), hex.clone()),
        (Check::Minisign { .. }, None) => unreachable!(),
    };
    let failed = |e: reqwest::Error| format!("The download failed ({e}).");
    let mut req = util::transfer_client().get(&r.url).header(reqwest::header::ACCEPT, "application/octet-stream");
    // lsuite's file route needs the account; the address it sends on to doesn't (and the token
    // isn't sent across the redirect).
    if r.url.starts_with(&crate::account::server())
        && let Some(acc) = crate::account::read()
    {
        req = req.bearer_auth(acc.token);
    }
    let res = req.send().await.map_err(failed)?;
    if !res.status().is_success() {
        let status = res.status().as_u16();
        if status == 401 {
            return Err(SIGN_IN.into());
        }
        return Err(format!("The download failed: the server answered {status}."));
    }
    let total = res.content_length().filter(|&n| n > 0);
    if total.is_some_and(|n| n > MAX_DOWNLOAD) {
        return Err("The release file is unexpectedly large; it was not downloaded.".into());
    }
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| format!("Couldn't prepare the download ({e})."))?;
    }
    let mut out = tokio::fs::File::create(dest).await.map_err(|e| format!("Couldn't save the download ({e})."))?;
    let mut stream = res.bytes_stream();
    let mut got = 0u64;
    progress.update(0, total);
    loop {
        let next = tokio::time::timeout(Duration::from_secs(60), stream.next()).await.map_err(|_| "The download stalled; try again.".to_string())?;
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(failed)?;
        got += chunk.len() as u64;
        if got > MAX_DOWNLOAD {
            return Err("The release file is unexpectedly large; the download was stopped.".into());
        }
        match &mut verifier {
            Verifier::Minisign(v) => v.update(&chunk),
            Verifier::Sha256(h, _) => h.update(&chunk),
        }
        out.write_all(&chunk).await.map_err(|e| format!("Couldn't save the download ({e})."))?;
        progress.update(got, total);
    }
    out.flush().await.map_err(|e| e.to_string())?;
    drop(out);
    if total.is_some_and(|t| t != got) {
        return Err("The download was cut short; try again.".into());
    }
    match verifier {
        Verifier::Minisign(mut v) => v.finalize().map_err(signature_error)?,
        Verifier::Sha256(h, want) => {
            let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if got != want {
                return Err("The download doesn't match the signed checksum; it was not installed.".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub struct Keys {
        pub sk: minisign::SecretKey,
        pub pk_b64: String,
    }

    pub fn keys() -> Keys {
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Keys { pk_b64: base64::engine::general_purpose::STANDARD.encode(kp.pk.to_box().unwrap().to_string()), sk: kp.sk }
    }

    /// Signs like the Tauri CLI: base64 of the signature file, the version in the trusted comment.
    pub fn sign(k: &Keys, data: &[u8], version: &str) -> String {
        let trusted = format!("timestamp:1\tfile:x\tversion:{version}");
        let sig = minisign::sign(None, &k.sk, std::io::Cursor::new(data), Some(&trusted), Some("signature from tauri secret key")).unwrap();
        base64::engine::general_purpose::STANDARD.encode(sig.to_string())
    }

    #[test]
    fn versions_compare_as_semver() {
        assert!(is_newer("0.10.0", "0.9.1"));
        assert!(is_newer("v1.0.0", "0.15.3"));
        assert!(!is_newer("0.15.3", "0.15.3"));
        assert!(!is_newer("0.15.2", "v0.15.3"));
    }

    #[test]
    fn file_kinds_follow_the_names() {
        assert_eq!(FileKind::of("folio-macos-arm64.app.tar.gz", Os::Macos), Some(FileKind::MacBundleTarGz));
        assert_eq!(FileKind::of("ryolune-macos-arm64.zip", Os::Macos), Some(FileKind::MacZip));
        assert_eq!(FileKind::of("nori_amd64.AppImage", Os::Linux), Some(FileKind::AppImage));
        assert_eq!(FileKind::of("ryolune-linux-x86_64.tar.gz", Os::Linux), Some(FileKind::TarGz));
        assert_eq!(FileKind::of("kimchi_x64-setup.exe", Os::Windows), Some(FileKind::WindowsInstaller));
        assert_eq!(FileKind::of("ryolune-windows-x86_64.zip", Os::Windows), Some(FileKind::Zip));
        assert_eq!(FileKind::of("kimchi_aarch64.dmg", Os::Macos), None);
    }

    #[test]
    fn checksums_signatures_check_against_the_key() {
        let sk = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let key_hex: String = sk.verifying_key().as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let sums = b"abc  ryolune-linux-x86_64.tar.gz\n";
        use ed25519_dalek::Signer;
        let sig = format!("ryolune-ed25519 {}\n", base64::engine::general_purpose::STANDARD.encode(sk.sign(sums).to_bytes()));
        verify_checksums(sums, &sig, &key_hex, "ryolune-ed25519").unwrap();
        assert!(verify_checksums(b"abd  ryolune-linux-x86_64.tar.gz\n", &sig, &key_hex, "ryolune-ed25519").is_err());
        assert!(verify_checksums(sums, &sig, &key_hex, "kimchi-ed25519").is_err());
        let other = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let other_hex: String = other.verifying_key().as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify_checksums(sums, &sig, &other_hex, "ryolune-ed25519").is_err());
    }

    #[test]
    fn real_release_signatures_verify() {
        // ryolune 0.15.3's published SHA256SUMS and its signature (the key in the catalogue).
        let sums = "6dcf88bd8006f56498cd2a7a8676fbb60eb633b4890981fc7bf05ff0e40bf3d9  ryolune-macos-arm64.zip\n4f237ee5e675f2984dd1ec60c6220f4748cbce8ce228c408386f83ca39a87946  ryolune-macos-x86_64.zip\n557ab694af4490f689cc848e5488cd66e84f0b377471ba993b9068f675c7e062  ryolune-linux-x86_64.zip\n148089efd4edf099b5e5643cf3e62e2036572f285c8e36f3744423d39d19e73e  ryolune-linux-x86_64.tar.gz\n909d66941f8c915f2decb5391a30d1d0f7be50c0d4d9450db83187ca7279bc7a  ryolune-windows-x86_64.zip\n04f393e13e9d41246d9562194f3f7cfb63f9acc5bd782bb3afe6f94bae9b2cb9  ryolune-linux-x86_64\n9a2e923b644fe30432da04f305f0a6e30ebdcdba57d784538e84c87c2135a10c  ryolune-windows-x86_64.exe\nd718fdb82b9c31db70b380635be862199624fa1abb3978e6cb183c0b4677d1bd  ryolune-Afterglow-demo.zip\n";
        let sig = "ryolune-ed25519 yEaZlzUUcXEF998NhIbA8vKSRX6vS87xJFFsS33LNbSPIMYiTUbnZCXF5oi1L6QUOcOukTKCnMt4VGOXkcbECQ==\n";
        let Signing::Checksums { public_key_hex, prefix } = crate::catalog::get("ryolune").unwrap().signing else { panic!() };
        verify_checksums(sums.as_bytes(), sig, public_key_hex, prefix).unwrap();
    }

    #[test]
    fn signed_version_must_match() {
        let k = keys();
        let s = decode_signature(&sign(&k, b"data", "0.2.0")).unwrap();
        check_signed_version(&s, "0.2.0").unwrap();
        check_signed_version(&s, "v0.2.0").unwrap();
        assert!(check_signed_version(&s, "0.3.0").unwrap_err().contains("0.2.0"));
    }
}
