//! The window's state.
//!
//! The window is one client of the command registry, like `lsuite-cli` and `lsuite-mcp`: every
//! action goes through [`Store::run`], and what is shown is what the commands answer. The store
//! keeps the last answers (apps, plugins, the agent), the view state (page, dialog), and mirrors
//! the core's event stream: progress of downloads and installs, and which part changed.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Context, Entity, Global, SharedString};
use lsuite_core::settings::Settings;
use lsuite_core::{Event, Launcher, Source};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Agent,
    Apps,
    Plugins,
    Settings,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Page::Agent => "Agent",
            Page::Apps => "Apps",
            Page::Plugins => "Plugins",
            Page::Settings => "Settings",
        }
    }
}

/// Dialogs over the window (one at a time).
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    RemoveApp { app: String, name: String },
    RemovePlugin { app: String, id: String, name: String },
    /// What the plugin should do, for the lsuite agent to build it.
    BuildPlugin { app: String, name: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub text: SharedString,
}

/// A task in progress, as the core reports it.
#[derive(Clone, Debug)]
pub struct TaskView {
    pub label: String,
    pub done: u64,
    pub total: Option<u64>,
}

impl TaskView {
    pub fn fraction(&self) -> Option<f64> {
        self.total.filter(|t| *t > 0).map(|t| self.done as f64 / t as f64)
    }
}

pub struct Store {
    pub launcher: Arc<Launcher>,
    pub page: Page,
    /// `apps.list`.
    pub apps: Value,
    /// `plugins.list`; `Null` until the first answer.
    pub plugins: Value,
    pub settings: Settings,
    pub tasks: BTreeMap<String, TaskView>,
    pub toasts: Vec<Toast>,
    pub dialog: Option<Dialog>,
    /// The app whose "more" menu is open, and where.
    pub menu: Option<(String, gpui::Point<gpui::Pixels>)>,
    pub checking: bool,
    /// The launcher's own update (`app.checkUpdates`).
    pub update: Value,
    /// The lsuite agent: `agent.log`, `agent.providers` and the provider chosen.
    pub agent: Value,
    pub agent_providers: Value,
    pub agent_provider: Option<String>,
    next_toast: u64,
}

pub struct GlobalStore(pub Entity<Store>);
impl Global for GlobalStore {}

pub trait StoreExt {
    fn store(&self) -> Entity<Store>;
}

impl StoreExt for App {
    fn store(&self) -> Entity<Store> {
        self.global::<GlobalStore>().0.clone()
    }
}

impl Store {
    pub fn new(launcher: Arc<Launcher>, cx: &mut Context<Self>) -> Self {
        let apps = lsuite_core::apps::list(&launcher);
        let mut s = Store {
            launcher,
            page: Page::Apps,
            apps,
            plugins: Value::Null,
            settings: lsuite_core::settings::load(),
            tasks: BTreeMap::new(),
            toasts: vec![],
            dialog: None,
            menu: None,
            checking: false,
            update: Value::Null,
            agent: Value::Null,
            agent_providers: Value::Null,
            agent_provider: None,
            next_toast: 0,
        };
        s.pump(cx);
        s.refresh_plugins(cx);
        // Apps can open, close or be installed by another client: look again every few seconds.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                if this.update(cx, |s, cx| s.refresh_apps(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        s
    }

    /// Mirrors the core's events into the window.
    fn pump(&mut self, cx: &mut Context<Self>) {
        let mut rx = self.launcher.hub.subscribe();
        cx.spawn(async move |this, cx| {
            loop {
                let e = match rx.recv().await {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                if this.update(cx, |s, cx| s.on_event(e, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn on_event(&mut self, e: Event, cx: &mut Context<Self>) {
        match e {
            Event::Progress { task, label, done, total } => {
                if !task.is_empty() {
                    self.tasks.insert(task, TaskView { label, done, total });
                }
            }
            Event::TaskDone { task, ok, message } => {
                self.tasks.remove(&task);
                if !message.is_empty() {
                    self.toast(if ok { ToastKind::Success } else { ToastKind::Error }, message, cx);
                }
            }
            Event::Changed { what } => match what.as_str() {
                "apps" => self.refresh_apps(cx),
                "plugins" => self.refresh_plugins(cx),
                "agent" => {
                    self.agent = lsuite_core::agent::log(&self.launcher);
                    // A plugin the agent builds shows up as it is installed.
                    if self.page == Page::Plugins {
                        self.refresh_plugins(cx);
                    }
                }
                "update" => self.update = lsuite_core::selfupdate::to_value(&lsuite_core::selfupdate::status(&self.launcher)),
                "settings" => {
                    self.settings = lsuite_core::settings::load();
                    crate::app::apply_theme(&self.settings, cx);
                }
                _ => {}
            },
        }
        cx.notify();
    }

    /// Runs a command in the background and hands its result to `then`.
    pub fn run_result(&mut self, name: &str, params: Value, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, lsuite_core::CmdResult<Value>, &mut Context<Self>) + 'static) {
        let l = self.launcher.clone();
        let name = name.to_string();
        let task = gpui_tokio::Tokio::spawn(cx, async move { lsuite_core::call(&l, Source::Window, &name, params).await });
        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_else(|e| Err(format!("The command stopped ({e}).")));
            this.update(cx, |s, cx| {
                then(s, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs a command in the background; its answer goes to `then`, errors become toasts.
    pub fn run_then(&mut self, name: &str, params: Value, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, Value, &mut Context<Self>) + 'static) {
        self.run_result(name, params, cx, |s, r, cx| match r {
            Ok(v) => then(s, v, cx),
            Err(e) => s.toast(ToastKind::Error, e, cx),
        });
    }

    pub fn run(&mut self, name: &str, params: Value, cx: &mut Context<Self>) {
        self.run_then(name, params, cx, |_, _, _| {});
    }

    pub fn refresh_apps(&mut self, cx: &mut Context<Self>) {
        let l = self.launcher.clone();
        let task = gpui_tokio::Tokio::spawn(cx, async move { lsuite_core::apps::list(&l) });
        cx.spawn(async move |this, cx| {
            if let Ok(v) = task.await {
                this.update(cx, |s, cx| {
                    if s.apps != v {
                        s.apps = v;
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// Looks for new versions of every app now.
    pub fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.checking {
            return;
        }
        self.checking = true;
        cx.notify();
        self.run_result("apps.check", json!({}), cx, |s, r, cx| {
            s.checking = false;
            let v = match r {
                Ok(v) => v,
                Err(e) => return s.toast(ToastKind::Error, e, cx),
            };
            s.apps = v;
            let n = s.apps["updates"].as_u64().unwrap_or(0);
            let errors: Vec<String> = s.apps["apps"].as_array().into_iter().flatten().filter_map(|a| a["latestError"].as_str().map(str::to_string)).collect();
            if let Some(e) = errors.first() {
                s.toast(ToastKind::Error, e.clone(), cx);
            } else {
                s.toast(ToastKind::Info, if n == 0 { "Every installed app is up to date.".to_string() } else { format!("{n} update{} ready.", if n == 1 { "" } else { "s" }) }, cx);
            }
        });
    }

    /// The plugins installed for each app.
    pub fn refresh_plugins(&mut self, cx: &mut Context<Self>) {
        self.run_then("plugins.list", json!({}), cx, |s, v, _| s.plugins = v);
    }

    /// Starts the lsuite agent on `prompt`, with the provider chosen (its errors go in its log).
    pub fn ask_agent(&mut self, prompt: String, cx: &mut Context<Self>) {
        let provider = self.agent_provider.clone();
        self.run_result("agent.run", json!({ "prompt": prompt, "provider": provider }), cx, |s, _, _| s.agent = lsuite_core::agent::log(&s.launcher));
        self.agent = lsuite_core::agent::log(&self.launcher);
    }

    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.menu = None;
        if page == Page::Plugins {
            self.refresh_plugins(cx);
        }
        // The Plugins page builds through the agent too.
        if matches!(page, Page::Agent | Page::Plugins) {
            self.agent = lsuite_core::agent::log(&self.launcher);
            self.agent_providers = lsuite_core::agent::providers();
        }
        cx.notify();
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.next_toast += 1;
        let id = self.next_toast;
        self.toasts.push(Toast { id, kind, text: text.into() });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        let wait = if kind == ToastKind::Error { 9 } else { 4 };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(wait)).await;
            this.update(cx, |s, cx| {
                s.toasts.retain(|t| t.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|t| t.id != id);
        cx.notify();
    }

    pub fn open_dialog(&mut self, d: Dialog, cx: &mut Context<Self>) {
        self.dialog = Some(d);
        self.menu = None;
        cx.notify();
    }

    pub fn close_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        cx.notify();
    }

    pub fn set_setting(&mut self, key: &str, value: Value, cx: &mut Context<Self>) {
        self.run_then("settings.set", json!({ "key": key, "value": value }), cx, |s, v, cx| {
            if let Ok(set) = serde_json::from_value(v) {
                s.settings = set;
            }
            crate::app::apply_theme(&s.settings, cx);
        });
    }

    /// The app's entry in `apps.list`.
    pub fn app(&self, id: &str) -> Value {
        self.apps["apps"].as_array().into_iter().flatten().find(|a| a["id"] == id).cloned().unwrap_or(Value::Null)
    }
}
