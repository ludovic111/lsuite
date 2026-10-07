//! lsuite: the launcher of the suite. Installs, updates, opens and removes the lsuite apps, holds
//! the lsuite AI account they share, and manages lsuite Cloud.
//!
//! The window (GPUI) is one client of `lsuite-core`'s command registry, like `lsuite-cli` and
//! `lsuite-mcp`; everything it shows comes from the commands' answers and the core's events.

// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod store;
// The lsuite UI kit, shared with the apps (copied from folio): not every piece is used here.
#[allow(dead_code)]
mod theme;
#[allow(dead_code)]
mod ui;
mod views;

use gpui::{App, AppContext as _, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, point, px, size};

fn main() {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("lsuite {}", lsuite_core::VERSION);
        return;
    }
    // `lsuite mcp …` and `lsuite cli …` run lsuite-mcp and lsuite-cli from beside this program:
    // one stable path to give an agent, even from an AppImage (whose inside moves every run).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(tool) = args.first().and_then(|a| match a.as_str() {
        "mcp" => Some("lsuite-mcp"),
        "cli" => Some("lsuite-cli"),
        _ => None,
    }) {
        let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(if cfg!(windows) { format!("{tool}.exe") } else { tool.to_string() })));
        let Some(exe) = exe.filter(|p| p.exists()) else {
            eprintln!("lsuite: {tool} isn't beside this program");
            std::process::exit(1);
        };
        let status = std::process::Command::new(exe).args(&args[1..]).status();
        std::process::exit(status.map(|s| s.code().unwrap_or(1)).unwrap_or(1));
    }
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,naga=warn,wgpu=warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).try_init();

    // Downloads, installs and the network run on Tokio; GPUI drives the window.
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(3).enable_all().thread_name("lsuite-worker").build().expect("tokio runtime");
    let launcher = lsuite_core::Launcher::new();
    // The copy the last update replaced, now that this one runs.
    lsuite_core::selfupdate::finish_pending();
    let handle = runtime.handle().clone();
    gpui_platform::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        gpui_tokio::init_from_handle(cx, handle);
        assets::load_fonts(cx);
        app::init(launcher.clone(), cx);
        open_main_window(cx);
        cx.activate(true);
        app::start(cx);
        let l = launcher.clone();
        cx.on_app_quit(move |_| {
            // A downloaded Windows installer runs once the launcher has exited.
            lsuite_core::selfupdate::apply_on_quit(&l);
            async {}
        })
        .detach();
    });
    drop(runtime);
}

/// Smallest window that keeps every area usable.
pub const WINDOW_MIN_W: f32 = 860.;
pub const WINDOW_MIN_H: f32 = 560.;

pub fn open_main_window(cx: &mut App) {
    // `LSUITE_WINDOW_SIZE=1600x1000` opens the window at that size (screenshots, tests).
    let (w, h) = std::env::var("LSUITE_WINDOW_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))))
        .unwrap_or_else(|| {
            let screen = cx.primary_display().map(|d| d.visible_bounds().size);
            let fit = |want: f32, room: Option<f32>, min: f32| room.map_or(want, |r| want.min(r * 0.92)).max(min);
            (fit(1200., screen.map(|s| f32::from(s.width)), WINDOW_MIN_W), fit(780., screen.map(|s| f32::from(s.height)), WINDOW_MIN_H))
        });
    let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
    let transparent = cx.global::<theme::Theme>().transparent;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions { title: Some("lsuite".into()), appears_transparent: true, traffic_light_position: Some(point(px(16.), px(17.))) }),
        focus: true,
        show: true,
        window_min_size: Some(size(px(WINDOW_MIN_W), px(WINDOW_MIN_H))),
        window_background: if transparent { WindowBackgroundAppearance::Blurred } else { WindowBackgroundAppearance::Opaque },
        app_id: Some("xyz.lsuite.launcher".into()),
        icon: image::load_from_memory(include_bytes!("../resources/lsuite.png")).ok().map(|i| std::sync::Arc::new(i.to_rgba8())),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| cx.new(|cx| app::Workspace::new(window, cx))).expect("couldn't open the lsuite window");
}
