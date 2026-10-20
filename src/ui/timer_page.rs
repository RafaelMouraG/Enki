//! The main screen: phase switcher, ring, controls and the focus composer.

use std::time::{Duration, Instant};

use gpui::{
    div, ease_in_out, prelude::*, px, Animation, AnimationExt as _, AnyElement, Context,
    FontWeight, MouseButton, SharedString, Window,
};

use crate::{
    assets::Icon,
    native::{self, Haptic},
    theme::{clock_font, with_alpha, Theme},
    timer::Phase,
    ui::{
        components::StyledExt as _,
        components::{glass, glass_styled, icon, icon_sized, segmented, SegmentOption},
        format_minutes, on_model,
        ring::{mini_ring, ring},
        root::RootView,
    },
};

const RING: f32 = 300.;
const COMPOSER_RADIUS: gpui::Pixels = px(18.);

pub fn render(view: &mut RootView, _window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let theme = Theme::get(cx);
    let enki = model.read(cx);
    let timer = &enki.timer;
    let now = Instant::now();
    let phase = timer.phase();
    let color = theme.phase_color(phase.is_break());
    let reduce_motion = enki.settings.reduce_motion;
    let has_wallpaper = enki.wallpaper.is_some();
    let liquid = theme.liquid();

    let phases = [Phase::Focus, Phase::ShortBreak, Phase::LongBreak]
        .into_iter()
        .map(|p| SegmentOption {
            label: p.label().into(),
            icon: None,
            selected: p == phase,
            on_click: Box::new(on_model(&model, move |m, cx| m.select_phase(p, cx))),
        })
        .collect();

    let status = if timer.is_running() {
        enki.phase_caption()
    } else if timer.is_paused() {
        "Pausado".to_string()
    } else {
        match phase {
            Phase::Focus => enki.phase_caption(),
            _ => format!("{} · pronta", phase.label()),
        }
    };

    let clock = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .text_size(px(76.))
                .line_height(px(84.))
                .font(clock_font(FontWeight::LIGHT))
                .text_color(theme.text)
                .child(enki.clock_text()),
        )
        .child(
            div()
                .text_size(px(14.))
                .text_color(if timer.is_paused() {
                    theme.text_faint
                } else {
                    theme.text_muted
                })
                .child(status),
        );

    let ring_block = div()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .size(px(RING))
        .child(div().absolute().inset_0().child(ring(
            timer.progress(now),
            px(RING),
            px(7.),
            theme.ring_track,
            color,
        )))
        .child(clock);

    // Fade/scale in when the phase changes; keyed by phase so it replays.
    let ring_block: AnyElement = if reduce_motion {
        ring_block.into_any_element()
    } else {
        ring_block
            .with_animation(
                SharedString::from(format!("ring-{}", phase.key())),
                Animation::new(Duration::from_millis(380)).with_easing(ease_in_out),
                |el, t| el.opacity(0.35 + 0.65 * t),
            )
            .into_any_element()
    };

    let round = |id: &'static str, glyph: Icon, size: f32, primary: bool| {
        let (bg, fg, hover) = if primary {
            (
                theme.primary_button,
                theme.primary_button_fg,
                with_alpha(theme.primary_button, 0.88),
            )
        } else {
            (
                with_alpha(theme.surface, 0.8),
                theme.text,
                theme.surface_hover,
            )
        };
        let glassy = liquid && !primary;
        div()
            .id(id)
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .size(px(size))
            .rounded_full()
            .map(|d| {
                if glassy {
                    d.child(glass_styled(id, px(size / 2.), true, theme))
                        .hover(|s| s.bg(with_alpha(gpui::white(), 0.08)))
                } else {
                    d.bg(bg).hover(move |s| s.bg(hover))
                }
            })
            .when(!primary && !liquid, |d| {
                d.border_1().border_color(theme.border)
            })
            .when(primary, |d| d.shadow(theme.card_shadow()))
            .cursor_pointer()
            .active(|s| s.opacity(0.85))
            // A tap from the trackpad as the button goes down.
            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                native::haptic(if primary {
                    Haptic::LevelChange
                } else {
                    Haptic::Generic
                })
            })
            .child(icon_sized(glyph, px(if primary { 22. } else { 17. }), fg))
    };

    let controls = div()
        .flex()
        .items_center()
        .gap(px(20.))
        .child(
            round("timer-reset", Icon::RotateCcw, 46., false)
                .on_click(on_model(&model, |m, cx| m.reset_timer(cx))),
        )
        .child(
            round(
                "timer-toggle",
                if timer.is_running() {
                    Icon::Pause
                } else {
                    Icon::Play
                },
                64.,
                true,
            )
            .on_click(on_model(&model, |m, cx| m.toggle_timer(cx))),
        )
        .child(
            round("timer-skip", Icon::Skip, 46., false)
                .on_click(on_model(&model, |m, cx| m.skip_phase(cx))),
        );

    // The composer pill, modeled on Zeron's "Do anything…" box.
    let current_task = enki.current_task_title().map(|t| t.to_string());
    let composer = div()
        .flex()
        .flex_col()
        .w_full()
        .max_w(px(560.))
        .gap(px(8.))
        .when_some(current_task, |d, title| {
            d.child(
                div().flex().justify_center().child(
                    div()
                        .id("current-task-chip")
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .h(px(26.))
                        .pl(px(10.))
                        .pr(px(6.))
                        .rounded_full()
                        .bg(theme.accent_soft)
                        .text_color(theme.accent_text)
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(
                            icon(Icon::Target)
                                .size(px(13.))
                                .text_color(theme.accent_text),
                        )
                        .child(div().max_w(px(360.)).ellipsis().child(title))
                        .child(
                            div()
                                .id("clear-current-task")
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(18.))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.7))
                                .child(icon(Icon::X).size(px(12.)).text_color(theme.accent_text))
                                .on_click(on_model(&model, |m, cx| m.set_current_task(None, cx))),
                        ),
                ),
            )
        })
        .child(
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(52.))
                .pl(px(16.))
                .pr(px(8.))
                .rounded(COMPOSER_RADIUS)
                .map(|d| {
                    if liquid {
                        d.child(glass("composer", COMPOSER_RADIUS, theme))
                    } else {
                        d.bg(if has_wallpaper {
                            with_alpha(theme.surface, 0.86)
                        } else {
                            theme.surface
                        })
                        .border_1()
                        .border_color(theme.border_strong)
                        .shadow(theme.card_shadow())
                    }
                })
                .text_size(px(15.))
                .child(icon_sized(Icon::Sparkle, px(16.), theme.text_faint))
                .child(view.intention_input.clone())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(8.))
                        .text_size(px(13.))
                        .text_color(theme.text_muted)
                        .child(icon_sized(Icon::Clock, px(13.), theme.text_faint))
                        .child(format_minutes(timer.durations().of(phase).as_secs())),
                ),
        );

    let stats = &enki.stats;
    let today = stats.today();
    let goal = enki.settings.daily_goal.max(1);
    let goal_progress = today.pomodoros as f32 / goal as f32;
    let status_bar = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .w_full()
        .max_w(px(560.))
        .h(px(36.))
        .px(px(4.))
        .text_size(px(12.5))
        .text_color(theme.text_muted)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icon_sized(Icon::Clock, px(13.), theme.text_faint))
                        .child(format!(
                            "{} de foco hoje",
                            format_minutes(today.focus_seconds)
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(icon_sized(Icon::Flame, px(13.), theme.text_faint))
                        .child(match stats.streak {
                            0 => "Sem sequência".to_string(),
                            1 => "1 dia seguido".to_string(),
                            n => format!("{n} dias seguidos"),
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(mini_ring(goal_progress, theme.ring_track, theme.accent))
                .child(format!("{}/{goal} pomodoros", today.pomodoros)),
        );

    div()
        .id("timer-page")
        .flex()
        .flex_col()
        .items_center()
        .size_full()
        .px(px(32.))
        .pb(px(16.))
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .flex_1()
                .w_full()
                .gap(px(28.))
                .py(px(12.))
                .child(segmented("phase", theme, phases))
                .child(ring_block)
                .child(controls)
                .child(div().h(px(8.)))
                .child(composer),
        )
        .child(status_bar)
        .into_any_element()
}
