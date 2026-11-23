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
    theme::{clock_font, radius, with_alpha, Theme, UI_FONT},
    ui::{
        components::{
            ease_out_cubic, glass, glass_shadow, hover_track, icon_button, icon_sized,
            liquid_highlights, StyledExt as _, WindowDrag,
        },
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
        let theme = Theme::get(cx).clone();
        // Glass over whatever is behind the window; no backdrop of our own.
        native::glass_begin(window, theme.native_glass, None);
        let (is_break, is_running, progress, clock_text, caption) = {
            let enki = self.model.read(cx);
            let timer = &enki.timer;
            (
                timer.phase().is_break(),
                timer.is_running(),
                timer.progress(Instant::now()),
                enki.clock_text(),
                enki.current_task_title()
                    .map(str::to_string)
                    .unwrap_or_else(|| enki.phase_caption()),
            )
        };
        let color = theme.phase_color(is_break);
        let client_decorated = matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        );

        let toggle_track = hover_track("mini-toggle", theme.reduce_motion, window, cx);
        let is_dark = theme.is_dark;
        let ring_track = theme.ring_track;
        let text_color = theme.text;
        let toggle_visual = toggle_track.animate(
            div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .rounded_full(),
            210,
            ease_out_cubic,
            move |d, t| {
                let tc = t.clamp(0.0, 1.0);
                d.bg(with_alpha(
                    gpui::white(),
                    if is_dark { 0.1 * tc } else { 0.55 * tc },
                ))
                .shadow(glass_shadow(is_dark, 0.0, tc))
                .when(tc > 0.01, |d| {
                    d.child(liquid_highlights(px(24.), is_dark, 0.0, tc))
                })
                .child(div().relative().child(icon_sized(
                    if is_running { Icon::Pause } else { Icon::Play },
                    px(16.),
                    text_color,
                )))
            },
        );

        let toggle = div()
            .id("mini-toggle")
            .relative()
            .flex()
            .size(px(48.))
            .rounded_full()
            .flex_none()
            .cursor_pointer()
            .on_hover(toggle_track.on_hover())
            .on_mouse_down(MouseButton::Left, |_, _, _| {
                native::haptic(native::Haptic::LevelChange)
            })
            .on_click(on_model(&self.model, |m, cx| m.toggle_timer(cx)))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(ring(progress, px(48.), px(4.), ring_track, color)),
            )
            .child(toggle_visual);

        let expand_btn = icon_button(
            "mini-expand",
            Icon::Expand,
            &theme,
            window,
            cx,
            |_, _, cx| {
                crate::show_main_window(cx);
                crate::toggle_mini_window(cx);
            },
        );
        let skip_btn = icon_button(
            "mini-skip",
            Icon::Skip,
            &theme,
            window,
            cx,
            on_model(&self.model, |m, cx| m.skip_phase(cx)),
        );

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
                    d.child(glass("mini", radius::LG, &theme))
                } else {
                    d.bg(theme.window_bg).when(client_decorated, |d| {
                        d.border_1().border_color(theme.border_strong)
                    })
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
                            .child(clock_text),
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
                    .child(expand_btn)
                    .child(skip_btn),
            )
            .child(
                canvas(|_, _, _| {}, |_, _, window, _| native::glass_end(window))
                    .absolute()
                    .size_0(),
            )
    }
}
