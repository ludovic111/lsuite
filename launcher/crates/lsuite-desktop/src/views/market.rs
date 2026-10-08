//! Marketplace: plugins for the apps made by the people who use lsuite and by lsuite, every
//! version reviewed. Installing comes with lsuite Pass; publishing needs an account.

use gpui::{AnyElement, App, FontWeight, PathPromptOptions, SharedString, Window, div, prelude::*, px};
use serde_json::{Value, json};

use crate::app::app_icon;
use crate::store::{Dialog, StoreExt};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, caps, segmented, spinner, tag};
use crate::views::{column, empty, lede};

pub fn actions(cx: &App) -> AnyElement {
    let s = cx.store().read(cx);
    div()
        .flex()
        .gap(px(8.))
        .when(s.signed_in(), |d| d.child(Button::new("publish", "Publish a plugin").with_icon("upload").small().on_click(|_, _, cx| pick_bundle(cx))))
        .child(Button::icon("market-refresh", "refresh-cw", "Refresh").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.refresh_market(cx))))
        .into_any_element()
}

/// Asks for a plugin bundle folder (a path when there's no picker), then publishes it.
fn pick_bundle(cx: &mut App) {
    if std::env::var("LSUITE_NO_PICKER").is_ok_and(|v| !v.is_empty() && v != "0") {
        return cx.store().update(cx, |s, cx| s.open_dialog(Dialog::PublishPath, cx));
    }
    let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Publish".into()) });
    let store = cx.store();
    cx.spawn(async move |cx| {
        let picked = rx.await;
        store.update(cx, |s, cx| match picked {
            Ok(Ok(Some(mut p))) => {
                if let Some(path) = p.pop() {
                    s.run("market.publish", json!({ "path": path }), cx);
                }
            }
            Ok(Ok(None)) => {}
            _ => s.open_dialog(Dialog::PublishPath, cx),
        });
    })
    .detach();
}

pub fn page(window: &mut Window, cx: &mut App) -> AnyElement {
    let s = cx.store().read(cx);
    let t = cx.theme().clone();
    let filter = s.market_app.clone();
    let has_pass = s.has_pass();
    let signed_in = s.signed_in();
    let tasks = s.tasks.clone();
    let error = s.market_error.clone();
    let mine: Vec<Value> = s.market_mine.as_array().cloned().unwrap_or_default();
    let loading = s.market.is_null() && error.is_none();
    let plugins: Vec<Value> = s.market["plugins"].as_array().cloned().unwrap_or_default().into_iter().filter(|p| filter.as_deref().is_none_or(|a| p["app"] == a)).collect();
    let server = lsuite_core::account::server();

    let mut options: Vec<(Option<String>, SharedString)> = vec![(None, "All".into())];
    for a in lsuite_core::catalog::APPS {
        options.push((Some(a.id.to_string()), a.name.into()));
    }
    let filter_row = segmented("market-app", options, filter.clone(), |v, _, cx| {
        let v = v.clone();
        cx.store().update(cx, |s, cx| {
            s.market_app = v;
            cx.notify();
        })
    }, cx)
    .w(px(560.));

    let pass_banner = (!has_pass).then(|| {
        let url = format!("{server}/pass");
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(14.))
            .p(px(14.))
            .glass(t.glass1)
            .child(div().flex().flex_col().gap(px(2.)).child(div().font_weight(FontWeight::SEMIBOLD).child("Installing comes with lsuite Pass")).child(div().text_size(px(sz::SM)).text_color(t.text_2).child(if signed_in { "Browse freely; any Pass plan installs every plugin, with lsuite AI and lsuite Cloud." } else { "Browse freely; sign in, and any Pass plan installs every plugin." })))
            .child(Button::new("get-pass", "See lsuite Pass").icon_after("arrow-up-right").primary().small().on_click(move |_, _, _| {
                let _ = lsuite_core::util::open_external(&url);
            }))
            .into_any_element()
    });

    let grid: AnyElement = if loading {
        div().flex().justify_center().py(px(40.)).text_color(t.text_3).child(spinner("market-wait")).into_any_element()
    } else if let Some(e) = error {
        div().p(px(14.)).glass(t.glass1).text_color(t.danger).child(e).into_any_element()
    } else if plugins.is_empty() {
        empty(
            "The first plugins are on their way",
            "Build one with your app's agent (Plugins › Build with your agent), then publish it here: lsuite reviews every version before anyone can install it.",
            if signed_in { vec![Button::new("empty-publish", "Publish a plugin").with_icon("upload").primary().on_click(|_, _, cx| pick_bundle(cx)).into_any_element()] } else { vec![] },
            window,
            cx,
        )
        .glass(t.glass1)
        .into_any_element()
    } else {
        div().flex().flex_wrap().gap(px(14.)).children(plugins.iter().map(|p| card(p, has_pass, tasks.contains_key(&format!("plugin:{}", p["id"].as_str().unwrap_or(""))), cx))).into_any_element()
    };

    column(
        "market-scroll",
        1120.,
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(lede("Plugins made by the people who use lsuite.", "Effects, filters, functions and tools for every app, from people like you and from lsuite. Every version is reviewed before anyone can install it, and its file is checked on your computer before it goes in.", cx))
            .children(pass_banner)
            .child(filter_row)
            .child(grid)
            .when(signed_in, |d| d.child(yours(&mine, cx))),
    )
}

fn card(p: &Value, has_pass: bool, busy: bool, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let id = p["id"].as_str().unwrap_or("").to_string();
    let app = p["app"].as_str().unwrap_or("").to_string();
    let installed = p["installed"].as_str().map(str::to_string);
    let update = p["updateAvailable"] == true;
    let here = p["availableHere"] == true;
    let version = p["version"].as_str().unwrap_or("").to_string();
    let verified = p["author"]["verified"] == true;
    let (i1, i2) = (id.clone(), id.clone());
    let button: AnyElement = if busy {
        div().flex().items_center().gap(px(6.)).text_color(t.text_2).child(spinner(gpui::ElementId::Name(format!("plug-spin-{id}").into()))).child("Installing…").into_any_element()
    } else if !here {
        div().text_size(px(sz::SM)).text_color(t.text_3).child("Not for this computer yet").into_any_element()
    } else if installed.is_some() && !update {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(tag("installed", false, cx))
            .child(Button::new(SharedString::from(format!("plug-rm-{id}")), "Remove").ghost().small().on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("market.remove", json!({ "id": i1 }), cx))))
            .into_any_element()
    } else {
        Button::new(SharedString::from(format!("plug-get-{id}")), if update { format!("Update to {version}") } else { "Install".to_string() })
            .with_icon("download")
            .primary()
            .small()
            .disabled(!has_pass)
            .tooltip(if has_pass { "Install it for this computer" } else { "Comes with lsuite Pass" })
            .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("market.install", json!({ "id": i2 }), cx)))
            .into_any_element()
    };
    let platforms = p["platforms"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(" · ")).unwrap_or_default();
    div()
        .w(px(340.))
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(16.))
        .glass(t.glass1)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(app_icon(&app, 32.))
                .child(div().flex_1().min_w_0().flex().flex_col().child(div().truncate().text_size(px(sz::LG)).font_weight(FontWeight::BOLD).child(p["name"].as_str().unwrap_or("").to_string())).child(caps(format!("{app} · {}", p["kind"].as_str().unwrap_or("plugin")), cx)))
                .when(verified, |d| d.child(tag("by lsuite", true, cx))),
        )
        .child(div().h(px(38.)).text_color(t.text_2).child(p["description"].as_str().unwrap_or("").to_string()))
        .child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).truncate().child(format!(
            "v{version} · {} · {} download{} · {platforms}",
            p["author"]["name"].as_str().unwrap_or("someone"),
            p["downloads"].as_u64().unwrap_or(0),
            if p["downloads"].as_u64() == Some(1) { "" } else { "s" }
        )))
        .child(div().h(px(24.)).flex().items_center().child(button))
        .into_any_element()
}

/// The account's own submissions and their review.
fn yours(mine: &[Value], cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(caps("Your plugins", cx))
        .child(if mine.is_empty() {
            div().text_color(t.text_3).child("Nothing published yet. Build a plugin with an app's agent, then Publish a plugin.").into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .glass(t.glass1)
                .children(mine.iter().map(|m| {
                    let status = m["status"].as_str().unwrap_or("pending").to_string();
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(9.))
                        .border_b_1()
                        .border_color(t.line)
                        .child(app_icon(m["app"].as_str().unwrap_or(""), 22.))
                        .child(div().flex_1().min_w_0().flex().flex_col().child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(format!("{} {}", m["name"].as_str().unwrap_or(""), m["version"].as_str().unwrap_or("")))).when_some(m["note"].as_str().filter(|n| !n.is_empty()), |d, n| d.child(div().text_size(px(sz::SM)).text_color(t.text_2).child(n.to_string()))))
                        .child(tag(match status.as_str() {
                            "approved" => "listed",
                            "rejected" => "not approved",
                            _ => "in review",
                        }, status == "approved", cx))
                }))
                .into_any_element()
        })
        .into_any_element()
}
