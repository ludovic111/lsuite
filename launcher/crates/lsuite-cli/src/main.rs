//! `lsuite-cli`: every command of the lsuite launcher in a terminal, without the window.
//!
//! `lsuite-cli apps.install app=kimchi`, `lsuite-cli plugins.list app=ryolune`.
//! Parameters are `key=value`; values are read by the command's parameter types (`true`/`false`,
//! lists as JSON or comma-separated). The answer is JSON on stdout; progress goes to stderr.

use std::io::Write;

use lsuite_core::registry::{self, Kind};
use lsuite_core::{Event, Launcher, Source};
use serde_json::{Map, Value, json};

const USAGE: &str = "lsuite-cli — the lsuite launcher in a terminal

USAGE
  lsuite-cli <command> [key=value …]   run a command; the answer is JSON
  lsuite-cli help [command]            the commands, or one command's parameters
  lsuite-cli mcp-config                how to connect lsuite-mcp to Claude Code and other MCP clients
  lsuite-cli docs                      regenerate docs/COMMANDS.md (in the source tree)

EXAMPLES
  lsuite-cli apps.list refresh=true
  lsuite-cli apps.install app=folio
  lsuite-cli plugins.list
  lsuite-cli plugins.remove app=ryolune id=com.example.warm
  lsuite-cli agent.run prompt=\"Build a nori plugin: a halftone filter\"";

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first().cloned() else {
        println!("{USAGE}");
        return;
    };
    match first.as_str() {
        "-h" | "--help" => return println!("{USAGE}"),
        "help" => return help(args.get(1).map(String::as_str)),
        "-V" | "--version" => return println!("lsuite-cli {}", lsuite_core::VERSION),
        "docs" => {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/COMMANDS.md");
            match std::fs::write(path, registry::markdown()) {
                Ok(()) => println!("wrote {path}"),
                Err(e) => fail(&format!("couldn't write {path}: {e}")),
            }
            return;
        }
        "mcp-config" => return mcp_config(),
        _ => {}
    }
    let Some(spec) = registry::spec(&first) else { fail(&format!("there's no command {first:?}; `lsuite-cli help` lists them")) };
    let mut params = Map::new();
    for a in &args[1..] {
        let Some((k, v)) = a.split_once('=') else { fail(&format!("{a:?}: parameters are key=value")) };
        let kind = spec.params.iter().find(|p| p.name == k).map(|p| p.kind);
        params.insert(k.to_string(), parse(kind, v));
    }
    let l = Launcher::new();
    let mut events = l.hub.subscribe();
    // Progress on one line of stderr, cleared when the answer comes.
    let shown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let printing = shown.clone();
    let printer = tokio::spawn(async move {
        while let Ok(e) = events.recv().await {
            if let Event::Progress { label, done, total, .. } = e {
                let pct = total.filter(|t| *t > 0).map(|t| format!(" {:>3.0} %", done as f64 / t as f64 * 100.0)).unwrap_or_default();
                eprint!("\r\x1b[2K{label}{pct}");
                let _ = std::io::stderr().flush();
                printing.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    });
    let result = lsuite_core::call(&l, Source::Cli, spec.name, Value::Object(params)).await;
    printer.abort();
    if shown.load(std::sync::atomic::Ordering::Relaxed) {
        eprint!("\r\x1b[2K");
    }
    match result {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()),
        Err(e) => fail(&e),
    }
}

/// A parameter's value, read by its type.
fn parse(kind: Option<Kind>, v: &str) -> Value {
    match kind {
        Some(Kind::Bool) => match v {
            "true" | "yes" | "1" | "on" => json!(true),
            "false" | "no" | "0" | "off" => json!(false),
            _ => json!(v),
        },
        Some(Kind::List) => serde_json::from_str::<Value>(v).ok().filter(Value::is_array).unwrap_or_else(|| json!(v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>())),
        Some(Kind::Any) => serde_json::from_str(v).unwrap_or_else(|_| json!(v)),
        _ => json!(expand_home(v)),
    }
}

/// `~/x` → the home folder's x (shells don't expand it after `key=`).
fn expand_home(v: &str) -> String {
    match (v.strip_prefix("~/"), std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))) {
        (Some(rest), Some(home)) => std::path::Path::new(&home).join(rest).display().to_string(),
        _ => v.to_string(),
    }
}

fn help(command: Option<&str>) {
    if let Some(name) = command {
        let Some(s) = registry::spec(name) else { fail(&format!("there's no command {name:?}")) };
        println!("{}\n\n{}", s.name, s.summary);
        for p in s.params {
            println!("  {}{}  {}", p.name, if p.required { " (required)" } else { "" }, p.doc);
        }
        return;
    }
    println!("{USAGE}\n\nCOMMANDS");
    for s in registry::COMMANDS {
        let first = s.summary.split(". ").next().unwrap_or(s.summary).trim_end_matches('.');
        println!("  {:<22} {first}", s.name);
    }
}

fn mcp_config() {
    let mcp = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(if cfg!(windows) { "lsuite-mcp.exe" } else { "lsuite-mcp" }))).filter(|p| p.exists());
    let mcp = mcp.map(|p| p.display().to_string()).unwrap_or_else(|| "lsuite-mcp".into());
    println!("Claude Code:\n  claude mcp add lsuite -- {mcp}\n\nCodex (~/.codex/config.toml):\n  [mcp_servers.lsuite]\n  command = \"{mcp}\"\n\nAny MCP client (JSON):\n  {{ \"mcpServers\": {{ \"lsuite\": {{ \"command\": \"{mcp}\" }} }} }}");
}

fn fail(msg: &str) -> ! {
    eprintln!("lsuite-cli: {msg}");
    std::process::exit(1)
}
