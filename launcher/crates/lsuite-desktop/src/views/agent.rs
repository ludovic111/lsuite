//! Agent: the lsuite agent, for jobs that span the apps. The conversation shows what was asked,
//! every tool it used in which app (with the pictures it looked at), and its answers.

use gpui::{AnyElement, App, Context, FontWeight, ObjectFit, SharedString, Window, div, img, prelude::*, px};
use serde_json::{Value, json};

use crate::app::{Workspace, app_icon};
use crate::store::StoreExt;
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, caps, icon, segmented, spinner};

const SUGGESTIONS: &[&str] = &[
    "Write a calm 60-second piano track in ryolune and put it under my latest kimchi cut",
    "Turn my report in folio into a 10-slide deck, then make a matching poster in nori",
    "Make a budget sheet for a 3-day trip in folio, with a chart of the costs",
    "Design an album cover in nori for the track I'm making in ryolune",
];

pub fn actions(cx: &App) -> AnyElement {
    let s = cx.store().read(cx);
    let running = s.agent["running"] == true;
    let list: Vec<(Option<String>, SharedString)> = s.agent_providers["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["ready"] == true)
        .map(|p| (p["id"].as_str().map(str::to_string), p["name"].as_str().unwrap_or("").to_string().into()))
        .collect();
    let current = s.agent_provider.clone().or_else(|| s.agent_providers["default"].as_str().map(str::to_string));
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .when(list.len() > 1, |d| {
            d.child(
                segmented("agent-provider", list, current, |v, _, cx| {
                    let v = v.clone();
                    cx.store().update(cx, |s, cx| {
                        s.agent_provider = v;
                        cx.notify();
                    })
                }, cx)
                .w(px(320.)),
            )
        })
        .child(Button::new("agent-new", "New conversation").with_icon("pencil").small().disabled(running).on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run_then("agent.clear", json!({}), cx, |s, _, _| s.agent = lsuite_core::agent::log(&s.launcher)))))
        .into_any_element()
}

pub fn page(ws: &Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let s = cx.store().read(cx);
    let t = cx.theme().clone();
    let log: Vec<Value> = s.agent["log"].as_array().cloned().unwrap_or_default();
    let running = s.agent["running"] == true;
    let ready = s.agent_providers["default"].is_string();
    let entity = cx.entity();
    let _ = window;

    let body: AnyElement = if log.is_empty() {
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .pt(px(10.))
            .child(div().flex().flex_col().gap(px(6.)).child(div().text_size(px(sz::XXL)).font_weight(FontWeight::BOLD).child("One agent for every app.")).child(div().max_w(px(720.)).text_size(px(sz::MD)).text_color(t.text_2).child("Ask for a job, in one app or across several. The agent reads each app's expert brief and skills, does the work there, looks at what it made and fixes it before it reports. Every app keeps its changes as one undo step.")))
            .when(!ready, |d| d.child(div().p(px(14.)).glass(t.glass1).text_color(t.text_2).child("To run the agent: install Claude Code (it uses your Claude subscription), or set ANTHROPIC_API_KEY.")))
            .child(caps("Try", cx))
            .child(div().flex().flex_wrap().gap(px(10.)).children(SUGGESTIONS.iter().enumerate().map(|(i, s)| {
                let text = s.to_string();
                let e = entity.clone();
                div()
                    .id(("agent-suggest", i))
                    .w(px(330.))
                    .p(px(12.))
                    .glass(t.glass1)
                    .cursor_pointer()
                    .hover(|d| d.border_color(t.accent))
                    .text_color(t.text_2)
                    .child(text.clone())
                    .on_click(move |_, _, cx| {
                        let text = text.clone();
                        e.update(cx, |ws, cx| ws.agent_input.update(cx, |i, cx| i.set_text(text, cx)))
                    })
            })))
            .into_any_element()
    } else {
        div().flex().flex_col().gap(px(10.)).children(log.iter().enumerate().map(|(i, e)| entry(i, e, cx))).when(running, |d| d.child(div().flex().items_center().gap(px(8.)).text_color(t.text_3).child(spinner("agent-working")).child("Working…"))).into_any_element()
    };

    let e1 = cx.entity();
    let composer = div()
        .flex_none()
        .flex()
        .items_end()
        .gap(px(10.))
        .p(px(14.))
        .border_t_1()
        .border_color(t.line)
        .child(div().flex_1().min_w_0().child(ws.agent_input.clone()))
        .child(if running {
            Button::new("agent-stop", "Stop").with_icon("square").danger().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("agent.stop", json!({}), cx))).into_any_element()
        } else {
            Button::new("agent-send", "Ask").with_icon("sparkles").primary().disabled(!ready).on_click(move |_, _, cx| e1.update(cx, |ws, cx| ws.ask_agent(cx))).into_any_element()
        });

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(div().id("agent-scroll").flex_1().min_h_0().overflow_y_scroll().child(div().max_w(px(920.)).mx_auto().px(px(28.)).py(px(22.)).child(body)))
        .child(div().max_w(px(920.)).w_full().mx_auto().child(composer))
        .into_any_element()
}

fn entry(i: usize, e: &Value, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let text = |k: &str| e[k].as_str().unwrap_or("").to_string();
    match e["kind"].as_str().unwrap_or("") {
        "user" => div().flex().justify_end().child(div().max_w(px(640.)).px(px(14.)).py(px(10.)).bg(t.accent).text_color(t.text_on_accent).child(text("text"))).into_any_element(),
        "assistant" => div().max_w(px(760.)).text_size(px(sz::MD)).line_height(px(sz::MD * 1.5)).child(text("text")).into_any_element(),
        "tool" => {
            let app = text("app");
            let ok = e["ok"] != false;
            let image = e["image"].as_str().map(std::path::PathBuf::from).filter(|p| p.is_file());
            div()
                .id(("agent-tool", i))
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(12.))
                .py(px(8.))
                .glass(t.glass1)
                .when(!ok, |d| d.border_color(t.danger))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(if lsuite_core::catalog::get(&app).is_some() { app_icon(&app, 18.).into_any_element() } else { icon("box").into_any_element() })
                        .child(div().font_family(MONO).text_size(px(sz::SM)).font_weight(FontWeight::SEMIBOLD).child(format!("{app} · {}", text("tool"))))
                        .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(text("input"))),
                )
                .when(!text("output").is_empty(), |d| d.child(div().font_family(MONO).text_size(px(sz::XS)).text_color(if ok { t.text_2 } else { t.danger }).child(text("output"))))
                .when_some(image, |d, p| d.child(img(p).max_w(px(560.)).max_h(px(320.)).object_fit(ObjectFit::Contain)))
                .into_any_element()
        }
        "error" => div().flex().items_center().gap(px(8.)).text_color(t.danger).child(icon("circle-alert")).child(text("text")).into_any_element(),
        _ => div().text_size(px(sz::SM)).text_color(t.text_3).child(text("text")).into_any_element(),
    }
}
