//! Account: the lsuite AI account every app on this computer shares: sign in (browser or key),
//! the plan, the month's allowance, cloud storage, and the plans.

use gpui::{AnyElement, App, Context, FontWeight, SharedString, Window, div, prelude::*, px};
use lsuite_core::util::bytes;
use serde_json::{Value, json};

use crate::app::Workspace;
use crate::store::StoreExt;
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, busy_bar, caps, icon, meter, spinner, tag};
use crate::views::{column, lede};

pub fn actions(cx: &App) -> AnyElement {
    let s = cx.store().read(cx);
    div()
        .flex()
        .gap(px(8.))
        .child(Button::icon("account-refresh", "refresh-cw", "Refresh").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| {
            s.refresh_account(cx);
            s.refresh_plans(cx);
        })))
        .when(s.signed_in(), |d| d.child(Button::new("manage", "Manage plan").icon_after("arrow-up-right").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.manage", json!({}), cx)))))
        .into_any_element()
}

/// The sidebar's foot: who is signed in, or a way to sign in.
pub fn chip(signed_in: bool, account: &Value, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let body = if signed_in {
        let name = account["name"].as_str().filter(|n| !n.is_empty()).or(account["email"].as_str()).unwrap_or("").to_string();
        let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
        let line = account["summary"].as_str().or(account["planName"].as_str()).unwrap_or("").to_string();
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().size(px(30.)).flex_none().flex().items_center().justify_center().bg(t.accent).text_color(t.text_on_accent).font_weight(FontWeight::BOLD).child(initial))
            .child(div().flex_1().min_w_0().flex().flex_col().child(div().truncate().font_weight(FontWeight::MEDIUM).child(name)).child(div().truncate().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(line)))
    } else {
        div().flex().items_center().gap(px(10.)).child(div().size(px(30.)).flex_none().flex().items_center().justify_center().border_1().border_color(t.line_strong).child(icon("user"))).child(div().flex().flex_col().child(div().font_weight(FontWeight::MEDIUM).child("Sign in to lsuite")).child(div().text_size(px(sz::XS)).text_color(t.text_3).child("AI and cloud for every app")))
    };
    div()
        .id("account-chip")
        .m(px(10.))
        .p(px(8.))
        .border_1()
        .border_color(t.line)
        .cursor_pointer()
        .hover(|d| d.bg(t.hover))
        .on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.go(crate::store::Page::Account, cx)))
        .child(body)
        .into_any_element()
}

pub fn page(ws: &Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let s = cx.store().read(cx);
    let t = cx.theme().clone();
    let account = s.account.clone();
    let plans = s.plans.clone();
    let waiting = s.signing_in.clone();
    let _ = window;
    if account.is_null() {
        return column("account-scroll", 1120., div().flex().justify_center().pt(px(80.)).text_color(t.text_3).child(spinner("account-wait")));
    }
    let signed_in = account["signedIn"] == true;
    let current = account["plan"].as_str().unwrap_or("").to_string();
    let top: AnyElement = if signed_in { signed_in_card(&account, cx) } else { sign_in_card(ws, cx.entity(), waiting, cx) };
    column(
        "account-scroll",
        1120.,
        div()
            .flex()
            .flex_col()
            .gap(px(26.))
            .child(lede(
                "One account for every lsuite app.",
                "The apps are free, with every feature, and need no account. lsuite AI is the one thing sold: agents that work in every app with nothing to set up, and cloud storage for your projects. Bring your own model instead whenever you like.",
                cx,
            ))
            .child(top)
            .child(plans_grid(&plans, signed_in.then_some(current.as_str()), cx)),
    )
}

fn sign_in_card(ws: &Workspace, entity: gpui::Entity<Workspace>, waiting: Option<String>, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let waiting_box = waiting.map(|label| {
        let url = label.split_once(": ").map(|(_, u)| u.to_string()).unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex().items_center().gap(px(8.)).child(spinner("signin-spin")).child(div().font_weight(FontWeight::MEDIUM).child("Waiting for the browser… Finish signing in there.")))
            .child(busy_bar("signin-bar", cx))
            .when(!url.is_empty(), |d| d.child(div().font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).truncate().child(url)))
            .child(div().flex().child(Button::new("signin-cancel", "Cancel").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.cancelSignIn", json!({}), cx)))))
    });
    div()
        .flex()
        .gap(px(14.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(14.))
                .p(px(20.))
                .glass(t.glass1)
                .child(div().flex().items_center().gap(px(10.)).child(icon("sparkles")).child(div().text_size(px(sz::LG)).font_weight(FontWeight::BOLD).child("No setup. Sign in and your agents work.")))
                .child(div().text_color(t.text_2).child("Signing in here signs in every lsuite app on this computer: ryolune, kimchi, zenith, nori and folio use the same account, and lsuite AI is the first provider in each of their agents."))
                .child(match waiting_box {
                    Some(w) => w.into_any_element(),
                    None => div().flex().child(Button::new("signin", "Sign in with the browser").with_icon("log-in").primary().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signIn", json!({}), cx)))).into_any_element(),
                }),
        )
        .child(
            div()
                .w(px(340.))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(20.))
                .glass(t.glass1)
                .child(div().flex().items_center().gap(px(8.)).child(icon("key-round")).child(div().font_weight(FontWeight::SEMIBOLD).child("Or use a key")))
                .child(div().text_size(px(sz::SM)).text_color(t.text_2).child("Make one on lsuite.xyz/account (Keys) and paste it here: handy on a computer without a browser."))
                .child(ws.key_input.clone())
                .child(div().flex().gap(px(8.)).child(Button::new("key-signin", "Sign in").small().on_click(move |_, _, cx| entity.update(cx, |ws, cx| ws.sign_in_with_key(cx)))).child(Button::new("key-page", "Get a key").icon_after("arrow-up-right").ghost().small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.manage", json!({}), cx))))),
        )
        .into_any_element()
}

fn signed_in_card(a: &Value, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let name = a["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("").to_string();
    let email = a["email"].as_str().unwrap_or("").to_string();
    let plan_name = a["planName"].as_str().or(a["plan"].as_str()).unwrap_or("Free").to_string();
    let free = a["plan"].as_str().is_none_or(|p| p == "free");
    let (used, limit) = (a["usage"]["used"].as_f64().unwrap_or(0.), a["usage"]["limit"].as_f64().unwrap_or(0.));
    let resets = a["usage"]["resetsAt"].as_str().and_then(|r| chrono::DateTime::parse_from_rfc3339(r).ok()).map(|d| d.format("%-d %b").to_string()).unwrap_or_default();
    let (cu, cq) = (a["cloud"]["used"].as_u64().unwrap_or(0), a["cloud"]["quota"].as_u64().unwrap_or(0));
    let models: Vec<String> = a["models"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(str::to_string)).collect();
    let offline = a["offline"] == true;
    let stat = |title: &'static str, value: String, sub: String, frac: Option<f64>| {
        div()
            .flex_1()
            .min_w(px(220.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(16.))
            .glass(t.glass1)
            .child(caps(title, cx))
            .child(div().text_size(px(sz::XL)).font_weight(FontWeight::BOLD).child(value))
            .when_some(frac, |d, f| d.child(meter(f, 0.9, cx)))
            .child(div().text_size(px(sz::SM)).text_color(t.text_2).child(sub))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .p(px(20.))
                .glass(t.glass1)
                .child(div().size(px(48.)).flex_none().flex().items_center().justify_center().bg(t.accent).text_color(t.text_on_accent).text_size(px(sz::XL)).font_weight(FontWeight::BOLD).child(name.chars().next().or(email.chars().next()).map(|c| c.to_uppercase().to_string()).unwrap_or_default()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().flex().items_center().gap(px(8.)).child(div().text_size(px(sz::LG)).font_weight(FontWeight::BOLD).child(if name.is_empty() { email.clone() } else { name.clone() })).child(tag(plan_name.clone(), !free, cx)).when(a["demo"] == true, |d| d.child(tag("demo", false, cx))))
                        .child(div().text_color(t.text_2).child(format!("{email} · every lsuite app on this computer uses this account"))),
                )
                .child(Button::new("signout", "Sign out").with_icon("log-out").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signOut", json!({}), cx)))),
        )
        .when(offline, |d| d.child(div().flex().items_center().gap(px(8.)).text_color(t.danger).child(icon("circle-alert")).child(a["error"].as_str().unwrap_or("Offline").to_string())))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(14.))
                .child(if free {
                    stat("lsuite AI", "Bring your own".into(), "On Free, the apps use the model you bring: Claude Code, Codex, an API key or a local model.".into(), None)
                } else {
                    stat("lsuite AI this month", format!("{:.0} % used", if limit > 0. { used / limit * 100. } else { 0. }), format!("{} of {} credits · resets {resets}", thousands(used.round() as u64), thousands(limit as u64)), Some(if limit > 0. { used / limit } else { 0. }))
                })
                .child(if cq > 0 {
                    stat("lsuite Cloud", format!("{} used", bytes(cu)), format!("of {}", bytes(cq)), Some(cu as f64 / cq as f64))
                } else {
                    stat("lsuite Cloud", "No storage".into(), "Cloud storage comes with Plus, Pro and Studio.".into(), None)
                })
                .child(stat("Models", if models.is_empty() { "Yours".into() } else { format!("{}", models.len()) }, if models.is_empty() { "Bring your own provider.".into() } else { models.join(", ") }, None)),
        )
        .into_any_element()
}

fn plans_grid(plans: &Value, current: Option<&str>, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let list = plans["plans"].as_array().cloned().unwrap_or_default();
    let demo = plans["demo"] == true;
    if list.is_empty() {
        return div().flex().flex_col().gap(px(10.)).child(caps("Plans", cx)).child(div().text_color(t.text_3).child("The plans couldn't be loaded. Check your connection, then refresh.")).into_any_element();
    }
    let server = lsuite_core::account::server();
    let cards: Vec<AnyElement> = list
        .iter()
        .map(|p| {
            let id = p["id"].as_str().unwrap_or("").to_string();
            let is_current = current == Some(id.as_str());
            let free = id == "free";
            let price = p["price"].as_f64().unwrap_or(0.);
            let credits = p["credits"].as_u64().unwrap_or(0);
            let storage = p["storageLabel"].as_str().map(str::to_string).or_else(|| p["storage"].as_u64().filter(|s| *s > 0).map(bytes));
            let families: Vec<String> = p["families"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_string)).collect();
            let mut lines: Vec<String> = vec![];
            if free {
                lines.push("Every feature of every app".into());
                lines.push("Your own models and keys".into());
            } else {
                lines.push(format!("{} credits a month", thousands(credits)));
                if let Some(s) = &storage {
                    lines.push(format!("{s} lsuite Cloud"));
                }
                lines.push(families.join(", "));
                if p["priority"] == true {
                    lines.push("Priority when it's busy".into());
                }
            }
            let url = format!("{server}/account/checkout?plan={id}");
            let lit = id == "pro";
            div()
                .flex_1()
                .min_w(px(220.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(16.))
                .glass(t.glass1)
                .when(lit || is_current, |d| d.border_color(t.accent))
                .child(div().flex().items_center().justify_between().child(div().text_size(px(sz::LG)).font_weight(FontWeight::BOLD).child(p["name"].as_str().unwrap_or("").to_string())).when(is_current, |d| d.child(tag("your plan", true, cx))))
                .child(div().flex().items_baseline().gap(px(4.)).child(div().text_size(px(sz::XXL)).font_weight(FontWeight::BOLD).child(format!("${price:.0}"))).when(!free, |d| d.child(div().text_color(t.text_2).child("/ month"))))
                .child(div().h(px(36.)).text_size(px(sz::SM)).text_color(t.text_2).child(p["summary"].as_str().unwrap_or("").to_string()))
                .child(div().flex().flex_col().gap(px(4.)).children(lines.into_iter().map(|l| div().flex().items_start().gap(px(6.)).text_size(px(sz::SM)).child(div().flex_none().mt(px(2.)).text_color(t.text_3).child(icon("check").size(px(12.)))).child(div().flex_1().min_w_0().child(l)))))
                .child(div().flex_1())
                .when(!is_current && !free, |d| {
                    d.child(div().flex().child(Button::new(SharedString::from(format!("choose-{id}")), format!("Choose {}", p["name"].as_str().unwrap_or(""))).full_width().variant(if lit { crate::ui::Variant::Primary } else { crate::ui::Variant::Secondary }).on_click(move |_, _, _| {
                        let _ = lsuite_core::util::open_external(&url);
                    })))
                })
                .into_any_element()
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(div().flex().items_center().gap(px(10.)).child(caps("Plans", cx)).when(demo, |d| d.child(tag("demo — no payment is taken", false, cx))))
        .child(div().flex().flex_wrap().gap(px(14.)).children(cards))
        .child(div().text_size(px(sz::SM)).text_color(t.text_3).child("Plans are chosen and paid on lsuite.xyz; the launcher shows the change as soon as you come back."))
        .into_any_element()
}

/// 12000 → "12,000".
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
