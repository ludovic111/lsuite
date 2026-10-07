//! Settings: appearance, updates, what agents may do, and where things are.

use gpui::{AnyElement, App, FontWeight, SharedString, Window, div, prelude::*, px};
use serde_json::json;

use crate::store::{StoreExt, ToastKind};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, caps, segmented, switch};
use crate::views::{code_line, column};

fn block(title: &'static str, text: &'static str, body: impl IntoElement, cx: &App) -> AnyElement {
    let t = cx.theme();
    div()
        .flex()
        .gap(px(24.))
        .p(px(18.))
        .glass(t.glass1)
        .child(div().w(px(240.)).flex_none().flex().flex_col().gap(px(4.)).child(div().font_weight(FontWeight::SEMIBOLD).text_size(px(sz::MD)).child(title)).child(div().text_size(px(sz::SM)).text_color(t.text_2).child(text)))
        .child(div().flex_1().min_w_0().flex().flex_col().gap(px(12.)).child(body))
        .into_any_element()
}

fn toggle(id: &'static str, label: &'static str, key: &'static str, on: bool, cx: &App) -> AnyElement {
    switch(SharedString::from(id), label, on, move |v, _, cx| cx.store().update(cx, |s, cx| s.set_setting(key, json!(v), cx)), cx).into_any_element()
}

pub fn page(_window: &mut Window, cx: &mut App) -> AnyElement {
    let s = cx.store().read(cx);
    let t = cx.theme().clone();
    let set = s.settings.clone();
    let platform = s.apps["platform"].as_str().unwrap_or("").to_string();
    let apps_dir = s.apps["appsDir"].as_str().unwrap_or("").to_string();
    let home = lsuite_core::paths::lsuite_home().display().to_string();
    let server = lsuite_core::account::server();
    let mcp = crate::app::mcp_command();
    let a = &set.agent;
    let u = s.update.clone();
    let update_line = u["message"].as_str().map(str::to_string).or_else(|| u["error"].as_str().map(str::to_string)).unwrap_or_else(|| "Not checked yet".into());
    column(
        "settings-scroll",
        960.,
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(block(
                "Appearance",
                "Light and dark follow the system unless you choose.",
                segmented("theme", vec![("system".to_string(), "System".into()), ("dark".to_string(), "Dark".into()), ("light".to_string(), "Light".into())], set.theme.clone(), |v, _, cx| cx.store().update(cx, |s, cx| s.set_setting("theme", json!(v), cx)), cx)
                    .w(px(300.)),
                cx,
            ))
            .child(block(
                "Updates",
                "Each app also updates itself; the launcher keeps them all in one place.",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(toggle("set-check", "Look for new versions when the launcher starts", "checkOnStart", set.check_on_start, cx))
                    .child(toggle("set-auto", "Install them by themselves (apps that are closed)", "autoUpdate", set.auto_update, cx))
                    .child(toggle("set-glass", "Reduce transparency", "reduceTransparency", set.reduce_transparency, cx))
                    .child(div().flex().items_center().justify_between().gap(px(10.)).child("Sync synced folders every").child(segmented(
                        "sync-every",
                        vec![(0u64, "Only when asked".into()), (5u64, "5 min".into()), (15u64, "15 min".into()), (60u64, "Hour".into())],
                        set.sync_every_minutes,
                        |v, _, cx| cx.store().update(cx, |s, cx| s.set_setting("syncEveryMinutes", json!(v), cx)),
                        cx,
                    ).w(px(360.)))),
                cx,
            ))
            .child(block(
                "Agents",
                "What an agent may do through lsuite-mcp. The window and lsuite-cli act for you and can always do everything.",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(toggle("agent-install", "Install and update apps", "agent.install", a.install, cx))
                    .child(toggle("agent-remove", "Remove apps", "agent.remove", a.remove, cx))
                    .child(toggle("agent-cloud-write", "Upload, create folders, move and rename in the cloud", "agent.cloudWrite", a.cloud_write, cx))
                    .child(toggle("agent-cloud-delete", "Delete in the cloud", "agent.cloudDelete", a.cloud_delete, cx))
                    .child(toggle("agent-account", "Sign in and out", "agent.account", a.account, cx))
                    .child(div().flex().flex_col().gap(px(6.)).mt(px(4.)).child(caps("Connect an agent", cx)).child(match &mcp {
                        Some(line) => code_line("set-mcp", line.clone(), cx).into_any_element(),
                        None => div().text_size(px(sz::SM)).text_color(t.text_3).child("lsuite-mcp sits next to the launcher in a release build.").into_any_element(),
                    })),
                cx,
            ))
            .child(block(
                "About",
                "lsuite is free and open source (MIT).",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(info("Version", format!("lsuite {} · {platform}", lsuite_core::VERSION), cx))
                    .child(info("Updates", update_line, cx))
                    .child(info("Apps go in", apps_dir.clone(), cx))
                    .child(info("Account and discovery", home, cx))
                    .child(info("Server", server, cx))
                    .child(div().flex().gap(px(8.)).mt(px(4.)).child(Button::new("check-self", "Check for updates").with_icon("refresh-cw").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run_result("app.checkUpdates", json!({}), cx, |s, r, cx| match r {
                        Ok(v) => {
                            s.toast(ToastKind::Info, v["message"].as_str().unwrap_or("").to_string(), cx);
                            s.update = v;
                        }
                        Err(e) => s.toast(ToastKind::Error, e, cx),
                    })))).child(Button::new("open-apps-dir", crate::ui::reveal_label()).with_icon("folder-open").small().on_click(move |_, _, _| {
                        let _ = std::fs::create_dir_all(&apps_dir);
                        let _ = lsuite_core::util::open_external(&apps_dir);
                    })).child(Button::new("site", "lsuite.xyz").icon_after("arrow-up-right").ghost().small().on_click(|_, _, _| {
                        let _ = lsuite_core::util::open_external("https://lsuite.xyz");
                    }))),
                cx,
            )),
    )
}

fn info(label: &'static str, value: String, cx: &App) -> AnyElement {
    let t = cx.theme();
    div().flex().gap(px(12.)).child(div().w(px(160.)).flex_none().text_color(t.text_2).child(label)).child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(sz::SM)).child(value)).into_any_element()
}
