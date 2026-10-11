//! Plugins: the lsuite plugins installed for each app (`~/.lsuite/plugins/<app>/<id>/`), with
//! Remove, and "Build a plugin", which asks the lsuite agent to make one with the app's own
//! plugin tools.

use gpui::{AnyElement, App, FontWeight, SharedString, Window, div, prelude::*, px};
use serde_json::Value;

use crate::app::app_icon;
use crate::store::{Dialog, Page, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, caps, spinner, tag};
use crate::views::{column, empty, lede};

pub fn actions(_cx: &App) -> AnyElement {
    Button::icon("plugins-refresh", "refresh-cw", "Refresh").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.refresh_plugins(cx))).into_any_element()
}

pub fn page(window: &mut Window, cx: &mut App) -> AnyElement {
    let s = cx.store().read(cx);
    let t = cx.theme().clone();
    let loading = s.plugins.is_null();
    // Installed apps, and any other app that still has plugins in its folder.
    let apps: Vec<Value> = s.plugins["apps"].as_array().cloned().unwrap_or_default().into_iter().filter(|a| a["installed"] == true || a["plugins"].as_array().is_some_and(|p| !p.is_empty())).collect();
    let ready = s.agent_providers["default"].is_string();
    let running = s.agent["running"] == true;

    let body: AnyElement = if loading {
        div().flex().justify_center().py(px(40.)).text_color(t.text_3).child(spinner("plugins-wait")).into_any_element()
    } else if apps.is_empty() {
        empty(
            "No app installed yet",
            "Plugins belong to an app: install ryolune, kimchi, nori or folio first, then build plugins for it here.",
            vec![Button::new("plugins-to-apps", "See the apps").with_icon("layout-grid").primary().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.go(Page::Apps, cx))).into_any_element()],
            window,
            cx,
        )
        .glass(t.glass1)
        .into_any_element()
    } else {
        div().flex().flex_col().gap(px(22.)).children(apps.iter().map(|a| section(a, ready, running, cx))).into_any_element()
    };

    column(
        "plugins-scroll",
        960.,
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(lede("Plugins, built for you.", "Effects, filters, functions and tools for each app. Describe one and the lsuite agent builds it with the app's plugin kit, checks that it compiles and installs it; the app loads it without a restart.", cx))
            .when(!ready && !loading && !apps.is_empty(), |d| d.child(div().p(px(14.)).glass(t.glass1).text_color(t.text_2).child("To build plugins, the lsuite agent needs Claude Code on this computer (it uses your Claude subscription) or ANTHROPIC_API_KEY.")))
            .child(body),
    )
}

/// One app: its plugins, and the button to build another.
fn section(a: &Value, ready: bool, running: bool, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let id = a["app"].as_str().unwrap_or("").to_string();
    let name = a["name"].as_str().unwrap_or("").to_string();
    let installed = a["installed"] == true;
    let plugins: Vec<Value> = a["plugins"].as_array().cloned().unwrap_or_default();
    let (i1, n1) = (id.clone(), name.clone());
    let build = Button::new(SharedString::from(format!("build-{id}")), "Build a plugin")
        .with_icon("sparkles")
        .small()
        .disabled(!ready || running || !installed)
        .tooltip(if !installed {
            format!("Install {name} first")
        } else if running {
            "The agent is working: wait for it, or stop it".to_string()
        } else if ready {
            format!("Describe a plugin and the lsuite agent builds it for {name}")
        } else {
            "Needs Claude Code or ANTHROPIC_API_KEY".to_string()
        })
        .on_click(move |_, _, cx| {
            let (app, name) = (i1.clone(), n1.clone());
            cx.store().update(cx, |s, cx| s.open_dialog(Dialog::BuildPlugin { app, name }, cx))
        });
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(app_icon(&id, 28.))
                .child(div().text_size(px(sz::LG)).font_weight(FontWeight::BOLD).child(name.clone()))
                .child(caps(format!("{} plugin{}", plugins.len(), if plugins.len() == 1 { "" } else { "s" }), cx))
                .when(!installed, |d| d.child(tag("not installed", false, cx)))
                .child(div().flex_1())
                .child(build),
        )
        .child(if plugins.is_empty() {
            div().px(px(14.)).py(px(12.)).glass(t.glass1).text_color(t.text_3).child(format!("No plugins for {name} yet.")).into_any_element()
        } else {
            div().flex().flex_col().glass(t.glass1).children(plugins.iter().map(|p| row(&id, p, cx))).into_any_element()
        })
        .into_any_element()
}

fn row(app: &str, p: &Value, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let text = |k: &str| p[k].as_str().unwrap_or("").to_string();
    let (id, name) = (text("id"), text("name"));
    let (a1, i1, n1) = (app.to_string(), id.clone(), name.clone());
    let detail = [text("version"), id.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .py(px(10.))
        .border_b_1()
        .border_color(t.line)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().flex().items_center().gap(px(8.)).child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(name.clone())).when(!text("kind").is_empty(), |d| d.child(tag(text("kind"), false, cx))))
                .when(!text("description").is_empty(), |d| d.child(div().text_size(px(sz::SM)).text_color(t.text_2).child(text("description"))))
                .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).truncate().child(detail)),
        )
        .child(Button::new(SharedString::from(format!("plug-rm-{app}-{id}")), "Remove").with_icon("trash-2").ghost().small().on_click(move |_, _, cx| {
            let (app, id, name) = (a1.clone(), i1.clone(), n1.clone());
            cx.store().update(cx, |s, cx| s.open_dialog(Dialog::RemovePlugin { app, id, name }, cx))
        }))
        .into_any_element()
}
