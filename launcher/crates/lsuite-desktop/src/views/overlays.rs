//! What floats above the page: dialogs, an app's "more" menu, toasts.

use gpui::{AnyElement, App, Context, FontWeight, MouseButton, Pixels, Point, Window, anchored, deferred, div, prelude::*, px};
use serde_json::json;

use crate::app::Workspace;
use crate::store::{Dialog, StoreExt, Toast, ToastKind};
use crate::theme::{ActiveTheme, size as sz};
use crate::ui::{Button, GlassExt, icon, motion};

/// Carries out the dialog's action (its button, or Enter in its field).
pub fn confirm(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let store = cx.store();
    let Some(dialog) = store.read(cx).dialog.clone() else { return };
    let text = ws.input.read(cx).text().trim().to_string();
    let (name, params) = match &dialog {
        Dialog::RemoveApp { app, .. } => ("apps.uninstall", json!({ "app": app })),
        Dialog::RemovePlugin { app, id, .. } => ("plugins.remove", json!({ "app": app, "id": id })),
        Dialog::BuildPlugin { app, .. } if !text.is_empty() => {
            let prompt = lsuite_core::plugins::build_request(app, &text);
            ws.close_dialog(window, cx);
            store.update(cx, |s, cx| {
                s.ask_agent(prompt, cx);
                s.go(crate::store::Page::Agent, cx);
            });
            return;
        }
        _ => return,
    };
    let say = match &dialog {
        Dialog::RemoveApp { name, .. } => Some(format!("{name} is removed. Its documents and settings are still there.")),
        Dialog::RemovePlugin { name, .. } => Some(format!("{name} is removed.")),
        _ => None,
    };
    ws.close_dialog(window, cx);
    store.update(cx, |s, cx| {
        s.run_then(name, params, cx, move |s, _, cx| {
            if let Some(m) = say {
                s.toast(ToastKind::Success, m, cx);
            }
        })
    });
}

pub fn dialog(ws: &Workspace, d: &Dialog, _window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let t = cx.theme().clone();
    let (title, text, button, danger, field): (String, String, &str, bool, bool) = match d {
        Dialog::RemoveApp { name, .. } => (format!("Remove {name}?"), format!("{name} is removed from this computer. Your documents, its settings and its data stay where they are; install it again any time."), "Remove", true, false),
        Dialog::RemovePlugin { app, name, .. } => (format!("Remove {name}?"), format!("Its folder is deleted from this computer, and {app} lets go of it. Build it again any time."), "Remove", true, false),
        Dialog::BuildPlugin { name, .. } => (
            format!("Build a {name} plugin"),
            format!("Say what it should do. The lsuite agent builds it with {name}'s plugin tools, checks it compiles and installs it; you follow it on the Agent page."),
            "Build",
            false,
            true,
        ),
    };
    let entity = cx.entity();
    let (e1, e2) = (entity.clone(), entity);
    let body = div()
        .id("dialog")
        .occlude()
        .relative()
        .w(px(460.))
        .flex()
        .flex_col()
        .gap(px(14.))
        .p(px(20.))
        .glass(t.glass3)
        .shadow(t.glass_shadow())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(div().flex().items_center().gap(px(8.)).when(danger, |d| d.child(icon("circle-alert").text_color(t.danger))).child(div().text_size(px(sz::LG)).font_weight(FontWeight::BOLD).child(title)))
        .child(div().text_color(t.text_2).child(text))
        .when(field, |d| d.child(ws.input.clone()))
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(Button::new("dialog-cancel", "Cancel").on_click(move |_, window, cx| e1.update(cx, |ws, cx| ws.close_dialog(window, cx))))
                .child(Button::new("dialog-ok", button).variant(if danger { crate::ui::Variant::Danger } else { crate::ui::Variant::Primary }).on_click(move |_, window, cx| e2.update(cx, |ws, cx| confirm(ws, window, cx)))),
        );
    deferred(
        div()
            .id("scrim")
            .occlude()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(t.scrim)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.store().update(cx, |s, cx| s.close_dialog(cx)))
            .child(motion::enter(body, "dialog-in", motion::BASE, (0., 8.))),
    )
    .with_priority(1)
    .into_any_element()
}

pub fn app_menu(app: &str, at: Point<Pixels>, window: &Window, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let entries = crate::views::apps::menu_entries(app, cx);
    let items = entries.into_iter().enumerate().map(|(i, (ic, label, danger, action))| {
        let color = if danger { t.danger } else { t.text };
        div()
            .id(("app-menu-item", i))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .px(px(8.))
            .text_color(color)
            .cursor_pointer()
            .hover(|s| s.bg(t.accent_soft))
            .on_click(move |_, _, cx| {
                cx.store().update(cx, |s, cx| {
                    s.menu = None;
                    cx.notify();
                });
                action(cx);
            })
            .child(icon(ic).text_color(color))
            .child(label)
    });
    let key = (f32::from(at.x) as i32 as usize).wrapping_mul(10007) ^ f32::from(at.y) as i32 as usize;
    let _ = window;
    deferred(
        anchored().position(at).snap_to_window_with_margin(px(8.)).child(motion::enter(
            div()
                .id("app-menu")
                .occlude()
                .relative()
                .min_w(px(220.))
                .p(px(4.))
                .glass(t.glass2)
                .shadow(t.glass_shadow())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(|_, _, cx| {
                    cx.store().update(cx, |s, cx| {
                        s.menu = None;
                        cx.notify();
                    })
                })
                .children(items),
            ("menu-in", key),
            motion::FAST,
            (0., -4.),
        )),
    )
    .with_priority(2)
    .into_any_element()
}

pub fn toasts(list: Vec<Toast>, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    if list.is_empty() {
        return div().into_any_element();
    }
    deferred(
        div().absolute().bottom(px(16.)).right(px(16.)).flex().flex_col().items_end().gap(px(8.)).children(list.into_iter().map(|toast| {
            let (ic, color) = match toast.kind {
                ToastKind::Error => ("circle-alert", t.danger),
                ToastKind::Success => ("circle-check", t.success),
                ToastKind::Info => ("info", t.text_2),
            };
            let id = toast.id;
            let el = div()
                .id(("toast", id as usize))
                .occlude()
                .relative()
                .max_w(px(440.))
                .flex()
                .items_start()
                .gap(px(8.))
                .px(px(12.))
                .py(px(9.))
                .glass(t.glass2)
                .shadow(t.glass_shadow())
                .cursor_pointer()
                .on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.dismiss(id, cx)))
                .child(icon(ic).text_color(color).mt(px(2.)))
                .child(div().flex_1().min_w_0().font_weight(FontWeight::MEDIUM).child(toast.text));
            motion::enter(el, ("toast-in", id as usize), motion::BASE, (18., 0.))
        })),
    )
    .with_priority(3)
    .into_any_element()
}
