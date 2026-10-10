//! Apps: every lsuite app, installed or not, with its version, the latest one, and what to do.

use gpui::{AnyElement, App, ElementId, FontWeight, SharedString, Window, div, prelude::*, px};
use serde_json::{Value, json};

use crate::app::app_icon;
use crate::store::{Dialog, StoreExt, TaskView};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, busy_bar, caps, meter, spinner, tag};
use crate::views::{code_line, column, lede};

/// The top bar's actions: look for updates, update everything.
pub fn actions(cx: &App) -> AnyElement {
    let s = cx.store().read(cx);
    let updates = s.apps["updates"].as_u64().unwrap_or(0);
    let checking = s.checking;
    div()
        .flex()
        .gap(px(8.))
        .child(if checking {
            div().flex().items_center().gap(px(6.)).px(px(10.)).text_color(cx.theme().text_2).child(spinner("check-spin")).child("Checking…").into_any_element()
        } else {
            Button::new("check", "Check for updates").with_icon("refresh-cw").small().tooltip("Ask GitHub for each app's latest release").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.check_updates(cx))).into_any_element()
        })
        .when(updates > 0, |d| {
            d.child(Button::new("update-all", format!("Update all ({updates})")).with_icon("download").small().primary().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("apps.updateAll", json!({}), cx))))
        })
        .into_any_element()
}

pub fn page(_window: &mut Window, cx: &mut App) -> AnyElement {
    let s = cx.store().read(cx);
    let apps: Vec<Value> = s.apps["apps"].as_array().cloned().unwrap_or_default();
    let os = s.apps["os"].as_str().unwrap_or("").to_string();
    let platform = s.apps["platform"].as_str().unwrap_or("").to_string();
    let tasks = s.tasks.clone();
    let installed = s.apps["installed"].as_u64().unwrap_or(0);
    let others: Vec<Value> = s.apps["others"].as_array().cloned().unwrap_or_default();
    let signed_in = s.signed_in();
    let account_known = !s.account.is_null();
    let cards: Vec<AnyElement> = apps.iter().map(|a| card(a, tasks.get(&format!("install:{}", a["id"].as_str().unwrap_or(""))).cloned(), cx)).collect();
    let t = cx.theme().clone();
    column(
        "apps-scroll",
        1120.,
        div()
            .flex()
            .flex_col()
            .gap(px(26.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(px(20.))
                    .child(lede(
                        "Every lsuite app, signed and up to date.",
                        "Music, video, image and office: free, open source and driven by your agent. Each one comes from its own GitHub releases, and every download is checked against the app's signing key before anything is replaced.",
                        cx,
                    ))
                    .child(div().flex().flex_col().items_end().gap(px(4.)).flex_none().child(tag(format!("{os} · {}", platform.split('-').nth(1).unwrap_or("")), false, cx)).child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(format!("{installed} of {} installed", apps.len())))),
            )
            .when(account_known && !signed_in, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(14.))
                        .p(px(16.))
                        .glass(t.glass1)
                        .border_color(t.accent)
                        .child(div().flex().flex_col().gap(px(2.)).child(div().font_weight(FontWeight::SEMIBOLD).text_size(px(sz::MD)).child("Sign in to get the apps")).child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Your free lsuite account gets you every app, its updates and the marketplace. No payment, ever, for the apps.")))
                        .child(Button::new("apps-signin", "Sign in").with_icon("log-in").primary().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.go(crate::store::Page::Account, cx)))),
                )
            })
            .child(div().flex().flex_wrap().gap(px(14.)).children(cards))
            .when(!others.is_empty(), |d| d.child(more(&others, cx)))
            .child(agents(&apps, cx)),
    )
}

fn card(a: &Value, task: Option<TaskView>, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let id = a["id"].as_str().unwrap_or("").to_string();
    let name = a["name"].as_str().unwrap_or("").to_string();
    let installed = a["installed"] == true;
    let running = a["running"] == true;
    let available = a["available"] == true;
    let update = a["updateAvailable"] == true;
    let busy = a["busy"] == true || task.is_some();
    let can_manage = a["canManage"] == true;
    let version = a["version"].as_str().map(str::to_string);
    let latest = a["latest"].as_str().map(str::to_string);
    let os = cx.store().read(cx).apps["os"].as_str().unwrap_or("this computer").to_string();

    let (status, lit) = if busy {
        ("working", true)
    } else if running {
        ("open", true)
    } else if update {
        ("update", true)
    } else if installed {
        ("installed", false)
    } else if !available {
        ("not for this os", false)
    } else {
        ("not installed", false)
    };
    let line = match (&version, &latest, installed) {
        (Some(v), Some(l), true) if update => format!("{v} installed · {l} ready"),
        (Some(v), _, true) => format!("{v} installed · up to date"),
        (None, Some(l), true) => format!("installed · {l} is the latest"),
        (None, None, true) => "installed".to_string(),
        (_, Some(l), false) if available => format!("{l} available"),
        _ if !available => format!("no {os} build yet"),
        _ => "looking for the latest release…".to_string(),
    };
    let (i1, i2, i3, i4, i5) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
    let n2 = name.clone();
    let mut buttons: Vec<AnyElement> = vec![];
    if busy {
        // The card shows the task's progress instead.
    } else if installed {
        buttons.push(Button::new(SharedString::from(format!("open-{id}")), if running { "Show" } else { "Open" }).with_icon("play").primary().small().on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("apps.open", json!({ "app": i1 }), cx))).into_any_element());
        if update && can_manage {
            buttons.push(
                Button::new(SharedString::from(format!("update-{id}")), format!("Update to {}", latest.clone().unwrap_or_default()))
                    .with_icon("download")
                    .small()
                    .disabled(running)
                    .tooltip(if running { format!("Quit {name} to update it") } else { "Download, check and install the new version".into() })
                    .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("apps.update", json!({ "app": i2 }), cx)))
                    .into_any_element(),
            );
        }
    } else if available {
        let signed_in = cx.store().read(cx).signed_in();
        buttons.push(Button::new(SharedString::from(format!("install-{id}")), "Install").with_icon("download").primary().small().disabled(!signed_in).tooltip(if signed_in { "Download, check and install it" } else { "Sign in to get the apps (free)" }).on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("apps.install", json!({ "app": i3 }), cx))).into_any_element());
    }
    if busy {
    } else if !installed {
        buttons.push(Button::new(SharedString::from(format!("page-{id}")), "Page").icon_after("arrow-up-right").ghost().small().on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("apps.page", json!({ "app": i4 }), cx))).into_any_element());
    } else {
        buttons.push(
            Button::icon(SharedString::from(format!("more-{id}")), "ellipsis", format!("More for {n2}"))
                .small()
                .on_click(move |e, _, cx| {
                    let at = e.position();
                    let id = i5.clone();
                    cx.store().update(cx, |s, cx| {
                        s.menu = if s.menu.as_ref().is_some_and(|(m, _)| *m == id) { None } else { Some((id, at)) };
                        cx.notify();
                    })
                })
                .into_any_element(),
        );
    }

    div()
        .w(px(340.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .p(px(16.))
        .glass(t.glass1)
        .when(update && !busy, |d| d.border_color(t.line_strong))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(app_icon(&id, 56.))
                .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(div().text_size(px(sz::XL)).font_weight(FontWeight::BOLD).child(name.clone())).child(caps(a["kindLabel"].as_str().unwrap_or("").to_string(), cx)))
                .child(div().self_start().child(tag(status, lit, cx))),
        )
        .child(div().h(px(36.)).text_color(t.text_2).child(a["summary"].as_str().unwrap_or("").to_string()))
        .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(line))
        .when(installed && !can_manage, |d| {
            d.child(div().text_size(px(sz::SM)).text_color(t.text_3).child(format!("Built from source at {}: update it there.", a["path"].as_str().unwrap_or(""))))
        })
        .when_some(task.clone(), |d, task| {
            d.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().flex().justify_between().text_size(px(sz::SM)).child(div().truncate().child(task.label.clone())).child(div().font_family(MONO).text_color(t.text_3).child(task.fraction().map(|f| format!("{:.0} %", f * 100.)).unwrap_or_default())))
                    .child(match task.fraction() {
                        Some(f) => meter(f, 2., cx).into_any_element(),
                        None => busy_bar(ElementId::Name(format!("card-busy-{id}").into()), cx),
                    }),
            )
        })
        .when(busy && task.is_none(), |d| d.child(busy_bar(ElementId::Name(format!("card-wait-{id}").into()), cx)))
        .child(div().flex().items_center().gap(px(8.)).h(px(24.)).children(buttons))
        .into_any_element()
}

/// Apps the site lists that this launcher doesn't know yet: their page only.
fn more(others: &[Value], cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(caps("More from lsuite", cx))
        .child(div().text_color(t.text_2).child("Newer than this launcher: get them from their page, or update the launcher."))
        .child(div().flex().flex_wrap().gap(px(10.)).children(others.iter().map(|o| {
            let page = o["page"].as_str().unwrap_or("https://lsuite.xyz").to_string();
            div()
                .w(px(352.))
                .flex()
                .items_center()
                .gap(px(10.))
                .p(px(12.))
                .glass(t.glass1)
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::BOLD).child(o["name"].as_str().unwrap_or("").to_string())).child(div().text_size(px(sz::SM)).text_color(t.text_2).truncate().child(o["summary"].as_str().unwrap_or("").to_string())))
                .child(Button::new(SharedString::from(format!("other-{}", o["id"].as_str().unwrap_or(""))), "Page").icon_after("arrow-up-right").ghost().small().on_click(move |_, _, _| {
                    let _ = lsuite_core::util::open_external(&page);
                }))
        })))
        .into_any_element()
}

/// How to hand the apps to an agent: the launcher's own MCP server and each installed app's.
fn agents(apps: &[Value], cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let mut lines: Vec<AnyElement> = vec![];
    if let Some(line) = crate::app::mcp_command() {
        lines.push(code_line("mcp-lsuite", line, cx).into_any_element());
    }
    for a in apps {
        if let (Some(id), Some(mcp)) = (a["id"].as_str(), a["mcp"].as_str()) {
            lines.push(code_line(format!("mcp-{id}"), format!("claude mcp add {id} -- {} --live", crate::ui::shell_quote(mcp)), cx).into_any_element());
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(caps("Drive them with your agent", cx))
        .child(div().max_w(px(760.)).text_color(t.text_2).child("Every app is a command line and an MCP server as well as a window. Give them to Claude Code, Codex or any MCP client; the launcher's own server installs and updates apps and manages your cloud. Apps show here once they have been opened once."))
        .children(lines)
        .into_any_element()
}

/// A menu entry: icon, label, whether it destroys, what it does.
pub type MenuEntry = (&'static str, String, bool, Box<dyn Fn(&mut App)>);

/// The entries of an installed app's "more" menu.
pub fn menu_entries(id: &str, cx: &App) -> Vec<MenuEntry> {
    let a = cx.store().read(cx).app(id);
    let name = a["name"].as_str().unwrap_or(id).to_string();
    let can_manage = a["canManage"] == true;
    let (i1, i2, i3) = (id.to_string(), id.to_string(), id.to_string());
    let mut out: Vec<MenuEntry> = vec![
        ("folder-open", crate::ui::reveal_label().to_string(), false, Box::new(move |cx: &mut App| cx.store().update(cx, |s, cx| s.run("apps.reveal", json!({ "app": i1 }), cx)))),
        ("arrow-up-right", format!("{name} on lsuite.xyz"), false, Box::new(move |cx: &mut App| cx.store().update(cx, |s, cx| s.run("apps.page", json!({ "app": i2 }), cx)))),
    ];
    if can_manage {
        out.push(("trash-2", format!("Remove {name}…"), true, Box::new(move |cx: &mut App| {
            let name = name.clone();
            let app = i3.clone();
            cx.store().update(cx, |s, cx| s.open_dialog(Dialog::RemoveApp { app, name }, cx))
        })));
    }
    out
}
