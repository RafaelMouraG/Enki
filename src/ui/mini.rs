//! Mini mode: a small floating window with just the clock.

use std::time::Instant;

use gpui::{
    canvas, div, prelude::*, px, App, Context, Entity, FocusHandle, Focusable, FontWeight,
    MouseButton, Subscription, Window, WindowControlArea,
};

use crate::{
    assets::Icon,
    native,
    state::Enki,
    theme::{clock_font, radius, Theme, UI_FONT},
    ui::{
        components::StyledExt as _,
        components::{glass, icon_button, icon_sized, WindowDrag},
        on_model,
        ring::ring,
    },
};

pub struct MiniView {
    model: Entity<Enki>,
    drag: WindowDrag,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl MiniView {
    pub fn new(model: Entity<Enki>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            drag: WindowDrag::default(),
            focus_handle: cx.focus_handle(),
            _subscription: subscription,
        }
    }
}

impl Focusable for MiniView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MiniView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        // Glass over whatever is behind the window; no backdrop of our own.
        native::glass_begin(window, theme.native_glass, None);
        let enki = self.model.read(cx);
        let timer = &enki.timer;
        let color = theme.phase_color(timer.phase().is_break());
        let caption = enki
            .current_task_title()
            .map(str::to_string)
            .unwrap_or_else(|| enki.phase_caption());
        let client_decorated = matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        );

        let toggle = div()
            .id("mini-toggle")
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .size(px(48.))
            .flex_none()
            .cursor_pointer()
            .child(div().absolute().inset_0().child(ring(
                timer.progress(Instant::now()),
                px(48.),
                px(4.),
                theme.ring_track,
                color,
            )))
            .child(icon_sized(
                if timer.is_running() {
                    Icon::Pause
                } else {
                    Icon::Play
                },
                px(16.),
                theme.text,
            ))
            .hover(|s| s.opacity(0.8))
            .on_mouse_down(MouseButton::Left, |_, _, _| {
                native::haptic(native::Haptic::LevelChange)
            })
            .on_click(on_model(&self.model, |m, cx| m.toggle_timer(cx)));

        div()
            .id("mini")
            .key_context("Enki")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .items_center()
            .gap(px(12.))
            .pl(px(14.))
            .pr(px(10.))
            .font_family(UI_FONT)
            .text_color(theme.text)
            .relative()
            .map(|d| {
                if theme.native_glass {
                    d.child(glass("mini", radius::LG, theme))
                } else {
                    d.bg(theme.window_bg)
                }
            })
            .rounded(radius::LG)
            .window_control_area(WindowControlArea::Drag)
            .when(client_decorated, |d| self.drag.attach(d))
            .child(toggle)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(28.))
                            .line_height(px(32.))
                            .font(clock_font(FontWeight::LIGHT))
                            .child(enki.clock_text()),
                    )
                    .child(
                        div()
                            .ellipsis()
                            .text_size(px(12.))
                            .text_color(theme.text_muted)
                            .child(caption),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(icon_button(
                        "mini-expand",
                        Icon::Expand,
                        theme,
                        |_, _, cx| {
                            crate::show_main_window(cx);
                            crate::toggle_mini_window(cx);
                        },
                    ))
                    .child(icon_button(
                        "mini-skip",
                        Icon::Skip,
                        theme,
                        on_model(&self.model, |m, cx| m.skip_phase(cx)),
                    )),
            )
            .child(
                canvas(|_, _, _| {}, |_, _, window, _| native::glass_end(window))
                    .absolute()
                    .size_0(),
            )
    }
}
