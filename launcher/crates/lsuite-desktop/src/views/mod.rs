//! The pages (one module each) and what floats above them.

pub mod agent;
pub mod apps;
pub mod overlays;
pub mod plugins;
pub mod settings;

use gpui::{AnyElement, App, ClipboardItem, Div, FontWeight, SharedString, div, prelude::*, px};

use crate::store::{StoreExt, ToastKind};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::Button;

/// A page's scrolling column: centred, at most `max` wide.
pub fn column(id: &'static str, max: f32, body: impl IntoElement) -> AnyElement {
    div().id(id).size_full().overflow_y_scroll().child(div().max_w(px(max)).mx_auto().px(px(28.)).pt(px(22.)).pb(px(40.)).flex().flex_col().gap(px(26.)).child(body)).into_any_element()
}

/// A page's opening lines: a statement and what it means.
pub fn lede(title: impl Into<SharedString>, text: impl Into<SharedString>, cx: &App) -> Div {
    let t = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(div().text_size(px(sz::XXL)).font_weight(FontWeight::BOLD).line_height(px(sz::XXL * 1.15)).child(title.into()))
        .child(div().max_w(px(720.)).text_size(px(sz::MD)).text_color(t.text_2).child(text.into()))
}

/// A command line to copy (an MCP registration, a path).
pub fn code_line(id: impl Into<SharedString>, text: String, cx: &App) -> Div {
    let t = cx.theme();
    let id: SharedString = id.into();
    let copy = text.clone();
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .py(px(6.))
        .bg(t.bg_sunken.opacity(0.7))
        .border_1()
        .border_color(t.line)
        .child(div().flex_1().min_w_0().font_family(MONO).text_size(px(sz::SM)).truncate().child(text))
        .child(Button::icon(gpui::ElementId::Name(format!("copy-{id}").into()), "copy", "Copy").small().on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
            cx.store().update(cx, |s, cx| s.toast(ToastKind::Info, "Copied.", cx));
        }))
}

/// An empty state: a dithered fade, a title, a line and actions.
pub fn empty(title: impl Into<SharedString>, text: impl Into<SharedString>, actions: Vec<AnyElement>, window: &gpui::Window, cx: &App) -> Div {
    let t = cx.theme();
    div()
        .relative()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.))
        .px(px(24.))
        .py(px(48.))
        .overflow_hidden()
        .child(div().absolute().top_0().left_0().right_0().flex().justify_center().child(crate::ui::grain::dither(720., 70., 0.35, window, cx)))
        .child(div().text_size(px(sz::XL)).font_weight(FontWeight::BOLD).child(title.into()))
        .child(div().max_w(px(520.)).text_color(t.text_2).text_center().child(text.into()))
        .child(div().flex().gap(px(8.)).mt(px(6.)).children(actions))
}
