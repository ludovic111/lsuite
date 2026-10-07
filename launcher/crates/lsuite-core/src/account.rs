//! lsuite AI: the one account of the suite (lsuite's AI.md), shared with every app.
//!
//! The account lives in `~/.lsuite/account.json` (0600, written atomically; `LSUITE_HOME`
//! replaces `~/.lsuite`) and is read again whenever it is needed, since an app may change it.
//! Signing in here signs in every lsuite app on the computer, and the other way round. The
//! token is a secret: never logged, masked in answers.
//!
//! Signing in works like native apps' OAuth: the launcher listens on a random loopback port,
//! opens `<server>/account/connect?app=lsuite&port=…&state=…` in the browser, and the page sends
//! the browser back to `http://127.0.0.1:<port>/callback?code=…&state=…`; the code is exchanged
//! with `POST <server>/api/account/token`. Headless: a key from the account page (`lsk_…`).

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{CmdResult, paths, util};

pub const DEFAULT_SERVER: &str = "https://lsuite.xyz";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountFile {
    pub format: u32,
    pub server: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub plan: String,
    pub token: String,
    pub signed_in_at: chrono::DateTime<chrono::Utc>,
}

pub fn path() -> PathBuf {
    paths::lsuite_home().join("account.json")
}

/// The account on this computer, if signed in.
pub fn read() -> Option<AccountFile> {
    let bytes = std::fs::read(path()).ok()?;
    serde_json::from_slice::<AccountFile>(&bytes).ok().filter(|a| a.format == 1 && !a.token.is_empty())
}

pub fn write(a: &AccountFile) -> CmdResult<()> {
    let json = serde_json::to_vec_pretty(a).map_err(|e| e.to_string())?;
    util::write_private(&path(), &json).map_err(|e| format!("Couldn't write {} ({e}).", path().display()))
}

/// The server: `LSUITE_ACCOUNT_SERVER`, else the signed-in account's, else lsuite.xyz.
pub fn server() -> String {
    if let Ok(s) = std::env::var("LSUITE_ACCOUNT_SERVER")
        && !s.trim().is_empty()
    {
        return s.trim().trim_end_matches('/').to_string();
    }
    read().map(|a| a.server).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

pub fn manage_url() -> String {
    format!("{}/account", server())
}

/// A token shown in answers: its start and end only.
pub fn mask(token: &str) -> String {
    if token.len() <= 10 { "••••".into() } else { format!("{}…{}", &token[..6], &token[token.len() - 4..]) }
}

/// One line from an error in Anthropic's shape (`{type: "error", error: {type, message}}`).
pub fn error_line(status: u16, body: &Value) -> String {
    body["error"]["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("The lsuite server answered {status}."))
}

/// `GET /api/account/me` with a token.
pub async fn me(server: &str, token: &str) -> CmdResult<Value> {
    let r = util::client().get(format!("{server}/api/account/me")).bearer_auth(token).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    if status != 200 {
        return Err(error_line(status, &body));
    }
    Ok(body)
}

/// `GET /api/ai/plans`.
pub async fn plans() -> CmdResult<Value> {
    let server = server();
    let r = util::client().get(format!("{server}/api/ai/plans")).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    if status != 200 {
        return Err(error_line(status, &body));
    }
    Ok(body)
}

/// "Pro · 38 % used · resets 1 Nov".
pub fn summary(me: &Value) -> String {
    let plan = me["planName"].as_str().or(me["plan"].as_str()).unwrap_or("Free");
    let pct = me["usage"]["percent"].as_f64().unwrap_or(0.0);
    let resets = me["usage"]["resetsAt"].as_str().and_then(|r| chrono::DateTime::parse_from_rfc3339(r).ok()).map(|d| d.format("%-d %b").to_string());
    match resets {
        Some(r) if me["usage"]["limit"].as_f64().unwrap_or(0.0) > 0.0 => format!("{plan} · {pct:.0} % used · resets {r}"),
        _ => plan.to_string(),
    }
}

/// What `account.status` answers: always asked again of the server (plans and allowances
/// change there); offline, what the account file says, with `offline: true`.
pub async fn status() -> CmdResult<Value> {
    let Some(acc) = read() else {
        return Ok(json!({ "signedIn": false, "server": server(), "pitch": "No setup. Sign in and your agents work in every app.", "manageUrl": manage_url() }));
    };
    let mut v = json!({
        "signedIn": true,
        "server": acc.server,
        "email": acc.email,
        "name": acc.name,
        "plan": acc.plan,
        "token": mask(&acc.token),
        "manageUrl": format!("{}/account", acc.server),
        "signedInAt": acc.signed_in_at,
    });
    match me(&acc.server, &acc.token).await {
        Ok(m) => {
            if m["plan"].as_str().is_some_and(|p| p != acc.plan) {
                let mut a2 = acc.clone();
                a2.plan = m["plan"].as_str().unwrap_or("").to_string();
                let _ = write(&a2);
            }
            for k in ["plan", "planName", "status", "usage", "models", "defaultModel", "demo", "cloud"] {
                v[k] = m[k].clone();
            }
            if let Some(u) = m["manageUrl"].as_str() {
                v["manageUrl"] = json!(u);
            }
            v["summary"] = json!(summary(&m));
        }
        Err(e) => {
            v["offline"] = json!(true);
            v["error"] = json!(e);
        }
    }
    Ok(v)
}

/// Signs in with a key from the account page.
pub async fn sign_in_key(key: &str) -> CmdResult<Value> {
    let server = server();
    let key = key.trim();
    if !key.starts_with("lsk_") {
        return Err(format!("That isn't an lsuite key (they start with lsk_). Make one at {server}/account."));
    }
    let m = me(&server, key).await?;
    let acc = AccountFile {
        format: 1,
        server: server.clone(),
        email: m["email"].as_str().unwrap_or("").to_string(),
        name: m["name"].as_str().unwrap_or("").to_string(),
        plan: m["plan"].as_str().unwrap_or("").to_string(),
        token: key.to_string(),
        signed_in_at: chrono::Utc::now(),
    };
    write(&acc)?;
    Ok(json!({ "signedIn": true, "email": acc.email, "plan": acc.plan, "summary": summary(&m) }))
}

/// A browser sign-in in progress: where the browser was sent, and the task finishing it.
pub struct BrowserSignIn {
    pub url: String,
    pub done: tokio::task::JoinHandle<CmdResult<Value>>,
}

/// Starts the loopback sign-in: listens, opens the browser, and finishes when the page sends
/// the browser back (or after five minutes).
pub async fn sign_in_browser(open_browser: bool) -> CmdResult<BrowserSignIn> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let server = server();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| format!("Couldn't listen for the sign-in ({e})."))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let state = util::random_token();
    let url = format!("{server}/account/connect?app=lsuite&port={port}&state={state}");
    if open_browser {
        let _ = util::open_external(&url);
    }
    let done = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
        loop {
            let accepted = tokio::time::timeout_at(deadline, listener.accept()).await;
            let Ok(Ok((mut stream, peer))) = accepted else { return Err("The sign-in wasn't finished in the browser within 5 minutes.".to_string()) };
            if !peer.ip().is_loopback() {
                continue;
            }
            let mut buf = vec![0u8; 8192];
            let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await.ok().and_then(Result::ok).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = req.split_whitespace().nth(1).unwrap_or("").to_string();
            if !target.starts_with("/callback") {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            }
            let q = url::Url::parse(&format!("http://127.0.0.1{target}")).ok();
            let param = |k: &str| q.as_ref().and_then(|u| u.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.into_owned()));
            let (code, st) = (param("code"), param("state"));
            let page = |ok: bool, msg: &str| {
                let body = format!(
                    "<!doctype html><meta charset=utf-8><title>lsuite</title><body style=\"font-family:system-ui;background:#050505;color:#f2f2f2;display:grid;place-items:center;height:100vh;margin:0\"><div><h1 style=\"font-weight:600\">{}</h1><p>{msg}</p></div>",
                    if ok { "lsuite is connected" } else { "Sign-in didn't finish" }
                );
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
            };
            let (Some(code), true) = (code, st.as_deref() == Some(state.as_str())) else {
                let _ = stream.write_all(page(false, "This page didn't come from the launcher's sign-in. Start again from lsuite.").as_bytes()).await;
                continue;
            };
            let result = exchange(&server, &code).await;
            let msg = match &result {
                Ok(_) => "You can close this tab and go back to lsuite. Every lsuite app on this computer is signed in too.".to_string(),
                Err(e) => e.clone(),
            };
            let _ = stream.write_all(page(result.is_ok(), &msg).as_bytes()).await;
            return result;
        }
    });
    Ok(BrowserSignIn { url, done })
}

/// `POST /api/account/token {code}` → the account file.
async fn exchange(server: &str, code: &str) -> CmdResult<Value> {
    let r = util::client().post(format!("{server}/api/account/token")).json(&json!({ "code": code })).send().await.map_err(|e| format!("Couldn't reach {server} ({e})."))?;
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    if status != 200 {
        return Err(error_line(status, &body));
    }
    let token = body["token"].as_str().ok_or("The server didn't send a token.")?.to_string();
    let a = &body["account"];
    let acc = AccountFile {
        format: 1,
        server: server.to_string(),
        email: a["email"].as_str().unwrap_or("").to_string(),
        name: a["name"].as_str().unwrap_or("").to_string(),
        plan: a["plan"].as_str().unwrap_or("").to_string(),
        token,
        signed_in_at: chrono::Utc::now(),
    };
    write(&acc)?;
    Ok(json!({ "signedIn": true, "email": acc.email, "plan": acc.plan, "summary": summary(a) }))
}

/// Signs out here and on the server (the token is revoked). Every app on the computer is
/// signed out with it.
pub async fn sign_out() -> CmdResult<Value> {
    let Some(acc) = read() else { return Ok(json!({ "signedIn": false })) };
    let _ = util::client().post(format!("{}/api/account/signout", acc.server)).bearer_auth(&acc.token).send().await;
    let _ = std::fs::remove_file(path());
    Ok(json!({ "signedIn": false, "was": acc.email }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_and_masks() {
        let me = json!({ "planName": "Pro", "usage": { "percent": 38.2, "limit": 4000, "resetsAt": "2026-11-01T00:00:00.000Z" } });
        assert_eq!(summary(&me), "Pro · 38 % used · resets 1 Nov");
        assert_eq!(summary(&json!({ "plan": "free" })), "free");
        assert_eq!(mask("lsk_abcdefghijklmnop"), "lsk_ab…mnop");
        assert_eq!(mask("short"), "••••");
    }
}
