//! `lsuite-mcp`: the lsuite launcher as a Model Context Protocol server over stdio
//! (newline-delimited JSON-RPC 2.0). Every registry command is a tool (`apps.install` becomes
//! `apps_install`) with its description and schema from the registry. Calls are held to the
//! launcher's `settings.agent` permissions (removing apps, deleting cloud files and signing in
//! or out are off until the person turns them on). Only protocol goes to stdout.
//!
//! The launcher keeps its state on disk, so the server works the same whether the window is
//! open or not: `claude mcp add lsuite -- /path/to/lsuite-mcp`.

use std::sync::Arc;

use lsuite_core::{Launcher, Source, registry};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

const PROTOCOLS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

const INSTRUCTIONS: &str = "The lsuite launcher: installs, updates, opens and removes the lsuite apps (ryolune music, kimchi video, zenith code, nori image and design, folio office) from their signed releases; shows the lsuite account and its lsuite Pass; manages lsuite Cloud, the storage that comes with the Pass (upload, download, folders, synced folders); and the lsuite Marketplace of plugins made by people who use lsuite (browse, install, publish what you build). Start with apps_list, cloud_list or market_list. Each installed app has its own MCP server (apps_list gives its `mcp` path) to drive it.";

#[tokio::main]
async fn main() {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("lsuite-mcp {}", lsuite_core::VERSION);
        return;
    }
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!("lsuite-mcp — the lsuite launcher as an MCP server (stdio)\n\nRegister it with an MCP client, for example Claude Code:\n  claude mcp add lsuite -- /path/to/lsuite-mcp");
        return;
    }
    let l = Launcher::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(v) = rx.recv().await {
            let mut line = serde_json::to_vec(&v).unwrap_or_default();
            line.push(b'\n');
            if out.write_all(&line).await.is_err() || out.flush().await.is_err() {
                break;
            }
        }
    });
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    let mut tasks = vec![];
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                let _ = tx.send(error(Value::Null, -32700, "Parse error"));
                continue;
            }
        };
        let (l, tx) = (l.clone(), tx.clone());
        tasks.push(tokio::spawn(async move {
            let id = msg.get("id").cloned();
            let method = msg["method"].as_str().unwrap_or("").to_string();
            let reply = dispatch(&l, &method, &msg["params"]).await;
            if let Some(id) = id {
                let _ = tx.send(match reply {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err((code, message)) => error(id, code, &message),
                });
            }
        }));
        tasks.retain(|t| !t.is_finished());
    }
    for t in tasks {
        let _ = t.await;
    }
    drop(tx);
    let _ = writer.await;
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_name(command: &str) -> String {
    command.replace('.', "_")
}

async fn dispatch(l: &Arc<Launcher>, method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOLS.iter().find(|p| **p == asked).copied().unwrap_or(PROTOCOLS[PROTOCOLS.len() - 1]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "lsuite", "title": "lsuite launcher", "version": lsuite_core::VERSION },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => {
            let tools: Vec<Value> = registry::COMMANDS
                .iter()
                .map(|s| {
                    let read_only = s.perm == registry::Perm::Read;
                    json!({
                        "name": tool_name(s.name),
                        "title": s.name,
                        "description": s.summary,
                        "inputSchema": registry::input_schema(s),
                        "annotations": { "readOnlyHint": read_only, "destructiveHint": matches!(s.perm, registry::Perm::Remove | registry::Perm::CloudDelete) },
                    })
                })
                .collect();
            Ok(json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let Some(spec) = registry::COMMANDS.iter().find(|s| tool_name(s.name) == name || s.name == name) else {
                return Err((-32602, format!("Unknown tool {name:?}")));
            };
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(match lsuite_core::call(l, Source::Mcp, spec.name, args).await {
                Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }], "structuredContent": if v.is_object() { v.clone() } else { json!({ "result": v }) }, "isError": false }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            })
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        m if m.starts_with("notifications/") => Ok(Value::Null),
        _ => Err((-32601, format!("Method not found: {method}"))),
    }
}
