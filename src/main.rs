mod actions;
mod assets;
mod media;
mod native;
mod notify;
mod paths;
mod settings;
mod state;
mod store;
mod theme;
mod timer;
mod tray;
mod ui;
mod wallpaper;

use gpui::{
    point, prelude::*, px, size, App, Application, Bounds, Focusable, Global, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowHandle, WindowKind,
    WindowOptions,
};

use crate::{
    assets::Assets,
    theme::Theme,
    ui::{mini::MiniView, root::RootView},
};

#[derive(Default)]
struct Windows {
    main: Option<WindowHandle<RootView>>,
    mini: Option<WindowHandle<MiniView>>,
}

impl Global for Windows {}

fn background_appearance(cx: &App) -> WindowBackgroundAppearance {
    if Theme::get(cx).native_glass {
        // The native backdrop and glass views sit under GPUI's view.
        WindowBackgroundAppearance::Transparent
    } else if cfg!(target_os = "linux") {
        // Client-side decorations round the corners; an opaque surface paints them black.
        WindowBackgroundAppearance::Transparent
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// Pushes the glass setting to every open window.
pub fn sync_window_backgrounds(cx: &mut App) {
    let appearance = background_appearance(cx);
    for window in cx.windows() {
        window
            .update(cx, |_, window, _| {
                window.set_background_appearance(appearance)
            })
            .ok();
    }
}

fn open_main_window(cx: &mut App) -> Option<WindowHandle<RootView>> {
    let bounds = Bounds::centered(None, size(px(1080.), px(720.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Enki".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(18.), px(19.))),
        }),
        window_background: background_appearance(cx),
        window_min_size: Some(size(px(640.), px(500.))),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("enki".into()),
        focus: true,
        show: true,
        ..Default::default()
    };
    let model = state::model(cx);
    let handle = cx
        .open_window(options, |window, cx| {
            let view = cx.new(|cx| RootView::new(model, window, cx));
            window.focus(&view.focus_handle(cx));
            view
        })
        .map_err(|err| eprintln!("enki: could not open window: {err:#}"))
        .ok()?;
    cx.global_mut::<Windows>().main = Some(handle);
    Some(handle)
}

pub fn show_main_window(cx: &mut App) {
    let existing = cx.global::<Windows>().main;
    let alive = existing.is_some_and(|handle| {
        handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    });
    if !alive {
        open_main_window(cx);
    }
    cx.activate(true);
}

pub fn toggle_mini_window(cx: &mut App) {
    if let Some(handle) = cx.global_mut::<Windows>().mini.take() {
        if handle
            .update(cx, |_, window, _| window.remove_window())
            .is_ok()
        {
            return;
        }
    }
    let on_top = state::model(cx).read(cx).settings.mini_on_top;
    let mini_size = size(px(300.), px(84.));
    // Top-right corner of the main display, below the menu bar.
    let origin = cx
        .primary_display()
        .map(|d| {
            let b = d.bounds();
            point(
                b.origin.x + b.size.width - mini_size.width - px(24.),
                b.origin.y + px(48.),
            )
        })
        .unwrap_or(point(px(80.), px(80.)));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, mini_size))),
        titlebar: None,
        kind: if on_top {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        },
        is_resizable: false,
        is_minimizable: false,
        window_background: background_appearance(cx),
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("enki".into()),
        focus: false,
        show: true,
        ..Default::default()
    };
    let model = state::model(cx);
    match cx.open_window(options, |_, cx| cx.new(|cx| MiniView::new(model, cx))) {
        Ok(handle) => cx.global_mut::<Windows>().mini = Some(handle),
        Err(err) => eprintln!("enki: could not open mini window: {err:#}"),
    }
}

fn main() {
    // GPUI reports GPU and platform problems through `log`; RUST_LOG=info shows more.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let app = Application::new().with_assets(Assets);
    app.on_reopen(show_main_window);
    app.run(|cx: &mut App| {
        cx.set_global(Windows::default());
        ui::text_input::bind_keys(cx);
        actions::register(cx);
        state::init(cx);
        open_main_window(cx);
        #[cfg(debug_assertions)]
        if std::env::var("ENKI_DEBUG_MINI").is_ok() {
            toggle_mini_window(cx);
        }
        cx.on_window_closed(|cx| {
            // Without the tray there's no way back once every window is gone.
            let menu_bar = state::model(cx).read(cx).settings.menu_bar;
            if cx.windows().is_empty() && !menu_bar {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
