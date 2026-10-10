//! Installs and removes apps end to end against a stand-in for lsuite.xyz on loopback (the
//! builds route of DISTRIBUTION.md): a signed release manifest (AppImage) and signed checksums
//! (tar.gz), the sign-in it needs, and the refusals in between.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::catalog::{App, Signing};
use crate::platform::Platform;
use crate::release::tests::{keys, sign};
use crate::{Launcher, install, release};

type Routes = Arc<parking_lot::Mutex<HashMap<String, (u16, Vec<(String, String)>, Vec<u8>)>>>;

/// A tiny HTTP server: GET path → (status, headers, body).
async fn serve(routes: Routes) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { return };
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let authed = req.to_ascii_lowercase().contains("authorization: bearer lsk_test");
                let (status, headers, body) = if path.starts_with("/api/apps/") && !authed {
                    (401, vec![], br#"{"type":"error","error":{"type":"authentication_error","message":"Sign in"}}"#.to_vec())
                } else {
                    routes.lock().get(&path).cloned().unwrap_or((404, vec![], b"not found".to_vec()))
                };
                let mut head = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                for (k, v) in headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });
    base
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn targz(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = vec![];
    {
        let gz = flate2::write::GzEncoder::new(&mut out, flate2::Compression::fast());
        let mut t = tar::Builder::new(gz);
        for (name, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            t.append_data(&mut h, name, *data).unwrap();
        }
        t.into_inner().unwrap().finish().unwrap();
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn installs_updates_and_removes_from_signed_releases() {
    let home = tempfile::tempdir().unwrap();
    let routes: Routes = Arc::default();
    let base = serve(routes.clone()).await;
    // SAFETY: the only test in this crate that sets the environment.
    unsafe {
        std::env::set_var("LSUITE_HOME", home.path().join("lsuite"));
        std::env::set_var("LSUITE_APPS_DIR", home.path().join("apps"));
        std::env::set_var("LSUITE_GITHUB", &base);
        std::env::set_var("LSUITE_ACCOUNT_SERVER", &base);
    }
    let linux = Platform::parse("linux-x86_64").unwrap();
    let l = Launcher::new();
    // Signed out, the apps aren't offered.
    let probe: &'static App = Box::leak(Box::new(App { id: "folio", name: "folio", kind: "office", kind_label: "Office", summary: "t", repo: "t/folio", signing: Signing::Manifest { public_key: "x" }, files: &[] }));
    assert_eq!(release::latest(probe, linux).await.unwrap_err(), release::SIGN_IN);
    let account = json!({ "format": 1, "server": base, "email": "ada@example.com", "name": "Ada", "plan": "free", "token": "lsk_test_token_0123456789", "signedInAt": "2026-10-07T00:00:00Z" });
    std::fs::create_dir_all(home.path().join("lsuite")).unwrap();
    std::fs::write(home.path().join("lsuite/account.json"), account.to_string()).unwrap();

    // ---- A manifest app (AppImage), signed with a throwaway key. ----
    let k = keys();
    let manifest_app: &'static App = Box::leak(Box::new(App {
        id: "folio",
        name: "folio",
        kind: "office",
        kind_label: "Office",
        summary: "test",
        repo: "test/folio",
        signing: Signing::Manifest { public_key: leak(k.pk_b64.clone()) },
        files: &[],
    }));
    let appimage = b"#!/bin/sh\necho folio\n".repeat(50);
    // The server's answer: the release with its manifest, URLs on the server's file route.
    let latest = |version: &str, manifest: serde_json::Value| json!({ "app": "folio", "version": version, "tag": format!("folio-v{version}"), "manifest": manifest }).to_string().into_bytes();
    let publish = |version: &str, data: &[u8], signed_for: &str| {
        let path = format!("/api/apps/folio/files/folio-v{version}/folio-linux-x86_64.AppImage");
        let manifest = json!({ "version": version, "platforms": { "linux-x86_64-appimage": { "url": format!("{base}{path}"), "signature": sign(&k, data, signed_for) } } });
        let mut r = routes.lock();
        r.insert("/api/apps/folio/latest".into(), (200, vec![], latest(version, manifest)));
        // The file route sends the download on, like lsuite.xyz does to GitHub's signed address.
        r.insert(path, (302, vec![("Location".into(), format!("{base}/signed/folio-{version}"))], vec![]));
        r.insert(format!("/signed/folio-{version}"), (200, vec![], data.to_vec()));
    };
    publish("0.1.0", &appimage, "0.1.0");
    let rel = release::latest(manifest_app, linux).await.unwrap();
    assert_eq!(rel.version, "0.1.0");
    let mut p = l.hub.progress("t", "t");
    let rec = install::install(manifest_app, &rel, linux, &mut p).await.unwrap();
    assert_eq!(rec.executable, home.path().join("apps/folio/folio.AppImage"));
    assert_eq!(std::fs::read(&rec.executable).unwrap(), appimage);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&rec.executable).unwrap().permissions().mode() & 0o777, 0o755);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let entry = std::fs::read_to_string(home.path().join("apps/applications/xyz.lsuite.folio.desktop")).unwrap();
        assert!(entry.contains("Exec=\"") && entry.contains("folio.AppImage"));
    }
    let found = install::find("folio", linux.os).unwrap();
    assert!(found.managed);
    assert_eq!(found.version.as_deref(), Some("0.1.0"));

    // A tampered file is refused and the installed copy stays.
    let mut bad = b"#!/bin/sh\necho evil\n".repeat(50);
    let good_sig_for_other = sign(&k, &appimage, "0.2.0");
    {
        let path = "/api/apps/folio/files/folio-v0.2.0/folio-linux-x86_64.AppImage";
        let manifest = json!({ "version": "0.2.0", "platforms": { "linux-x86_64-appimage": { "url": format!("{base}{path}"), "signature": good_sig_for_other } } });
        let mut r = routes.lock();
        r.insert("/api/apps/folio/latest".into(), (200, vec![], latest("0.2.0", manifest)));
        r.insert(path.into(), (200, vec![], bad.clone()));
    }
    let rel = release::latest(manifest_app, linux).await.unwrap();
    let err = install::install(manifest_app, &rel, linux, &mut p).await.unwrap_err();
    assert!(err.contains("doesn't match its signature"), "{err}");
    assert_eq!(std::fs::read(&rec.executable).unwrap(), appimage, "the old copy is untouched");
    // A signature made for another version is refused before downloading.
    bad.truncate(10);
    publish("0.3.0", &bad, "0.1.0");
    assert!(release::latest(manifest_app, linux).await.unwrap_err().contains("signed for version 0.1.0"));
    // A manifest pointing elsewhere is refused.
    {
        let manifest = json!({ "version": "0.4.0", "platforms": { "linux-x86_64": { "url": "https://evil.example/folio.AppImage", "signature": sign(&k, b"x", "0.4.0") } } });
        routes.lock().insert("/api/apps/folio/latest".into(), (200, vec![], latest("0.4.0", manifest)));
    }
    assert!(release::latest(manifest_app, linux).await.unwrap_err().contains("outside lsuite's downloads"));
    // A proper update replaces it.
    let v2 = b"#!/bin/sh\necho folio 2\n".repeat(50);
    publish("0.2.0", &v2, "0.2.0");
    let rel = release::latest(manifest_app, linux).await.unwrap();
    let rec2 = install::install(manifest_app, &rel, linux, &mut p).await.unwrap();
    assert_eq!(std::fs::read(&rec2.executable).unwrap(), v2);
    assert_eq!(install::find("folio", linux.os).unwrap().version.as_deref(), Some("0.2.0"));

    // ---- A checksums app (tar.gz), signed with a throwaway Ed25519 key. ----
    let sk = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
    let key_hex: String = sk.verifying_key().as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let sums_app: &'static App = Box::leak(Box::new(App {
        id: "ryolune",
        name: "ryolune",
        kind: "music",
        kind_label: "Music",
        summary: "test",
        repo: "test/ryolune",
        signing: Signing::Checksums { public_key_hex: leak(key_hex), prefix: "ryolune-ed25519" },
        files: &[("linux-x86_64", "ryolune-linux-x86_64.tar.gz")],
    }));
    let archive = targz(&[("ryolune", b"#!/bin/sh\necho ryolune\n"), ("ryolune-cli", b"#!/bin/sh\n")]);
    let sha: String = {
        use sha2::Digest;
        sha2::Sha256::digest(&archive).iter().map(|b| format!("{b:02x}")).collect()
    };
    let sums = format!("{sha}  ryolune-linux-x86_64.tar.gz\n");
    use ed25519_dalek::Signer;
    let sig = format!("ryolune-ed25519 {}\n", base64::engine::general_purpose::STANDARD.encode(sk.sign(sums.as_bytes()).to_bytes()));
    let ryolune_latest = |sums: &str| json!({ "app": "ryolune", "version": "0.4.0", "tag": "ryolune-v0.4.0", "manifest": null, "sha256sums": sums, "sha256sumsSig": sig }).to_string().into_bytes();
    {
        let mut r = routes.lock();
        r.insert("/api/apps/ryolune/latest".into(), (200, vec![], ryolune_latest(&sums)));
        r.insert("/api/apps/ryolune/files/ryolune-v0.4.0/ryolune-linux-x86_64.tar.gz".into(), (200, vec![], archive.clone()));
    }
    let rel = release::latest(sums_app, linux).await.unwrap();
    assert_eq!(rel.version, "0.4.0");
    let rec = install::install(sums_app, &rel, linux, &mut p).await.unwrap();
    assert_eq!(rec.executable, home.path().join("apps/ryolune/ryolune"));
    assert!(home.path().join("apps/ryolune/ryolune-cli").is_file());
    // A forged checksums file is refused.
    routes.lock().insert("/api/apps/ryolune/latest".into(), (200, vec![], ryolune_latest(&format!("{}  ryolune-linux-x86_64.tar.gz\n", "0".repeat(64)))));
    assert!(release::latest(sums_app, linux).await.unwrap_err().contains("aren't signed"));

    // ---- Removing: refused while running, then done, documents untouched. ----
    let disc = home.path().join("lsuite/apps");
    std::fs::create_dir_all(&disc).unwrap();
    let running = json!({ "format": 1, "app": "ryolune", "version": "0.4.0", "appPath": rec.path, "executable": rec.executable, "running": { "pid": std::process::id() } });
    std::fs::write(disc.join("ryolune.json"), running.to_string()).unwrap();
    assert!(install::uninstall(sums_app, linux.os).unwrap_err().contains("is open"));
    let closed = json!({ "format": 1, "app": "ryolune", "version": "0.4.0", "appPath": rec.path, "executable": rec.executable, "running": null });
    std::fs::write(disc.join("ryolune.json"), closed.to_string()).unwrap();
    install::uninstall(sums_app, linux.os).unwrap();
    assert!(!home.path().join("apps/ryolune").exists());
    assert!(!disc.join("ryolune.json").exists(), "the discovery file of the removed copy goes too");
    assert!(install::find("ryolune", linux.os).is_none());
    assert!(install::records().contains_key("folio"));

    // A copy outside the usual places (a build from source) is never removed.
    let dev = home.path().join("src/target/debug");
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(dev.join("ryolune"), "dev").unwrap();
    let devdisc = json!({ "format": 1, "app": "ryolune", "version": "0.5.0-dev", "appPath": dev, "executable": dev.join("ryolune"), "running": null });
    std::fs::write(disc.join("ryolune.json"), devdisc.to_string()).unwrap();
    let f = install::find("ryolune", linux.os).unwrap();
    assert!(!f.managed && !f.standard);
    assert!(install::uninstall(sums_app, linux.os).unwrap_err().contains("wasn't installed by the launcher"));
    assert!(dev.join("ryolune").exists());

    // The registry: unknown parameters, agents held to their permissions.
    let err = crate::call(&l, crate::Source::Window, "apps.install", json!({ "app": "folio", "version": "1" })).await.unwrap_err();
    assert!(err.contains("no parameter \"version\""), "{err}");
    let err = crate::call(&l, crate::Source::Mcp, "apps.uninstall", json!({ "app": "folio" })).await.unwrap_err();
    assert!(err.contains("turned off for agents"), "{err}");
    let err = crate::call(&l, crate::Source::Mcp, "settings.set", json!({ "key": "agent.remove", "value": true })).await.unwrap_err();
    assert!(err.contains("own permissions"), "{err}");
    crate::call(&l, crate::Source::Cli, "settings.set", json!({ "key": "theme", "value": "dark" })).await.unwrap();
    assert!(crate::call(&l, crate::Source::Cli, "settings.set", json!({ "key": "theme", "value": "pink" })).await.is_err());
    assert!(crate::call(&l, crate::Source::Cli, "settings.set", json!({ "key": "autoUpdate", "value": "yes" })).await.is_err());
    // Signed out, cloud commands say to sign in first.
    std::fs::remove_file(home.path().join("lsuite/account.json")).unwrap();
    let err = crate::call(&l, crate::Source::Window, "cloud.status", json!({})).await.unwrap_err();
    assert!(err.contains("Sign in"), "{err}");
}

#[test]
fn commands_doc_is_current() {
    // Git on Windows may check the file out with CRLF line ends.
    let doc = include_str!("../../../docs/COMMANDS.md").replace("\r\n", "\n");
    assert_eq!(doc, crate::registry::markdown(), "run `cargo run -p lsuite-cli -- docs`");
}
