//! The lsuite agent (lsuite's HARNESS.md, part 8): one agent for jobs that span the apps.
//!
//! It plans the job, then drives each installed app through the app's own MCP server and
//! harness: it reads the app's expert brief and skills first (`harness.brief`, `harness.skill`),
//! looks up the commands it needs, calls them, and looks at the result (`harness.look`) before it
//! says it's done. With four apps and some 800 commands, it doesn't get every command at once:
//! its tools are `app_brief`, `app_skill`, `app_tools` (the app's commands, filtered), `app_call`,
//! `app_look`, and the launcher's own commands (apps, cloud, marketplace) as `lsuite_call`.
//!
//! Two ways to run it:
//! - **Claude Code** (`claude` on the computer, no key needed): Claude Code runs the loop, with
//!   every installed app's MCP server and the launcher's own (`lsuite mcp`) and the suite brief
//!   appended to its prompt.
//! - **lsuite AI** (a Pass plan) or **Anthropic** (`ANTHROPIC_API_KEY`): the launcher runs the
//!   loop itself against the Messages API, through MCP clients of the apps' servers.
//!
//! The conversation lives in [`Launcher::agent`]; the window draws it and `agent.*` commands drive it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::{CmdResult, Launcher, account, catalog, install, registry, util};

/// Steps of one run at most (a step is one model answer).
pub const MAX_STEPS: usize = 40;

/// The suite brief: how the lsuite agent works.
pub const BRIEF: &str = r#"You are the lsuite agent. lsuite is a suite of free, open-source creative and office apps that agents can drive end to end: ryolune (music, a DAW), kimchi (video editing, motion graphics, 3D), nori (images, vector design and page layout in one document) and folio (documents, spreadsheets and slides in one file). You run in the lsuite app on the person's computer and you can drive every installed app.

How you work:
1. Understand the job and say in one line what you'll make. If something essential is missing, ask one short question; otherwise pick sensible defaults and go.
2. For each app you'll use, read its expert brief first (app_brief) and the skill that fits the job (app_skill). The brief tells you the app's model of the document, the commands for common jobs and the quality bar of its trade. Follow it: it is how work is done well in that app.
3. Find the commands you need with app_tools (filter by a word), then do the work with app_call. Prefer a few well-chosen commands over many small ones; batch where the app offers it.
4. Moving work between apps: export a file from one app (an audio mix, a video, a picture, a PDF) and import it in the next; say where the file is.
5. Look at what you made before you say it's done: app_look shows you the app's picture of the work (a frame, a page, a slide, a bar range) with its numbers (loudness, contrast, formula errors). Compare it with the request and fix what's off, up to three passes.
6. Finish with a short report: what you made, in which app and file, and anything the person should check. Every app keeps your changes as one undo step per turn.

If an app refuses an action for its own permissions (Settings › Agent in that app), tell the person exactly which setting to turn on; don't try to get around it.

Rules: don't invent commands (look them up with app_tools); if an app isn't installed, offer to install it (lsuite_call apps.install); never delete the person's files or documents unless asked; keep secrets out of documents."#;

/// One entry of the conversation, as the window shows it.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Entry {
    User { text: String },
    Assistant { text: String },
    /// A tool the agent used, with a short line of what came back.
    #[serde(rename_all = "camelCase")]
    Tool { app: String, tool: String, input: String, output: String, ok: bool, image: Option<PathBuf> },
    Note { text: String },
    Error { text: String },
}

#[derive(Default)]
pub struct State {
    pub log: Vec<Entry>,
    pub running: bool,
    pub provider: Option<String>,
    stop: Option<tokio::task::AbortHandle>,
    /// The Messages API conversation, kept between runs so follow-ups have context.
    messages: Vec<Value>,
}

/// Which way the agent can run on this computer, best first.
pub fn providers(l: &Launcher) -> Value {
    let pass = account::read().is_some_and(|a| a.plan != "free" && !a.plan.is_empty());
    let claude = which("claude");
    let key = std::env::var("ANTHROPIC_API_KEY").is_ok_and(|k| !k.trim().is_empty());
    let mut list = vec![];
    if claude.is_some() {
        list.push(json!({ "id": "claude-code", "name": "Claude Code", "ready": true, "note": "Your Claude Code, with every app's tools" }));
    }
    list.push(json!({ "id": "lsuite", "name": "lsuite AI", "ready": pass, "note": if pass { "Your lsuite Pass" } else { "Comes with lsuite Pass" } }));
    list.push(json!({ "id": "anthropic", "name": "Anthropic API", "ready": key, "note": if key { "ANTHROPIC_API_KEY" } else { "Set ANTHROPIC_API_KEY" } }));
    let default = list.iter().find(|p| p["ready"] == true).and_then(|p| p["id"].as_str()).map(str::to_string);
    let _ = l;
    json!({ "providers": list, "default": default })
}

fn which(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join(".local/bin"));
        dirs.push(h.join(".claude/local"));
    }
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

/// The installed apps with an MCP server: `(app, program, args)`.
fn app_servers() -> Vec<(String, PathBuf, Vec<String>)> {
    catalog::APPS
        .iter()
        .filter_map(|a| {
            let d = install::discovery(a.id)?;
            let mcp = PathBuf::from(d["mcp"].as_str()?);
            if !mcp.is_file() {
                return None;
            }
            let live = install::running_pid(a.id).is_some();
            Some((a.id.to_string(), mcp, if live { vec!["--live".to_string()] } else { vec![] }))
        })
        .collect()
}

fn push(l: &Launcher, e: Entry) {
    l.agent.lock().log.push(e);
    l.hub.changed("agent");
}

/// Runs the agent on `prompt` until it finishes (or is stopped). Returns its last answer.
pub async fn run(l: &Arc<Launcher>, prompt: &str, provider: Option<&str>) -> CmdResult<Value> {
    let p = providers(l);
    let chosen = provider.map(str::to_string).or_else(|| p["default"].as_str().map(str::to_string)).ok_or("No way to run the agent here: install Claude Code, or sign in with lsuite Pass.")?;
    {
        let mut st = l.agent.lock();
        if st.running {
            return Err("The agent is already working. Stop it first.".into());
        }
        st.running = true;
        st.provider = Some(chosen.clone());
    }
    push(l, Entry::User { text: prompt.to_string() });
    let (l2, prompt2, chosen2) = (l.clone(), prompt.to_string(), chosen.clone());
    let task = tokio::spawn(async move {
        match chosen2.as_str() {
            "claude-code" => run_claude_code(&l2, &prompt2).await,
            other => run_messages(&l2, &prompt2, other).await,
        }
    });
    l.agent.lock().stop = Some(task.abort_handle());
    let result = match task.await {
        Ok(r) => r,
        Err(e) if e.is_cancelled() => Err("Stopped.".to_string()),
        Err(e) => Err(e.to_string()),
    };
    {
        let mut st = l.agent.lock();
        st.running = false;
        st.stop = None;
    }
    if let Err(e) = &result {
        push(l, Entry::Error { text: e.clone() });
    }
    l.hub.changed("agent");
    result.map(|text| json!({ "provider": chosen, "answer": text }))
}

pub fn stop(l: &Launcher) -> bool {
    l.agent.lock().stop.take().map(|h| h.abort()).is_some()
}

pub fn clear(l: &Launcher) {
    let mut st = l.agent.lock();
    if !st.running {
        st.log.clear();
        st.messages.clear();
    }
    drop(st);
    l.hub.changed("agent");
}

pub fn log(l: &Launcher) -> Value {
    let st = l.agent.lock();
    json!({ "running": st.running, "provider": st.provider, "log": st.log })
}

// ---- Claude Code -------------------------------------------------------------------------

/// The launcher's own MCP server: `lsuite mcp` beside the window, or `lsuite-mcp`.
fn launcher_mcp() -> Option<(PathBuf, Vec<String>)> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let mcp = dir.join(if cfg!(windows) { "lsuite-mcp.exe" } else { "lsuite-mcp" });
    if mcp.is_file() {
        return Some((mcp, vec![]));
    }
    std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()).map(|a| (a, vec!["mcp".to_string()]))
}

async fn run_claude_code(l: &Arc<Launcher>, prompt: &str) -> CmdResult<String> {
    let claude = which("claude").ok_or("Claude Code isn't installed (claude).")?;
    let mut servers = serde_json::Map::new();
    for (app, mcp, args) in app_servers() {
        servers.insert(app, json!({ "command": mcp, "args": args }));
    }
    if let Some((mcp, args)) = launcher_mcp() {
        servers.insert("lsuite".into(), json!({ "command": mcp, "args": args }));
    }
    if servers.is_empty() {
        push(l, Entry::Note { text: "No lsuite app is installed yet (or none has started once): the agent can only install them.".into() });
    }
    let config = crate::paths::launcher_dir().join("agent-mcp.json");
    util::write_private(&config, &serde_json::to_vec_pretty(&json!({ "mcpServers": servers })).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let brief = format!(
        "{BRIEF}\n\nWith Claude Code, each app's tools are its MCP server's (`mcp__<app>__<tool>`): use `harness_brief`, `harness_skills`/`harness_skill` and `harness_look` there when the app has them (else its MCP `instructions` and prompts), and the launcher's own as `mcp__lsuite__*`. Installed apps with tools: {}.",
        servers.keys().cloned().collect::<Vec<_>>().join(", ")
    );
    let mut cmd = Command::new(claude);
    cmd.arg("-p")
        .arg(prompt)
        .args(["--output-format", "stream-json", "--verbose"])
        .arg("--mcp-config")
        .arg(&config)
        .arg("--strict-mcp-config")
        .arg("--append-system-prompt")
        .arg(&brief)
        .arg("--allowedTools")
        .args(servers.keys().map(|k| format!("mcp__{k}")).chain(["Read".to_string()]))
        .current_dir(dirs::home_dir().unwrap_or_else(std::env::temp_dir))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Claude Code ({e})."))?;
    let out = child.stdout.take().ok_or("Claude Code has no output.")?;
    let mut lines = BufReader::new(out).lines();
    let mut answer = String::new();
    let mut pending: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        match v["type"].as_str() {
            Some("assistant") => {
                for c in v["message"]["content"].as_array().into_iter().flatten() {
                    match c["type"].as_str() {
                        Some("text") => {
                            let t = c["text"].as_str().unwrap_or("").trim().to_string();
                            if !t.is_empty() {
                                answer = t.clone();
                                push(l, Entry::Assistant { text: t });
                            }
                        }
                        Some("tool_use") => {
                            let name = c["name"].as_str().unwrap_or("").to_string();
                            let (app, tool) = split_tool(&name);
                            pending.insert(c["id"].as_str().unwrap_or("").to_string(), (app, tool, short(&c["input"].to_string(), 160)));
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for c in v["message"]["content"].as_array().into_iter().flatten() {
                    if c["type"] == "tool_result"
                        && let Some((app, tool, input)) = pending.remove(c["tool_use_id"].as_str().unwrap_or(""))
                    {
                        let (text, image) = tool_result_parts(&c["content"]);
                        push(l, Entry::Tool { app, tool, input, output: short(&text, 240), ok: c["is_error"] != true, image });
                    }
                }
            }
            Some("result") => {
                if let Some(r) = v["result"].as_str().filter(|r| !r.trim().is_empty()) {
                    answer = r.trim().to_string();
                }
                if v["is_error"] == true {
                    return Err(answer);
                }
            }
            _ => {}
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if !status.success() && answer.is_empty() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut e, &mut err).await;
        }
        return Err(format!("Claude Code stopped ({status}): {}", short(err.trim(), 300)));
    }
    Ok(answer)
}

/// `mcp__kimchi__project_overview` → ("kimchi", "project_overview").
fn split_tool(name: &str) -> (String, String) {
    let rest = name.strip_prefix("mcp__").unwrap_or(name);
    match rest.split_once("__") {
        Some((app, tool)) => (app.to_string(), tool.to_string()),
        None => (String::new(), rest.to_string()),
    }
}

/// A tool result's text, and its first picture saved to a file the window can show.
fn tool_result_parts(content: &Value) -> (String, Option<PathBuf>) {
    let mut text = String::new();
    let mut image = None;
    let items: Vec<Value> = match content {
        Value::String(s) => vec![json!({ "type": "text", "text": s })],
        Value::Array(a) => a.clone(),
        _ => vec![],
    };
    for c in items {
        match c["type"].as_str() {
            Some("text") => text.push_str(c["text"].as_str().unwrap_or("")),
            Some("image") if image.is_none() => {
                let data = c["source"]["data"].as_str().or(c["data"].as_str()).unwrap_or("");
                image = save_image(data);
            }
            _ => {}
        }
    }
    (text, image)
}

fn save_image(b64: &str) -> Option<PathBuf> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let dir = crate::paths::launcher_dir().join("agent-pictures");
    std::fs::create_dir_all(&dir).ok()?;
    let p = dir.join(format!("{}.png", util::random_token().chars().filter(char::is_ascii_alphanumeric).take(12).collect::<String>()));
    std::fs::write(&p, bytes).ok()?;
    Some(p)
}

fn short(s: &str, n: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= n { s } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

// ---- MCP clients (for the Messages API loop) ---------------------------------------------

struct McpClient {
    _child: Child,
    stdin: ChildStdin,
    stdout: tokio::io::Lines<BufReader<ChildStdout>>,
    next: u64,
}

impl McpClient {
    async fn start(program: &PathBuf, args: &[String]) -> CmdResult<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Couldn't start {} ({e}).", program.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?).lines();
        let mut c = McpClient { _child: child, stdin, stdout, next: 1 };
        c.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "lsuite-agent", "version": crate::VERSION } })).await?;
        c.notify("notifications/initialized").await?;
        Ok(c)
    }

    async fn notify(&mut self, method: &str) -> CmdResult<()> {
        let line = format!("{}\n", json!({ "jsonrpc": "2.0", "method": method }));
        self.stdin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())
    }

    async fn request(&mut self, method: &str, params: Value) -> CmdResult<Value> {
        let id = self.next;
        self.next += 1;
        let line = format!("{}\n", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.stdin.write_all(line.as_bytes()).await.map_err(|e| e.to_string())?;
        self.stdin.flush().await.map_err(|e| e.to_string())?;
        loop {
            let next = tokio::time::timeout(Duration::from_secs(600), self.stdout.next_line()).await.map_err(|_| "The app didn't answer within 10 minutes.".to_string())?;
            let Some(line) = next.map_err(|e| e.to_string())? else { return Err("The app's MCP server stopped.".into()) };
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v["id"].as_u64() == Some(id) {
                if let Some(e) = v.get("error") {
                    return Err(e["message"].as_str().unwrap_or("MCP error").to_string());
                }
                return Ok(v["result"].clone());
            }
        }
    }
}

/// The agent's tools for the Messages API loop.
fn tool_defs() -> Value {
    let app = json!({ "type": "string", "description": "The app: ryolune, kimchi, nori or folio." });
    json!([
        { "name": "app_brief", "description": "The app's expert brief: how work is done well in it. Read it before using an app.", "input_schema": { "type": "object", "properties": { "app": app }, "required": ["app"] } },
        { "name": "app_skill", "description": "One of the app's skills (a playbook for a job of its trade); without a name, the list of its skills.", "input_schema": { "type": "object", "properties": { "app": app, "name": { "type": "string" } }, "required": ["app"] } },
        { "name": "app_tools", "description": "The app's commands (name, description, parameters), filtered by a word when given.", "input_schema": { "type": "object", "properties": { "app": app, "filter": { "type": "string" } }, "required": ["app"] } },
        { "name": "app_call", "description": "Runs one of the app's commands (a tool name from app_tools) with its arguments.", "input_schema": { "type": "object", "properties": { "app": app, "tool": { "type": "string" }, "arguments": { "type": "object" } }, "required": ["app", "tool"] } },
        { "name": "app_look", "description": "The app's picture of the current work (an image you see) with its numbers.", "input_schema": { "type": "object", "properties": { "app": app, "arguments": { "type": "object" } }, "required": ["app"] } },
        { "name": "lsuite_call", "description": "One of the lsuite app's own commands: apps.list, apps.install, apps.open, cloud.*, market.*. Arguments as JSON.", "input_schema": { "type": "object", "properties": { "command": { "type": "string" }, "arguments": { "type": "object" } }, "required": ["command"] } }
    ])
}

/// The tools of an app's MCP server, cached per run.
async fn tools_of(c: &mut McpClient) -> CmdResult<Vec<Value>> {
    let r = c.request("tools/list", json!({})).await?;
    Ok(r["tools"].as_array().cloned().unwrap_or_default())
}

/// Calls an MCP tool; returns content blocks for the Messages API (text and images).
async fn call_tool(c: &mut McpClient, name: &str, args: Value) -> CmdResult<(Vec<Value>, bool)> {
    let r = c.request("tools/call", json!({ "name": name, "arguments": args })).await?;
    let mut blocks = vec![];
    for item in r["content"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("text") => blocks.push(json!({ "type": "text", "text": short_keep(item["text"].as_str().unwrap_or(""), 24_000) })),
            Some("image") => blocks.push(json!({ "type": "image", "source": { "type": "base64", "media_type": item["mimeType"].as_str().unwrap_or("image/png"), "data": item["data"] } })),
            _ => {}
        }
    }
    if blocks.is_empty() {
        blocks.push(json!({ "type": "text", "text": "(no output)" }));
    }
    Ok((blocks, r["isError"] == true))
}

fn short_keep(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { format!("{}… [cut: {} more bytes]", &s[..s.floor_char_boundary(n)], s.len() - n) }
}

/// Finds a tool by any of its usual names (`harness.brief`, `harness_brief`).
fn find_tool<'a>(tools: &'a [Value], names: &[&str]) -> Option<&'a str> {
    tools.iter().filter_map(|t| t["name"].as_str()).find(|n| names.iter().any(|w| n.eq_ignore_ascii_case(w) || n.replace('.', "_").eq_ignore_ascii_case(&w.replace('.', "_"))))
}

async fn run_messages(l: &Arc<Launcher>, prompt: &str, provider: &str) -> CmdResult<String> {
    let (url, key, model) = match provider {
        "lsuite" => {
            let acc = account::read().ok_or("Sign in with lsuite Pass to use lsuite AI.")?;
            let me = account::me(&acc.server, &acc.token).await?;
            let model = me["defaultModel"].as_str().map(str::to_string).ok_or("Your plan has no lsuite AI models: choose a Pass plan.")?;
            (format!("{}/api/ai/v1/messages", acc.server), acc.token, model)
        }
        "anthropic" => {
            let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| "Set ANTHROPIC_API_KEY to use the Anthropic API.")?;
            ("https://api.anthropic.com/v1/messages".to_string(), key, std::env::var("LSUITE_AGENT_MODEL").unwrap_or_else(|_| "claude-sonnet-5-5".into()))
        }
        other => return Err(format!("There's no agent provider called {other}.")),
    };
    // The installed apps' MCP servers, started as needed.
    let servers: BTreeMap<String, (PathBuf, Vec<String>)> = app_servers().into_iter().map(|(a, p, args)| (a, (p, args))).collect();
    let mut clients: BTreeMap<String, McpClient> = BTreeMap::new();
    let mut tool_cache: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut messages = l.agent.lock().messages.clone();
    messages.push(json!({ "role": "user", "content": prompt }));
    let system = format!("{BRIEF}\n\nInstalled apps with tools: {}.", servers.keys().cloned().collect::<Vec<_>>().join(", "));
    let http = reqwest::Client::builder().user_agent(util::USER_AGENT).timeout(Duration::from_secs(300)).build().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    for _ in 0..MAX_STEPS {
        let body = json!({ "model": model, "max_tokens": 8192, "system": system, "tools": tool_defs(), "messages": messages });
        let r = http.post(&url).header("x-api-key", &key).header("anthropic-version", "2023-06-01").json(&body).send().await.map_err(|e| format!("Couldn't reach the model ({e})."))?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if status != 200 {
            return Err(account::error_line(status, &v));
        }
        let content = v["content"].as_array().cloned().unwrap_or_default();
        messages.push(json!({ "role": "assistant", "content": content }));
        let mut results = vec![];
        for c in &content {
            match c["type"].as_str() {
                Some("text") => {
                    let t = c["text"].as_str().unwrap_or("").trim().to_string();
                    if !t.is_empty() {
                        answer = t.clone();
                        push(l, Entry::Assistant { text: t });
                    }
                }
                Some("tool_use") => {
                    let name = c["name"].as_str().unwrap_or("");
                    let input = c["input"].clone();
                    let app = input["app"].as_str().unwrap_or("").to_string();
                    let out = use_tool(l, name, &input, &servers, &mut clients, &mut tool_cache).await;
                    let (blocks, is_error) = match out {
                        Ok(x) => x,
                        Err(e) => (vec![json!({ "type": "text", "text": e })], true),
                    };
                    let text: String = blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(" ");
                    let image = blocks.iter().find(|b| b["type"] == "image").and_then(|b| save_image(b["source"]["data"].as_str().unwrap_or("")));
                    let shown = match name {
                        "app_call" | "app_look" => input["tool"].as_str().unwrap_or(name).to_string(),
                        "lsuite_call" => input["command"].as_str().unwrap_or(name).to_string(),
                        _ => name.to_string(),
                    };
                    push(l, Entry::Tool { app: if name == "lsuite_call" { "lsuite".into() } else { app }, tool: shown, input: short(&input.to_string(), 160), output: short(&text, 240), ok: !is_error, image });
                    results.push(json!({ "type": "tool_result", "tool_use_id": c["id"], "content": blocks, "is_error": is_error }));
                }
                _ => {}
            }
        }
        if results.is_empty() || v["stop_reason"] != "tool_use" {
            l.agent.lock().messages = messages;
            return Ok(answer);
        }
        messages.push(json!({ "role": "user", "content": results }));
    }
    l.agent.lock().messages = messages;
    Err(format!("The agent used its {MAX_STEPS} steps without finishing; ask it to go on."))
}

async fn use_tool(
    l: &Arc<Launcher>,
    name: &str,
    input: &Value,
    servers: &BTreeMap<String, (PathBuf, Vec<String>)>,
    clients: &mut BTreeMap<String, McpClient>,
    cache: &mut BTreeMap<String, Vec<Value>>,
) -> CmdResult<(Vec<Value>, bool)> {
    if name == "lsuite_call" {
        let cmd = input["command"].as_str().unwrap_or("");
        if cmd.starts_with("agent.") {
            return Err("The agent can't run itself.".into());
        }
        return match registry::call_boxed(l.clone(), registry::Source::Mcp, cmd.to_string(), input.get("arguments").cloned().unwrap_or(json!({}))).await {
            Ok(v) => Ok((vec![json!({ "type": "text", "text": short_keep(&v.to_string(), 24_000) })], false)),
            Err(e) => Ok((vec![json!({ "type": "text", "text": e })], true)),
        };
    }
    let app = input["app"].as_str().unwrap_or("").to_string();
    let Some((program, args)) = servers.get(&app) else {
        return Err(format!("{app} isn't installed (or hasn't been opened once). Install it with lsuite_call apps.install, open it once with apps.open, then try again."));
    };
    if !clients.contains_key(&app) {
        clients.insert(app.clone(), McpClient::start(program, args).await?);
    }
    let c = clients.get_mut(&app).expect("just started");
    if !cache.contains_key(&app) {
        let t = tools_of(c).await?;
        cache.insert(app.clone(), t);
    }
    let tools = cache.get(&app).cloned().unwrap_or_default();
    match name {
        "app_brief" => match find_tool(&tools, &["harness.brief"]) {
            Some(t) => call_tool(c, t, json!({})).await,
            None => {
                // Older apps: the MCP server's own instructions.
                let r = c.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "lsuite-agent", "version": crate::VERSION } })).await.unwrap_or(Value::Null);
                Ok((vec![json!({ "type": "text", "text": r["instructions"].as_str().unwrap_or("This app has no brief yet: use app_tools and read the descriptions.") })], false))
            }
        },
        "app_skill" => {
            let skill = input["name"].as_str().unwrap_or("");
            if skill.is_empty() {
                match find_tool(&tools, &["harness.skills"]) {
                    Some(t) => call_tool(c, t, json!({})).await,
                    None => {
                        let p = c.request("prompts/list", json!({})).await.unwrap_or(Value::Null);
                        Ok((vec![json!({ "type": "text", "text": p["prompts"].to_string() })], false))
                    }
                }
            } else {
                match find_tool(&tools, &["harness.skill"]) {
                    Some(t) => call_tool(c, t, json!({ "name": skill })).await,
                    None => {
                        let p = c.request("prompts/get", json!({ "name": skill })).await?;
                        Ok((vec![json!({ "type": "text", "text": p["messages"].to_string() })], false))
                    }
                }
            }
        }
        "app_tools" => {
            let f = input["filter"].as_str().unwrap_or("").to_ascii_lowercase();
            let list: Vec<Value> = tools
                .iter()
                .filter(|t| f.is_empty() || t["name"].as_str().unwrap_or("").to_ascii_lowercase().contains(&f) || t["description"].as_str().unwrap_or("").to_ascii_lowercase().contains(&f))
                .map(|t| if f.is_empty() { json!({ "name": t["name"], "description": short(t["description"].as_str().unwrap_or(""), 90) }) } else { json!({ "name": t["name"], "description": t["description"], "parameters": t["inputSchema"] }) })
                .collect();
            let note = if f.is_empty() { "Names and short descriptions; filter by a word to get full parameters." } else { "" };
            Ok((vec![json!({ "type": "text", "text": format!("{}{}", note, Value::Array(list)) })], false))
        }
        "app_call" => {
            let tool = input["tool"].as_str().unwrap_or("");
            let real = find_tool(&tools, &[tool]).ok_or_else(|| format!("{app} has no tool {tool}; look it up with app_tools."))?.to_string();
            call_tool(c, &real, input.get("arguments").cloned().unwrap_or(json!({}))).await
        }
        "app_look" => match find_tool(&tools, &["harness.look", "page.look", "project.renderFrame", "ui.screenshot"]) {
            Some(t) => {
                let t = t.to_string();
                call_tool(c, &t, input.get("arguments").cloned().unwrap_or(json!({}))).await
            }
            None => Err(format!("{app} can't show its work yet.")),
        },
        other => Err(format!("No tool {other}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_and_results() {
        assert_eq!(split_tool("mcp__kimchi__project_overview"), ("kimchi".into(), "project_overview".into()));
        assert_eq!(split_tool("Read"), ("".into(), "Read".into()));
        let tools = vec![json!({ "name": "harness_brief" }), json!({ "name": "page_look" })];
        assert_eq!(find_tool(&tools, &["harness.brief"]), Some("harness_brief"));
        assert_eq!(find_tool(&tools, &["harness.look", "page.look"]), Some("page_look"));
        assert_eq!(find_tool(&tools, &["nothing"]), None);
        let (text, image) = tool_result_parts(&json!([{ "type": "text", "text": "ok" }]));
        assert_eq!((text.as_str(), image), ("ok", None));
        assert_eq!(short("a\nb", 10), "a b");
        assert_eq!(short_keep("abcdef", 3), "abc… [cut: 3 more bytes]");
        let defs = tool_defs();
        assert_eq!(defs.as_array().unwrap().len(), 6);
    }
}
