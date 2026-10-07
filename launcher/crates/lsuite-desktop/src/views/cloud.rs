//! Cloud: lsuite Cloud, the storage that comes with lsuite Pass. A folder at a time:
//! upload files or folders (or drop them on the window), download, rename, move, delete.

use std::path::PathBuf;

use gpui::{AnyElement, App, FontWeight, PathPromptOptions, SharedString, Window, div, prelude::*, px};
use lsuite_core::cloud::{join, parent_of};
use lsuite_core::util::bytes;
use serde_json::{Value, json};

use crate::store::{Dialog, Page, StoreExt, ToastKind};
use crate::theme::{ActiveTheme, MONO, size as sz};
use crate::ui::{Button, GlassExt, caps, group, icon, meter, spinner, tag, tool};
use crate::views::{column, empty, when};

pub fn actions(cx: &App) -> AnyElement {
    let s = cx.store().read(cx);
    if !s.signed_in() {
        return div().into_any_element();
    }
    let loading = s.cloud_loading;
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .when(loading, |d| d.child(div().text_color(cx.theme().text_3).child(spinner("cloud-spin"))))
        .child(group(
            [
                tool("up-files", "file-up", "Upload files", true, "Upload files into this folder").on_click(|_, _, cx| pick_upload(false, cx)).into_any_element(),
                tool("up-folder", "folder-up", "Upload folder", true, "Upload a whole folder into this folder").on_click(|_, _, cx| pick_upload(true, cx)).into_any_element(),
                tool("new-folder", "folder-plus", "New folder", true, "Create a folder here").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.open_dialog(Dialog::NewFolder, cx))).into_any_element(),
            ],
            cx,
        ))
        .child(Button::new("sync-folder", "Sync a folder").with_icon("refresh-cw").small().tooltip("Keep a folder on this computer in step with your cloud, both ways").on_click(|_, _, cx| pick_sync(cx)))
        .child(Button::icon("cloud-refresh", "refresh-cw", "Refresh").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.refresh_cloud(cx))))
        .into_any_element()
}

/// `LSUITE_NO_PICKER=1`: never open the system's file picker (headless sessions, where it can't
/// show): uploads ask for a path, downloads go to Downloads.
fn no_picker() -> bool {
    std::env::var("LSUITE_NO_PICKER").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Asks for files or a folder, then uploads them into the folder shown. Without a system file
/// picker (some Linux desktops), asks for a path instead.
pub fn pick_upload(folders: bool, cx: &mut App) {
    if no_picker() {
        return cx.store().update(cx, |s, cx| s.open_dialog(Dialog::UploadPath, cx));
    }
    let rx = cx.prompt_for_paths(PathPromptOptions { files: !folders, directories: folders, multiple: true, prompt: Some("Upload".into()) });
    let store = cx.store();
    cx.spawn(async move |cx| {
        let picked = rx.await;
        store
            .update(cx, |s, cx| match picked {
                Ok(Ok(Some(paths))) => {
                    let into = s.cloud_dir.clone();
                    for p in paths {
                        s.run("cloud.upload", json!({ "source": p, "into": into }), cx);
                    }
                }
                Ok(Ok(None)) => {}
                _ => s.open_dialog(Dialog::UploadPath, cx),
            });
    })
    .detach();
}

/// Asks for a folder on this computer and syncs it with a cloud folder of the same name, in the
/// folder shown.
pub fn pick_sync(cx: &mut App) {
    if no_picker() {
        return cx.store().update(cx, |s, cx| s.open_dialog(Dialog::SyncPath, cx));
    }
    let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Sync".into()) });
    let store = cx.store();
    cx.spawn(async move |cx| {
        let picked = rx.await;
        store.update(cx, |s, cx| match picked {
            Ok(Ok(Some(mut p))) => {
                if let Some(local) = p.pop() {
                    start_sync(s, local.display().to_string(), cx);
                }
            }
            Ok(Ok(None)) => {}
            _ => s.open_dialog(Dialog::SyncPath, cx),
        });
    })
    .detach();
}

/// `cloud.syncAdd` for `local`, into the cloud folder shown.
pub fn start_sync(s: &mut crate::store::Store, local: String, cx: &mut gpui::Context<crate::store::Store>) {
    let name = std::path::Path::new(&local).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let remote = join(&s.cloud_dir, &name);
    s.run_then("cloud.syncAdd", json!({ "local": local, "remote": remote }), cx, move |s, v, cx| {
        let line = v["firstSync"]["summary"].as_str().or(v["firstSync"]["error"].as_str()).unwrap_or("").to_string();
        s.toast(ToastKind::Success, format!("{name} is synced with {remote}. {line}"), cx);
    });
}

/// Asks where to put a download (Downloads when there's no picker), then downloads.
pub fn pick_download(path: String, cx: &mut App) {
    if no_picker() {
        let into = dirs::download_dir().or_else(dirs::home_dir);
        return cx.store().update(cx, |s, cx| s.run("cloud.download", json!({ "path": path, "into": into }), cx));
    }
    let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Download here".into()) });
    let store = cx.store();
    cx.spawn(async move |cx| {
        let picked = rx.await;
        store
            .update(cx, |s, cx| {
                let into: Option<PathBuf> = match picked {
                    Ok(Ok(Some(mut p))) => p.pop(),
                    Ok(Ok(None)) => return,
                    _ => {
                        let d = dirs::download_dir().or_else(dirs::home_dir);
                        if let Some(d) = &d {
                            s.toast(ToastKind::Info, format!("Saving to {}.", d.display()), cx);
                        }
                        d
                    }
                };
                if let Some(into) = into {
                    s.run("cloud.download", json!({ "path": path, "into": into }), cx);
                }
            });
    })
    .detach();
}

pub fn page(window: &mut Window, cx: &mut App) -> AnyElement {
    let s = cx.store().read(cx);
    let t = cx.theme().clone();
    if s.account.is_null() {
        return column("cloud-scroll", 1120., div().flex().justify_center().pt(px(80.)).text_color(t.text_3).child(spinner("cloud-wait")));
    }
    if !s.signed_in() {
        return column(
            "cloud-scroll",
            1120.,
            empty(
                "lsuite Cloud comes with lsuite Pass",
                "Every lsuite Pass plan includes cloud storage for your projects: put them there from any computer, and take them back whenever you like. Your files stay plain files; the apps never need the cloud.",
                vec![
                    Button::new("cloud-signin", "Sign in").with_icon("log-in").primary().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("account.signIn", json!({}), cx))).into_any_element(),
                    Button::new("cloud-plans", "See the plans").on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.go(Page::Account, cx))).into_any_element(),
                ],
                window,
                cx,
            )
            .glass(t.glass1),
        );
    }
    let status = s.cloud_status.clone();
    let error = s.cloud_error.clone();
    let dir = s.cloud_dir.clone();
    let selected = s.cloud_selected.clone();
    let view = s.cloud_view();
    let tasks = s.tasks.clone();
    let syncs = s.syncs["pairs"].as_array().cloned().unwrap_or_default();
    let (used, quota) = (status["used"].as_u64().unwrap_or(0), status["quota"].as_u64().unwrap_or(0));
    let demo = status["demo"] == true;
    let plan = status["planName"].as_str().unwrap_or("").to_string();

    let usage = div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(14.))
        .glass(t.glass1)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(icon("hard-drive").text_color(t.text_2))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(if quota > 0 { format!("{} of {} used", bytes(used), bytes(quota)) } else { format!("{} used", bytes(used)) }))
                .child(div().flex_1())
                .when(!plan.is_empty(), |d| d.child(tag(plan.clone(), false, cx)))
                .when(demo, |d| d.child(tag("demo", false, cx))),
        )
        .when(quota > 0, |d| d.child(meter(used as f64 / quota as f64, 0.9, cx)))
        .when(quota == 0 && status.is_object(), |d| {
            d.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(div().text_color(t.text_2).child("Your plan has no cloud storage. Plus includes 50 GB, Pro 250 GB, Studio 1 TB. Files already here can still be downloaded and deleted."))
                    .child(Button::new("cloud-choose", "Choose a plan").primary().small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.go(Page::Account, cx)))),
            )
        })
        .when(demo && quota > 0, |d| {
            let max_file = status["maxFile"].as_u64().unwrap_or(0);
            d.child(div().text_size(px(sz::SM)).text_color(t.text_3).child(format!("Demo: {} per account and {} per file while lsuite Pass takes no payments.", bytes(quota), bytes(max_file))))
        });

    // The path: Cloud › Projects › My Project, each part a link.
    let mut crumbs: Vec<AnyElement> = vec![crumb("crumb-root", "Cloud", String::new(), dir.is_empty(), cx)];
    let mut acc = String::new();
    for (i, part) in dir.split('/').filter(|p| !p.is_empty()).enumerate() {
        acc = join(&acc, part);
        crumbs.push(div().text_color(t.text_3).child(icon("chevron-right")).into_any_element());
        crumbs.push(crumb(SharedString::from(format!("crumb-{i}")), part, acc.clone(), acc == dir, cx));
    }

    let list: AnyElement = match (&view, &error) {
        (_, Some(e)) => div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .p(px(14.))
            .glass(t.glass1)
            .child(div().flex().items_center().gap(px(8.)).text_color(t.danger).child(icon("circle-alert")).child(e.clone()))
            .child(Button::new("cloud-retry", "Try again").small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.refresh_cloud(cx))))
            .into_any_element(),
        (None, None) => div().flex().justify_center().py(px(40.)).text_color(t.text_3).child(spinner("cloud-list-wait")).into_any_element(),
        (Some(v), None) => listing(v, &dir, selected.as_deref(), quota > 0, window, cx),
    };
    let transfers: Vec<AnyElement> = tasks
        .iter()
        .filter(|(k, _)| k.starts_with("upload:") || k.starts_with("download:"))
        .map(|(k, task)| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(icon(if k.starts_with("upload:") { "upload" } else { "download" }).text_color(t.text_2))
                .child(div().w(px(260.)).truncate().child(task.label.clone()))
                .child(div().flex_1().child(match task.fraction() {
                    Some(f) => meter(f, 2., cx).into_any_element(),
                    None => crate::ui::busy_bar(gpui::ElementId::Name(format!("xfer-{k}").into()), cx),
                }))
                .child(div().w(px(120.)).font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(match task.total {
                    Some(total) => format!("{} / {}", bytes(task.done), bytes(total)),
                    None => bytes(task.done),
                }))
                .into_any_element()
        })
        .collect();

    column(
        "cloud-scroll",
        1120.,
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(usage)
            .when(!syncs.is_empty(), |d| d.child(synced(&syncs, &tasks, cx)))
            .when(!transfers.is_empty(), |d| d.child(div().flex().flex_col().gap(px(8.)).p(px(14.)).glass(t.glass1).child(caps("Transfers", cx)).children(transfers)))
            .child(div().flex().items_center().gap(px(4.)).h(px(28.)).children(crumbs).child(div().flex_1()).when(!dir.is_empty(), |d| {
                let up = parent_of(&dir).to_string();
                d.child(Button::new("cloud-up", "Up").with_icon("arrow-left").ghost().small().on_click(move |_, _, cx| {
                    let up = up.clone();
                    cx.store().update(cx, |s, cx| s.open_folder(up, cx))
                }))
            }))
            .child(list),
    )
}

/// The folders kept in step with the cloud.
fn synced(pairs: &[Value], tasks: &std::collections::BTreeMap<String, crate::store::TaskView>, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(14.))
        .glass(t.glass1)
        .child(div().flex().items_center().justify_between().child(caps("Synced folders", cx)).child(Button::new("sync-all", "Sync now").with_icon("refresh-cw").ghost().small().on_click(|_, _, cx| cx.store().update(cx, |s, cx| s.run("cloud.syncNow", json!({}), cx)))))
        .children(pairs.iter().enumerate().map(|(i, p)| {
            let id = p["id"].as_str().unwrap_or("").to_string();
            let (i1, i2) = (id.clone(), id.clone());
            let running = p["syncing"] == true || tasks.contains_key(&format!("sync:{id}"));
            let present = p["present"] == true;
            let ok = p["lastOk"] == true;
            let line = match (p["lastSync"].as_str(), p["lastResult"].as_str()) {
                _ if running => tasks.get(&format!("sync:{id}")).map(|t| t.label.clone()).unwrap_or_else(|| "Syncing…".into()),
                _ if !present => "The folder isn't there (a disk not connected?)".to_string(),
                (Some(at), Some(r)) => format!("{} · {r}", when(at)),
                _ => "Not synced yet".to_string(),
            };
            let remote = p["remote"].as_str().unwrap_or("").to_string();
            let r2 = remote.clone();
            div()
                .id(("sync-row", i))
                .flex()
                .items_center()
                .gap(px(10.))
                .py(px(4.))
                .child(div().text_color(t.text_2).child(if running { spinner(gpui::ElementId::Name(format!("sync-spin-{id}").into())) } else { icon("refresh-cw").into_any_element() }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().flex().items_center().gap(px(6.)).child(div().truncate().font_family(MONO).text_size(px(sz::SM)).child(crate::app::tilde(std::path::Path::new(p["local"].as_str().unwrap_or(""))))).child(div().text_color(t.text_3).child("⇄")).child(div().id(("sync-remote", i)).cursor_pointer().font_weight(FontWeight::SEMIBOLD).hover(|d| d.text_color(t.text_2)).child(remote.clone()).on_click(move |_, _, cx| {
                            let r = r2.clone();
                            cx.store().update(cx, |s, cx| s.open_folder(r, cx))
                        })))
                        .child(div().text_size(px(sz::XS)).text_color(if ok || running || p["lastSync"].is_null() { t.text_3 } else { t.danger }).truncate().child(line)),
                )
                .child(Button::icon(gpui::ElementId::Name(format!("sync-now-{id}").into()), "refresh-cw", "Sync this folder now").small().disabled(running).on_click(move |_, _, cx| cx.store().update(cx, |s, cx| s.run("cloud.syncNow", json!({ "id": i1 }), cx))))
                .child(Button::icon(gpui::ElementId::Name(format!("sync-stop-{id}").into()), "x", "Stop syncing (the files stay on both sides)").small().on_click(move |_, _, cx| {
                    let id = i2.clone();
                    cx.store().update(cx, |s, cx| s.run_then("cloud.syncRemove", json!({ "id": id }), cx, |s, _, cx| s.toast(ToastKind::Info, "That folder isn't synced any more. Its files stay on both sides.", cx)))
                }))
                .into_any_element()
        }))
        .into_any_element()
}

fn crumb(id: impl Into<SharedString>, label: &str, path: String, current: bool, cx: &App) -> AnyElement {
    let t = cx.theme();
    div()
        .id(gpui::ElementId::Name(id.into()))
        .px(px(6.))
        .py(px(2.))
        .text_size(px(sz::MD))
        .when(current, |d| d.font_weight(FontWeight::SEMIBOLD))
        .when(!current, |d| d.text_color(t.text_2).cursor_pointer().hover(|d| d.bg(t.hover).text_color(t.text)))
        .child(label.to_string())
        .on_click(move |_, _, cx| {
            let p = path.clone();
            cx.store().update(cx, |s, cx| s.open_folder(p, cx))
        })
        .into_any_element()
}

fn listing(v: &Value, dir: &str, selected: Option<&str>, can_write: bool, window: &Window, cx: &App) -> AnyElement {
    let t = cx.theme().clone();
    let folders = v["folders"].as_array().cloned().unwrap_or_default();
    let files = v["files"].as_array().cloned().unwrap_or_default();
    if folders.is_empty() && files.is_empty() {
        let actions = if can_write {
            vec![
                Button::new("empty-up", "Upload files").with_icon("file-up").primary().on_click(|_, _, cx| pick_upload(false, cx)).into_any_element(),
                Button::new("empty-folder", "Upload a folder").with_icon("folder-up").on_click(|_, _, cx| pick_upload(true, cx)).into_any_element(),
            ]
        } else {
            vec![]
        };
        return empty(
            if dir.is_empty() { "Your cloud is empty" } else { "This folder is empty" },
            "Drop files or folders on the window, or upload them. They keep their names, and come back exactly as they went.",
            actions,
            window,
            cx,
        )
        .glass(t.glass1)
        .into_any_element();
    }
    let header = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .h(px(30.))
        .border_b_1()
        .border_color(t.line)
        .child(div().w(px(16.)))
        .child(div().flex_1().child(caps("Name", cx)))
        .child(div().w(px(130.)).child(caps("Size", cx)))
        .child(div().w(px(150.)).child(caps("Modified", cx)))
        .child(div().w(px(132.)));
    let mut rows: Vec<AnyElement> = vec![];
    for (i, f) in folders.iter().enumerate() {
        let path = f["path"].as_str().unwrap_or("").to_string();
        let count = f["files"].as_u64().unwrap_or(0);
        let size = if count == 0 { "empty".to_string() } else { format!("{count} file{} · {}", if count == 1 { "" } else { "s" }, bytes(f["size"].as_u64().unwrap_or(0))) };
        rows.push(row(("folder", i), "folder", f["name"].as_str().unwrap_or(""), size, String::new(), path, true, selected, &t));
    }
    for (i, f) in files.iter().enumerate() {
        let path = f["path"].as_str().unwrap_or("").to_string();
        let name = lsuite_core::cloud::name_of(&path).to_string();
        rows.push(row(("file", i), "file", &name, bytes(f["size"].as_u64().unwrap_or(0)), f["modifiedAt"].as_str().map(when).unwrap_or_default(), path, false, selected, &t));
    }
    div().flex().flex_col().glass(t.glass1).child(header).children(rows).into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn row(id: (&'static str, usize), ic: &'static str, name: &str, size: String, modified: String, path: String, folder: bool, selected: Option<&str>, t: &crate::theme::Theme) -> AnyElement {
    let on = selected == Some(path.as_str());
    let (p_open, p_dl, p_ren, p_mv, p_del) = (path.clone(), path.clone(), path.clone(), path.clone(), path.clone());
    let idx = id.1;
    let action = |name: &'static str, ic: &'static str, tip: &'static str| Button::icon(gpui::ElementId::Name(format!("{}-{name}-{idx}", id.0).into()), ic, tip).small();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(14.))
        .h(px(40.))
        .border_b_1()
        .border_color(t.line)
        .cursor_pointer()
        .when(on, |d| d.bg(t.accent_soft))
        .when(!on, |d| d.hover(|d| d.bg(t.hover)))
        .on_click(move |e, _, cx| {
            let p = p_open.clone();
            cx.store().update(cx, |s, cx| {
                if folder && (e.click_count() >= 2 || s.cloud_selected.as_deref() == Some(p.as_str())) {
                    s.open_folder(p, cx);
                } else {
                    s.cloud_selected = Some(p);
                    cx.notify();
                }
            })
        })
        .child(icon(ic).text_color(if folder { t.text } else { t.text_2 }))
        .child(div().flex_1().min_w_0().truncate().font_weight(if folder { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }).child(name.to_string()))
        .child(div().w(px(130.)).font_family(MONO).text_size(px(sz::XS)).text_color(t.text_2).child(size))
        .child(div().w(px(150.)).font_family(MONO).text_size(px(sz::XS)).text_color(t.text_3).child(modified))
        .child(
            div()
                .w(px(132.))
                .flex()
                .justify_end()
                .gap(px(2.))
                .child(action("dl", "download", "Download").on_click(move |_, _, cx| pick_download(p_dl.clone(), cx)))
                .child(action("ren", "pencil", "Rename").on_click(move |_, _, cx| {
                    let path = p_ren.clone();
                    cx.store().update(cx, |s, cx| s.open_dialog(Dialog::Rename { path }, cx))
                }))
                .child(action("mv", "folder-input", "Move to another folder").on_click(move |_, _, cx| {
                    let path = p_mv.clone();
                    cx.store().update(cx, |s, cx| s.open_dialog(Dialog::MoveTo { path }, cx))
                }))
                .child(action("del", "trash-2", "Delete").on_click(move |_, _, cx| {
                    let path = p_del.clone();
                    cx.store().update(cx, |s, cx| s.open_dialog(Dialog::DeleteCloud { path, folder }, cx))
                })),
        )
        .into_any_element()
}
