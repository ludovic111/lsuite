//! The root view: the lsuite backdrop, the sidebar (Apps, Cloud, Account, Settings), the page
//! shown, and what floats above (dialogs, the app menu, toasts).

use std::sync::Arc;

use gpui::{AnyElement, App, AppContext as _, Context, Entity, FocusHandle, FontWeight, KeyBinding, MouseButton, Render, Subscription, Window, actions, div, img, prelude::*, px};
use lsuite_core::Launcher;
use lsuite_core::settings::Settings;
use serde_json::json;

use crate::store::{Dialog, GlobalStore, Page, Store, StoreExt, ToastKind};
use crate::theme::{ActiveTheme, MONO, Theme, os_reduces_transparency, size as sz};
use crate::ui::input::{InputEvent, TextInput};
use crate::ui::{GlassExt, caps, icon, tag, window_controls};
use crate::views;

actions!(lsuite, [Quit, Escape, Confirm, ShowApps, ShowCloud, ShowAccount, ShowSettings, CheckUpdates]);

pub fn init(launcher: Arc<Launcher>, cx: &mut App) {
    let settings = lsuite_core::settings::load();
    cx.set_global(theme_for(&settings, cx));
    cx.set_reduce_motion(crate::theme::os_reduces_motion());
    let store = cx.new(|cx| Store::new(launcher, cx));
    cx.set_global(GlobalStore(store));
    let m = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    let mut keys = crate::ui::input::bindings();
    keys.extend([
        KeyBinding::new(&format!("{m}-q"), Quit, None),
        KeyBinding::new("escape", Escape, Some("Workspace")),
        KeyBinding::new("enter", Confirm, Some("Workspace")),
        KeyBinding::new(&format!("{m}-1"), ShowApps, None),
        KeyBinding::new(&format!("{m}-2"), ShowCloud, None),
        KeyBinding::new(&format!("{m}-3"), ShowAccount, None),
        KeyBinding::new(&format!("{m}-,"), ShowSettings, None),
        KeyBinding::new(&format!("{m}-r"), CheckUpdates, None),
    ]);
    cx.bind_keys(keys);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

/// Once the window is up: look for new versions (unless turned off), then update by itself
/// when asked to.
pub fn start(cx: &mut App) {
    let store = cx.store();
    store.update(cx, |s, cx| {
        let off = std::env::var("LSUITE_NO_UPDATE").is_ok_and(|v| !v.is_empty() && v != "0");
        if s.settings.check_on_start && !off {
            s.run_result("app.checkUpdates", json!({}), cx, |s, r, _| {
                if let Ok(v) = r {
                    s.update = v;
                }
            });
        }
        // Synced folders catch up once at start.
        if s.syncs["pairs"].as_array().is_some_and(|p| !p.is_empty()) {
            s.run_result("cloud.syncNow", json!({}), cx, |_, _, _| {});
        }
        if s.settings.check_on_start && !off && lsuite_core::apps::stale(&s.launcher) {
            s.run_then("apps.check", json!({}), cx, |s, v, cx| {
                s.apps = v;
                if s.settings.auto_update && s.apps["updates"].as_u64().unwrap_or(0) > 0 {
                    s.run("apps.updateAll", json!({}), cx);
                }
            });
        } else if s.settings.auto_update && s.apps["updates"].as_u64().unwrap_or(0) > 0 && !off {
            s.run("apps.updateAll", json!({}), cx);
        }
    });
}

fn theme_for(settings: &Settings, cx: &App) -> Theme {
    let mode = Theme::mode_for(&settings.theme, cx.window_appearance());
    Theme::new(mode, !settings.reduce_transparency && !os_reduces_transparency())
}

/// Puts the theme setting (and the OS appearance) into the theme.
pub fn apply_theme(settings: &Settings, cx: &mut App) {
    let next = theme_for(settings, cx);
    let t = cx.global::<Theme>();
    if t.mode != next.mode || t.transparent != next.transparent {
        cx.set_global(next);
        cx.refresh_windows();
    }
}

pub struct Workspace {
    store: Entity<Store>,
    focus: FocusHandle,
    /// The dialogs' text field (folder names, paths).
    pub input: Entity<TextInput>,
    /// The account page's key field.
    pub key_input: Entity<TextInput>,
    last_dialog: Option<Dialog>,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = cx.store();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let input = cx.new(TextInput::new);
        let key_input = cx.new(|cx| TextInput::new(cx).placeholder("lsk_…"));
        key_input.update(cx, |i, _| i.mono = true);
        let mut subs = vec![cx.observe_in(&store, window, |ws: &mut Self, _, window, cx| ws.store_changed(window, cx))];
        subs.push(cx.observe_window_appearance(window, |ws, _, cx| {
            let settings = ws.store.read(cx).settings.clone();
            apply_theme(&settings, cx);
        }));
        subs.push(cx.subscribe_in(&input, window, |ws: &mut Self, _, e: &InputEvent, window, cx| match e {
            InputEvent::Submit => views::overlays::confirm(ws, window, cx),
            InputEvent::Cancel => ws.close_dialog(window, cx),
            _ => {}
        }));
        subs.push(cx.subscribe_in(&key_input, window, |ws: &mut Self, _, e: &InputEvent, _, cx| {
            if let InputEvent::Submit = e {
                ws.sign_in_with_key(cx);
            }
        }));
        Self { store, focus, input, key_input, last_dialog: None, _subs: subs }
    }

    /// A dialog that just opened gets its field filled and focused.
    fn store_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = self.store.read(cx).dialog.clone();
        if dialog != self.last_dialog {
            self.last_dialog = dialog.clone();
            let dir = self.store.read(cx).cloud_dir.clone();
            let (text, placeholder) = match &dialog {
                Some(Dialog::NewFolder) => (String::new(), "Folder name"),
                Some(Dialog::Rename { path }) => (lsuite_core::cloud::name_of(path).to_string(), "New name"),
                Some(Dialog::MoveTo { path }) => (lsuite_core::cloud::parent_of(path).to_string(), "Folder (empty: the top of your cloud)"),
                Some(Dialog::UploadPath) => (String::new(), if cfg!(windows) { r"C:\Users\you\Documents\file.folio" } else { "/home/you/Documents/file.folio" }),
                Some(Dialog::SyncPath) => (String::new(), if cfg!(windows) { r"C:\Users\you\Documents\Projects" } else { "/home/you/Documents/Projects" }),
                _ => (String::new(), ""),
            };
            let _ = dir;
            self.input.update(cx, |i, cx| {
                i.set_text(text, cx);
                i.set_placeholder(placeholder, cx);
                i.select_all_text(cx);
            });
            match dialog {
                Some(Dialog::NewFolder | Dialog::Rename { .. } | Dialog::MoveTo { .. } | Dialog::UploadPath | Dialog::SyncPath) => crate::ui::input::focus(&self.input, window, cx),
                _ => window.focus(&self.focus, cx),
            }
        }
        cx.notify();
    }

    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.store.update(cx, |s, cx| s.close_dialog(cx));
        window.focus(&self.focus, cx);
    }

    pub fn sign_in_with_key(&mut self, cx: &mut Context<Self>) {
        let key = self.key_input.read(cx).text().trim().to_string();
        if key.is_empty() {
            return;
        }
        self.key_input.update(cx, |i, cx| i.set_text("", cx));
        self.store.update(cx, |s, cx| {
            s.run_then("account.signIn", json!({ "key": key }), cx, |s, v, cx| {
                s.toast(ToastKind::Success, format!("Signed in as {}. Every lsuite app on this computer is signed in too.", v["email"].as_str().unwrap_or("you")), cx);
            })
        });
    }

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let page = s.page;
        let mac = cfg!(target_os = "macos") && !window.is_fullscreen();
        let updates = s.apps["updates"].as_u64().unwrap_or(0);
        let signed_in = s.signed_in();
        let plan = s.account["planName"].as_str().map(str::to_string);
        let cloud_pct = {
            let (u, q) = (s.cloud_status["used"].as_f64().unwrap_or(0.), s.cloud_status["quota"].as_f64().unwrap_or(0.));
            (q > 0.).then(|| format!("{:.0} %", u / q * 100.))
        };
        let nav = |id: &'static str, ic: &'static str, p: Page, badge: Option<AnyElement>| {
            let on = p == page;
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(34.))
                .px(px(10.))
                .cursor_pointer()
                .when(on, |d| d.bg(t.accent).text_color(t.text_on_accent).font_weight(FontWeight::SEMIBOLD))
                .when(!on, |d| d.text_color(t.text_2).hover(|d| d.bg(t.hover).text_color(t.text)))
                .child(icon(ic))
                .child(div().flex_1().child(p.title()))
                .children(badge)
                .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.go(p, cx)))
        };
        let tasks: Vec<(String, crate::store::TaskView)> = s.tasks.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let account = s.account.clone();
        div()
            .w(px(236.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .glass(t.glass1)
            .border_0()
            .border_r_1()
            .border_color(t.line)
            .child(
                div()
                    .id("side-bar")
                    .h(px(52.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(if mac { 84. } else { 16. }))
                    .window_control_area(gpui::WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |e, window, _| {
                        if e.click_count == 2 {
                            window.titlebar_double_click();
                        } else {
                            window.start_window_move();
                        }
                    })
                    .when(!mac, |d| d.child(icon("mark").size(px(20.))))
                    .child(div().font_weight(FontWeight::BOLD).text_size(px(sz::LG)).child("lsuite"))
                    .child(tag("beta", false, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(10.))
                    .pt(px(6.))
                    .child(nav("nav-apps", "layout-grid", Page::Apps, (updates > 0).then(|| tag(format!("{updates}"), page != Page::Apps, cx).into_any_element())))
                    .child(nav("nav-cloud", "cloud", Page::Cloud, cloud_pct.map(|p| div().font_family(MONO).text_size(px(sz::XS)).child(p).into_any_element())))
                    .child(nav("nav-account", "user", Page::Account, plan.map(|p| div().font_family(MONO).text_size(px(sz::XS)).child(p.to_uppercase()).into_any_element())))
                    .child(nav("nav-settings", "settings", Page::Settings, None)),
            )
            .child(div().flex_1())
            .when_some(update_banner(&self.store.read(cx).update.clone(), cx), |d, b| d.child(b))
            .when(!tasks.is_empty(), |d| {
                d.child(
                    div().flex().flex_col().gap(px(8.)).px(px(14.)).pb(px(12.)).child(caps("Activity", cx)).children(tasks.into_iter().map(|(k, task)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(div().text_size(px(sz::SM)).truncate().child(task.label.clone()))
                            .child(match task.fraction() {
                                Some(f) => crate::ui::meter(f, 2., cx).into_any_element(),
                                None => crate::ui::busy_bar(gpui::ElementId::Name(format!("busy-{k}").into()), cx),
                            })
                    })),
                )
            })
            .child(views::account::chip(signed_in, &account, cx))
            .into_any_element()
    }

    fn top_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let page = self.store.read(cx).page;
        let controls = window_controls(window, cx);
        div()
            .id("top-bar")
            .h(px(52.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(28.))
            .border_b_1()
            .border_color(t.line)
            .window_control_area(gpui::WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |e, window, _| {
                if e.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            .child(div().text_size(px(sz::XL)).font_weight(FontWeight::BOLD).child(page.title()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h_full()
                    .pr(px(if controls.is_some() { 0. } else { 20. }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(match page {
                        Page::Apps => views::apps::actions(cx),
                        Page::Cloud => views::cloud::actions(cx),
                        Page::Account => views::account::actions(cx),
                        Page::Settings => div().into_any_element(),
                    })
                    .children(controls),
            )
            .into_any_element()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let s = self.store.read(cx);
        let page = s.page;
        let menu = s.menu.clone();
        let toasts = s.toasts.clone();
        let dialog = s.dialog.clone();
        window.set_window_title(&format!("{} — lsuite", page.title()));
        let body = match page {
            Page::Apps => views::apps::page(window, cx),
            Page::Cloud => views::cloud::page(window, cx),
            Page::Account => views::account::page(self, window, cx),
            Page::Settings => views::settings::page(window, cx),
        };
        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(|ws, _: &Escape, window, cx| {
                let s = ws.store.read(cx);
                if s.menu.is_some() {
                    ws.store.update(cx, |s, cx| {
                        s.menu = None;
                        cx.notify();
                    });
                } else if s.dialog.is_some() {
                    ws.close_dialog(window, cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &Confirm, window, cx| {
                if ws.store.read(cx).dialog.is_some() {
                    views::overlays::confirm(ws, window, cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &ShowApps, _, cx| ws.store.update(cx, |s, cx| s.go(Page::Apps, cx))))
            .on_action(cx.listener(|ws, _: &ShowCloud, _, cx| ws.store.update(cx, |s, cx| s.go(Page::Cloud, cx))))
            .on_action(cx.listener(|ws, _: &ShowAccount, _, cx| ws.store.update(cx, |s, cx| s.go(Page::Account, cx))))
            .on_action(cx.listener(|ws, _: &ShowSettings, _, cx| ws.store.update(cx, |s, cx| s.go(Page::Settings, cx))))
            .on_action(cx.listener(|ws, _: &CheckUpdates, _, cx| ws.store.update(cx, |s, cx| s.check_updates(cx))))
            .on_drop(cx.listener(|ws, paths: &gpui::ExternalPaths, _, cx| {
                // Files dropped on the window go to the cloud folder shown.
                ws.store.update(cx, |s, cx| {
                    if !s.signed_in() {
                        return s.toast(ToastKind::Info, "Sign in to lsuite to put files in your cloud.", cx);
                    }
                    s.go(Page::Cloud, cx);
                    let into = s.cloud_dir.clone();
                    for p in paths.paths() {
                        s.run("cloud.upload", json!({ "source": p, "into": into }), cx);
                    }
                })
            }))
            .relative()
            .size_full()
            .font_family(crate::theme::SANS)
            .text_size(px(sz::BASE))
            .text_color(t.text)
            .child(crate::ui::grain::backdrop(t.bg, window, cx))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .child(self.sidebar(window, cx))
                    .child(div().flex_1().min_w_0().h_full().flex().flex_col().child(self.top_bar(window, cx)).child(div().flex_1().min_h_0().child(body))),
            )
            .when_some(menu, |d, (app, at)| d.child(views::overlays::app_menu(&app, at, window, cx)))
            .when_some(dialog, |d, dlg| d.child(views::overlays::dialog(self, &dlg, window, cx)))
            .child(views::overlays::toasts(toasts, cx))
    }
}

/// The launcher's own update, in the sidebar: available, downloading, or ready to restart.
fn update_banner(u: &serde_json::Value, cx: &App) -> Option<AnyElement> {
    let t = cx.theme();
    let ready = u["ready"] == true;
    let version = u["available"].as_str()?.to_string();
    let can = u["canInstall"] == true;
    let busy = u["busy"] == true;
    let action: AnyElement = if ready {
        crate::ui::Button::new("restart", "Restart to update").primary().small().full_width().on_click(|_, _, cx| {
            let l = cx.store().read(cx).launcher.clone();
            match lsuite_core::selfupdate::restart(&l) {
                Ok(()) => cx.quit(),
                Err(e) => cx.store().update(cx, |s, cx| s.toast(ToastKind::Error, format!("Couldn't restart ({e})."), cx)),
            }
        }).into_any_element()
    } else if busy {
        crate::ui::busy_bar("update-busy", cx)
    } else if can {
        crate::ui::Button::new("update-self", "Update").with_icon("download").primary().small().full_width().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("app.installUpdate", json!({}), cx))).into_any_element()
    } else {
        let url = u["downloadUrl"].as_str().unwrap_or(lsuite_core::selfupdate::RELEASES_URL).to_string();
        crate::ui::Button::new("update-get", "Download").icon_after("arrow-up-right").small().full_width().on_click(move |_, _, _| {
            let _ = lsuite_core::util::open_external(&url);
        }).into_any_element()
    };
    Some(
        div()
            .mx(px(10.))
            .mb(px(10.))
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_1()
            .border_color(t.line_strong)
            .child(div().font_weight(FontWeight::SEMIBOLD).child(if ready { format!("lsuite {version} is installed") } else { format!("lsuite {version} is available") }))
            .when_some(u["installBlocked"].as_str().filter(|_| !can && !ready), |d, why| d.child(div().text_size(px(sz::XS)).text_color(t.text_3).child(why.to_string())))
            .child(div().flex().child(action))
            .into_any_element(),
    )
}

/// How to register the launcher's MCP server with Claude Code, with a path that stays valid:
/// the AppImage itself (`… mcp`) when run from one, else `lsuite-mcp` beside this program.
pub fn mcp_command() -> Option<String> {
    let path = match std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).filter(|p| p.is_file()) {
        Some(image) => format!("{} mcp", tilde(&image)),
        None => {
            let exe = std::env::current_exe().ok()?;
            let mcp = exe.parent()?.join(if cfg!(windows) { "lsuite-mcp.exe" } else { "lsuite-mcp" });
            mcp.exists().then(|| tilde(&mcp))?
        }
    };
    Some(format!("claude mcp add lsuite -- {path}"))
}

/// A path for a shell: `~/…` under the home folder (when it needs no quotes), quoted otherwise.
pub fn tilde(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    if !cfg!(windows)
        && let Some(home) = dirs::home_dir()
        && let Ok(rest) = p.strip_prefix(&home)
    {
        let rest = rest.display().to_string();
        if crate::ui::shell_quote(&rest) == rest {
            return format!("~/{rest}");
        }
    }
    crate::ui::shell_quote(&s)
}

/// An app's icon, `size` square.
pub fn app_icon(id: &str, size: f32) -> gpui::Img {
    img(crate::assets::app_icon(id)).size(px(size)).flex_none()
}
