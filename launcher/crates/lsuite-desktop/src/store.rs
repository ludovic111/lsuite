//! The window's state.
//!
//! The window is one client of the command registry, like `lsuite-cli` and `lsuite-mcp`: every
//! action goes through [`Store::run`], and what is shown is what the commands answer. The store
//! keeps the last answers (apps, account, cloud), the view state (page, cloud folder, dialog),
//! and mirrors the core's event stream: progress of downloads, installs and transfers, and which
//! part changed.

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
    Marketplace,
    Cloud,
    Account,
    Settings,
}

impl Page {
    pub fn title(self) -> &'static str {
        match self {
            Page::Agent => "Agent",
            Page::Apps => "Apps",
            Page::Marketplace => "Marketplace",
            Page::Cloud => "Cloud",
            Page::Account => "Account",
            Page::Settings => "Settings",
        }
    }
}

/// Dialogs over the window (one at a time).
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    RemoveApp { app: String, name: String },
    DeleteCloud { path: String, folder: bool },
    NewFolder,
    Rename { path: String },
    MoveTo { path: String },
    /// No system file picker: type the path to upload instead.
    UploadPath,
    /// No system file picker: type the path of the folder to sync.
    SyncPath,
    /// No system file picker: type the path of a plugin bundle to publish.
    PublishPath,
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
    /// `account.status`; `Null` until the first answer.
    pub account: Value,
    pub plans: Value,
    /// Every cloud file and folder (`cloud.list all=true`), when signed in.
    pub cloud: Option<Value>,
    pub cloud_status: Value,
    pub cloud_error: Option<String>,
    pub cloud_loading: bool,
    /// The folder shown (`""`: the root).
    pub cloud_dir: String,
    pub cloud_selected: Option<String>,
    pub settings: Settings,
    pub tasks: BTreeMap<String, TaskView>,
    pub toasts: Vec<Toast>,
    pub dialog: Option<Dialog>,
    /// The app whose "more" menu is open, and where.
    pub menu: Option<(String, gpui::Point<gpui::Pixels>)>,
    pub signing_in: Option<String>,
    pub checking: bool,
    /// The launcher's own update (`app.checkUpdates`).
    pub update: Value,
    /// `cloud.syncList`.
    pub syncs: Value,
    /// `market.list` (all apps), `market.mine`, and the app the list is narrowed to.
    pub market: Value,
    pub market_mine: Value,
    pub market_app: Option<String>,
    pub market_error: Option<String>,
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
            account: Value::Null,
            plans: Value::Null,
            cloud: None,
            cloud_status: Value::Null,
            cloud_error: None,
            cloud_loading: false,
            cloud_dir: String::new(),
            cloud_selected: None,
            settings: lsuite_core::settings::load(),
            tasks: BTreeMap::new(),
            toasts: vec![],
            dialog: None,
            menu: None,
            signing_in: None,
            checking: false,
            update: Value::Null,
            syncs: Value::Null,
            market: Value::Null,
            market_mine: Value::Null,
            market_app: None,
            market_error: None,
            agent: Value::Null,
            agent_providers: Value::Null,
            agent_provider: None,
            next_toast: 0,
        };
        s.syncs = lsuite_core::sync::list(&s.launcher);
        s.pump(cx);
        s.refresh_account(cx);
        s.refresh_plans(cx);
        // Synced folders, every `syncEveryMinutes` while signed in.
        cx.spawn(async move |this, cx| {
            let mut waited = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(60)).await;
                waited += 1;
                let Ok(go) = this.update(cx, |s, _| {
                    let every = s.settings.sync_every_minutes;
                    every > 0 && waited >= every && s.signed_in() && s.syncs["pairs"].as_array().is_some_and(|p| !p.is_empty())
                }) else {
                    break;
                };
                if go {
                    waited = 0;
                    let _ = this.update(cx, |s, cx| s.run_result("cloud.syncNow", json!({}), cx, |_, _, _| {}));
                }
            }
        })
        .detach();
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
                if task == "signIn" {
                    self.signing_in = Some(label);
                } else if !task.is_empty() {
                    self.tasks.insert(task, TaskView { label, done, total });
                }
            }
            Event::TaskDone { task, ok, message } => {
                self.tasks.remove(&task);
                if task == "signIn" {
                    self.signing_in = None;
                    if ok {
                        self.toast(ToastKind::Success, "Signed in. Every lsuite app on this computer is signed in too.", cx);
                    }
                }
                if !message.is_empty() && task != "signIn" {
                    self.toast(if ok { ToastKind::Success } else { ToastKind::Error }, message, cx);
                }
            }
            Event::Changed { what } => match what.as_str() {
                "apps" => self.refresh_apps(cx),
                "account" => self.refresh_account(cx),
                "cloud" => self.refresh_cloud(cx),
                "sync" => self.syncs = lsuite_core::sync::list(&self.launcher),
                "market" => self.refresh_market(cx),
                "agent" => self.agent = lsuite_core::agent::log(&self.launcher),
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

    pub fn refresh_account(&mut self, cx: &mut Context<Self>) {
        self.run_then("account.status", json!({}), cx, |s, v, cx| {
            let was = s.account["signedIn"].as_bool();
            s.account = v;
            if s.account["signedIn"] == true {
                if was != Some(true) || s.cloud.is_none() {
                    s.refresh_cloud(cx);
                }
            } else {
                s.cloud = None;
                s.cloud_status = Value::Null;
            }
        });
    }

    /// The marketplace's listings and, signed in, the account's own submissions.
    pub fn refresh_market(&mut self, cx: &mut Context<Self>) {
        self.run_result("market.list", json!({}), cx, |s, r, _| match r {
            Ok(v) => {
                s.market = v;
                s.market_error = None;
            }
            Err(e) => s.market_error = Some(e),
        });
        if self.signed_in() {
            self.run_result("market.mine", json!({}), cx, |s, r, _| s.market_mine = r.unwrap_or(Value::Null));
        }
    }

    /// Whether the account has a paid Pass plan (the marketplace and lsuite Cloud come with it).
    pub fn has_pass(&self) -> bool {
        self.signed_in() && self.account["plan"].as_str().is_some_and(|p| p != "free")
    }

    pub fn refresh_plans(&mut self, cx: &mut Context<Self>) {
        self.run_then("account.plans", json!({}), cx, |s, v, _| s.plans = v);
    }

    pub fn signed_in(&self) -> bool {
        self.account["signedIn"] == true
    }

    pub fn refresh_cloud(&mut self, cx: &mut Context<Self>) {
        if !self.signed_in() {
            return;
        }
        self.cloud_loading = true;
        let l = self.launcher.clone();
        let task = gpui_tokio::Tokio::spawn(cx, async move {
            let all = lsuite_core::call(&l, Source::Window, "cloud.list", json!({ "all": true })).await;
            let status = lsuite_core::call(&l, Source::Window, "cloud.status", json!({})).await;
            (all, status)
        });
        cx.spawn(async move |this, cx| {
            let Ok((all, status)) = task.await else { return };
            this.update(cx, |s, cx| {
                s.cloud_loading = false;
                match all {
                    Ok(v) => {
                        s.cloud = Some(v);
                        s.cloud_error = None;
                    }
                    Err(e) => s.cloud_error = Some(e),
                }
                if let Ok(v) = status {
                    s.cloud_status = v;
                }
                // The folder shown may be gone (deleted, moved by another client).
                if !s.cloud_dir.is_empty() && !s.folder_exists(&s.cloud_dir.clone()) {
                    s.cloud_dir = lsuite_core::cloud::parent_of(&s.cloud_dir).to_string();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn folder_exists(&self, dir: &str) -> bool {
        let Some(all) = &self.cloud else { return false };
        let prefix = format!("{dir}/");
        all["files"].as_array().into_iter().flatten().any(|f| f["path"].as_str().is_some_and(|p| p.starts_with(&prefix)))
            || all["folders"].as_array().into_iter().flatten().any(|f| f["path"].as_str().is_some_and(|p| p == dir || p.starts_with(&prefix)))
    }

    /// The folder shown: its subfolders and files.
    pub fn cloud_view(&self) -> Option<Value> {
        self.cloud.as_ref().map(|all| lsuite_core::cloud::folder_view(all, &self.cloud_dir))
    }

    pub fn open_folder(&mut self, dir: impl Into<String>, cx: &mut Context<Self>) {
        self.cloud_dir = dir.into();
        self.cloud_selected = None;
        cx.notify();
    }

    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.menu = None;
        match page {
            Page::Account => {
                self.refresh_account(cx);
                if self.plans.is_null() {
                    self.refresh_plans(cx);
                }
            }
            Page::Cloud => self.refresh_cloud(cx),
            Page::Marketplace => self.refresh_market(cx),
            Page::Agent => {
                self.agent = lsuite_core::agent::log(&self.launcher);
                self.agent_providers = lsuite_core::agent::providers(&self.launcher);
            }
            _ => {}
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
