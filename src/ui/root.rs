//! The main window: title bar, sidebar and the current page.

use gpui::{
    canvas, div, img, linear_color_stop, linear_gradient, prelude::*, px, AnyElement, App, Context,
    Decorations, Entity, FocusHandle, Focusable, ImageSource, MouseButton, ObjectFit, ResizeEdge,
    SharedString, Subscription, Window, WindowControlArea,
};

use crate::{
    actions,
    assets::Icon,
    native,
    state::{Enki, Page},
    theme::{radius, with_alpha, Theme, UI_FONT},
    timer::Phase,
    ui::{
        components::StyledExt as _,
        components::{icon, icon_button, icon_sized, WindowDrag},
        on_model, settings_page, sidebar, stats_page, tasks_page,
        text_input::{TextInput, TextInputEvent},
        timer_page, wrapped_page,
    },
};

pub const TITLEBAR_HEIGHT: f32 = 52.;
/// Room left for the macOS traffic lights.
pub const TRAFFIC_LIGHTS_WIDTH: f32 = 84.;

pub struct RootView {
    pub model: Entity<Enki>,
    pub task_input: Entity<TextInput>,
    pub new_task_estimate: u32,
    pub editing_task: Option<String>,
    pub edit_task_input: Entity<TextInput>,
    pub intention_input: Entity<TextInput>,
    /// Which dropdown in the settings page is open.
    pub open_menu: Option<SharedString>,
    pub drag: WindowDrag,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl RootView {
    pub fn new(model: Entity<Enki>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let task_input = cx.new(|cx| TextInput::new("Adicionar tarefa…", cx));
        let edit_task_input = cx.new(|cx| TextInput::new("Título da tarefa…", cx));
        let intention_input = cx.new(|cx| TextInput::new("No que você vai focar?", cx));

        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |this, window, cx| {
                let appearance = window.appearance();
                this.model
                    .update(cx, |m, cx| m.set_system_appearance(appearance, cx));
            }),
            cx.subscribe(&task_input, |this, input, event, cx| {
                if let TextInputEvent::Submit = event {
                    let text = input.read(cx).text().to_string();
                    let estimate = this.new_task_estimate;
                    this.model
                        .update(cx, |m, cx| m.add_task_with_estimate(&text, estimate, cx));
                    input.update(cx, |input, cx| input.clear(cx));
                    if !text.trim().is_empty() {
                        this.new_task_estimate = 1;
                        cx.notify();
                    }
                }
            }),
            cx.subscribe(&edit_task_input, |this, input, event, cx| match event {
                TextInputEvent::Submit => {
                    if let Some(id) = this.editing_task.take() {
                        let title = input.read(cx).text().to_string();
                        this.model.update(cx, |m, cx| m.rename_task(&id, &title, cx));
                        cx.notify();
                    }
                }
                TextInputEvent::Cancel => {
                    if this.editing_task.take().is_some() {
                        cx.notify();
                    }
                }
                TextInputEvent::Changed => {}
            }),
            cx.subscribe(&intention_input, |this, input, event, cx| match event {
                TextInputEvent::Changed | TextInputEvent::Submit => {
                    let text = input.read(cx).text().to_string();
                    this.model.update(cx, |m, cx| m.set_intention(text, cx));
                }
                TextInputEvent::Cancel => {}
            }),
        ];

        Self {
            model,
            task_input,
            new_task_estimate: 1,
            editing_task: None,
            edit_task_input,
            intention_input,
            // Dev helper for screenshots of an open dropdown.
            open_menu: cfg!(debug_assertions)
                .then(|| std::env::var("ENKI_DEBUG_MENU").ok())
                .flatten()
                .map(SharedString::from),
            drag: WindowDrag::default(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn focus_task_input(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.task_input.focus_handle(cx));
    }

    pub fn start_editing_task(
        &mut self,
        id: String,
        title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_task = Some(id);
        let title = title.to_string();
        self.edit_task_input
            .update(cx, |input, cx| input.set_text(title, cx));
        window.focus(&self.edit_task_input.focus_handle(cx));
        cx.notify();
    }

    fn titlebar(
        &self,
        title: AnyElement,
        sidebar_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let model = self.model.clone();
        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let lights = cfg!(target_os = "macos");

        let sidebar_btn = (!sidebar_open).then(|| {
            icon_button(
                "titlebar-sidebar",
                Icon::PanelLeft,
                &theme,
                window,
                cx,
                on_model(&model, |m, cx| {
                    m.update_settings(cx, |s| s.sidebar_open = !s.sidebar_open)
                }),
            )
        });
        let mini_btn = icon_button(
            "titlebar-mini",
            Icon::Mini,
            &theme,
            window,
            cx,
            |_, _, cx| crate::toggle_mini_window(cx),
        );

        div()
            .id("titlebar")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(TITLEBAR_HEIGHT))
            .pr(px(12.))
            .pl(px(if !sidebar_open && lights {
                TRAFFIC_LIGHTS_WIDTH
            } else {
                20.
            }))
            .window_control_area(WindowControlArea::Drag)
            .when(client_decorated, |d| self.drag.attach(d))
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    // Only macOS has a system double-click action; elsewhere it maximizes.
                    if cfg!(target_os = "macos") {
                        window.titlebar_double_click();
                    } else {
                        window.zoom_window();
                    }
                }
            })
            .children(sidebar_btn)
            .child(title)
            .child(div().flex_1())
            .child(mini_btn)
            .when(client_decorated, |d| d.child(window_controls(&theme)))
    }

    fn page_title(&self, cx: &App) -> AnyElement {
        let theme = Theme::get(cx);
        let enki = self.model.read(cx);
        let (glyph, title): (Icon, SharedString) = match enki.page {
            Page::Timer => match enki.timer.phase() {
                Phase::Focus => (
                    Icon::Timer,
                    enki.current_task_title()
                        .map(|t| SharedString::from(t.to_string()))
                        .unwrap_or_else(|| "Sessão de foco".into()),
                ),
                Phase::ShortBreak => (Icon::Coffee, "Pausa curta".into()),
                Phase::LongBreak => (Icon::Coffee, "Pausa longa".into()),
            },
            Page::Tasks => (Icon::ListChecks, "Tarefas".into()),
            Page::Stats => (Icon::Chart, "Estatísticas".into()),
            Page::Wrapped => (Icon::Music, "Wrapped".into()),
            Page::Settings(section) => (
                Icon::Settings,
                format!("Configurações · {}", section.label()).into(),
            ),
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .text_size(px(14.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.text)
            .child(icon(glyph).text_color(theme.accent))
            .child(div().ellipsis().child(title))
            .into_any_element()
    }

    /// The wallpaper drawn by GPUI. With native glass the backdrop is a
    /// native view under the glass instead, so this draws nothing.
    fn background(&self, cx: &App) -> Option<AnyElement> {
        let theme = Theme::get(cx);
        let enki = self.model.read(cx);
        let wallpaper = enki.wallpaper.as_ref().filter(|_| !theme.native_glass)?;
        let dim = enki.settings.background_dim as f32 / 100.0;
        Some(
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .child(
                    img(ImageSource::from(wallpaper.rendered.clone()))
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                )
                .child(div().absolute().inset_0().bg(with_alpha(theme.scrim, dim)))
                .into_any_element(),
        )
    }

    /// Extra shade where text sits on the image: title bar and status row.
    fn shades(&self, cx: &App) -> Option<AnyElement> {
        let theme = Theme::get(cx);
        self.model.read(cx).wallpaper.as_ref()?;
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(120.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(with_alpha(theme.scrim, 0.35), 0.),
                            linear_color_stop(with_alpha(theme.scrim, 0.), 1.),
                        )),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(px(200.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(with_alpha(theme.scrim, 0.), 0.),
                            linear_color_stop(with_alpha(theme.scrim, 0.6), 1.),
                        )),
                )
                .into_any_element(),
        )
    }

    /// Floating error toast, visible even when the sidebar is collapsed.
    fn error_toast(&self, cx: &App) -> Option<AnyElement> {
        let message = self.model.read(cx).error.clone()?;
        let theme = Theme::get(cx);
        let model = self.model.clone();
        Some(
            div()
                .id("error-toast")
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .max_w(px(420.))
                .rounded(radius::LG)
                .bg(theme.surface)
                .border_1()
                .border_color(with_alpha(theme.danger, 0.45))
                .shadow(theme.popover_shadow())
                .overflow_hidden()
                .cursor_pointer()
                .occlude()
                .on_click(on_model(&model, |m, cx| m.dismiss_error(cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(14.))
                        .py(px(10.))
                        .bg(with_alpha(theme.danger, 0.12))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.))
                                .text_color(theme.danger)
                                .child(message),
                        )
                        .child(icon_sized(Icon::X, px(14.), theme.danger)),
                )
                .into_any_element(),
        )
    }

    /// Starts the native glass frame: backdrop under everything, or nothing.
    fn begin_glass(&self, window: &Window, cx: &App) {
        let theme = Theme::get(cx);
        let enki = self.model.read(cx);
        let backdrop = theme.native_glass.then(|| native::Backdrop {
            color: theme.window_bg,
            wallpaper: enki.wallpaper.as_ref().map(|w| w.rendered.as_path()),
            dim: with_alpha(theme.scrim, enki.settings.background_dim as f32 / 100.0),
        });
        native::glass_begin(window, theme.native_glass, backdrop);
    }
}

fn window_controls(theme: &Theme) -> impl IntoElement {
    let control = |id: &'static str, glyph: Icon, area: WindowControlArea, danger: bool| {
        let (hover, text, muted) = (
            if danger {
                theme.danger
            } else {
                theme.surface_hover
            },
            if danger { gpui::white() } else { theme.text },
            theme.text_muted,
        );
        div()
            .id(id)
            .group("window-control")
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded_full()
            .hover(move |s| s.bg(hover))
            .window_control_area(area)
            .child(
                icon(glyph)
                    .size(px(14.))
                    .text_color(muted)
                    .group_hover("window-control", move |s| s.text_color(text)),
            )
            .on_click(move |_, window, _| match area {
                WindowControlArea::Close => {
                    native::glass_cleanup(window);
                    window.remove_window();
                }
                WindowControlArea::Max => window.zoom_window(),
                WindowControlArea::Min => window.minimize_window(),
                WindowControlArea::Drag => {}
            })
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .ml(px(6.))
        .child(control(
            "win-min",
            Icon::WinMin,
            WindowControlArea::Min,
            false,
        ))
        .child(control(
            "win-max",
            Icon::WinMax,
            WindowControlArea::Max,
            false,
        ))
        .child(control(
            "win-close",
            Icon::WinClose,
            WindowControlArea::Close,
            true,
        ))
}

/// Invisible strips along the edges that start an interactive resize when
/// the compositor leaves decorations to us (GNOME on Wayland).
fn resize_handles() -> impl IntoElement {
    const EDGE: f32 = 6.;
    let handle = |id: &'static str, edge: ResizeEdge| {
        div()
            .id(id)
            .absolute()
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                window.start_window_resize(edge);
            })
    };
    div()
        .absolute()
        .inset_0()
        .child(
            handle("rz-t", ResizeEdge::Top)
                .top_0()
                .left(px(EDGE))
                .right(px(EDGE))
                .h(px(EDGE))
                .cursor_ns_resize(),
        )
        .child(
            handle("rz-b", ResizeEdge::Bottom)
                .bottom_0()
                .left(px(EDGE))
                .right(px(EDGE))
                .h(px(EDGE))
                .cursor_ns_resize(),
        )
        .child(
            handle("rz-l", ResizeEdge::Left)
                .left_0()
                .top(px(EDGE))
                .bottom(px(EDGE))
                .w(px(EDGE))
                .cursor_ew_resize(),
        )
        .child(
            handle("rz-r", ResizeEdge::Right)
                .right_0()
                .top(px(EDGE))
                .bottom(px(EDGE))
                .w(px(EDGE))
                .cursor_ew_resize(),
        )
        .child(
            handle("rz-tl", ResizeEdge::TopLeft)
                .top_0()
                .left_0()
                .size(px(EDGE * 2.))
                .cursor_nwse_resize(),
        )
        .child(
            handle("rz-tr", ResizeEdge::TopRight)
                .top_0()
                .right_0()
                .size(px(EDGE * 2.))
                .cursor_nesw_resize(),
        )
        .child(
            handle("rz-bl", ResizeEdge::BottomLeft)
                .bottom_0()
                .left_0()
                .size(px(EDGE * 2.))
                .cursor_nesw_resize(),
        )
        .child(
            handle("rz-br", ResizeEdge::BottomRight)
                .bottom_0()
                .right_0()
                .size(px(EDGE * 2.))
                .cursor_nwse_resize(),
        )
}

impl Focusable for RootView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (page, sidebar_open) = {
            let enki = self.model.read(cx);
            (enki.page, enki.settings.sidebar_open)
        };
        self.begin_glass(window, cx);
        let title = self.page_title(cx);
        let background = self.background(cx);
        let shades = self.shades(cx);
        let titlebar = self
            .titlebar(title, sidebar_open, window, cx)
            .into_any_element();
        let body: AnyElement = match page {
            Page::Timer => timer_page::render(self, window, cx),
            Page::Tasks => tasks_page::render(self, window, cx),
            Page::Stats => stats_page::render(self, window, cx),
            Page::Wrapped => wrapped_page::render(self, window, cx),
            Page::Settings(section) => settings_page::render(self, section, window, cx),
        };
        let sidebar = sidebar_open.then(|| sidebar::render(self, window, cx));
        let error_toast = self.error_toast(cx);
        // Drawn edges only while floating; maximized or tiled, the window meets the screen edge.
        let floating = matches!(
            window.window_decorations(),
            Decorations::Client { tiling } if !tiling.is_tiled()
        );

        let theme = Theme::get(cx);
        let has_background = self.model.read(cx).wallpaper.is_some();
        let liquid = theme.liquid();

        div()
            .id("root")
            .key_context("Enki")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &actions::NewTask, window, cx| {
                this.model.update(cx, |m, cx| m.navigate(Page::Tasks, cx));
                this.focus_task_input(window, cx);
            }))
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .font_family(UI_FONT)
            .text_size(px(14.))
            .text_color(theme.text)
            .when(!theme.native_glass, |d| d.bg(theme.window_bg))
            .children(background)
            // Over the whole window, or the floating sidebar's gaps show a seam.
            .children(shades)
            .children(sidebar)
            .child(
                div()
                    .id("main")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(!has_background && !theme.native_glass, |d| {
                        d.bg(theme.window_bg)
                    })
                    .when(sidebar_open && !liquid, |d| {
                        d.border_l_1().border_color(theme.border)
                    })
                    .child(titlebar)
                    .child(div().flex().flex_col().flex_1().min_h_0().child(body)),
            )
            .children(error_toast)
            .when(floating, |d| d.child(resize_handles()))
            // Painted last: hides glass views nothing placed this frame.
            .child(
                canvas(|_, _, _| {}, |_, _, window, _| native::glass_end(window))
                    .absolute()
                    .size_0(),
            )
            .when(floating, |d| d.rounded(radius::LG).overflow_hidden())
    }
}
