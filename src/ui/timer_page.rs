//! The main screen: phase switcher, ring, controls and the music player.

use std::time::{Duration, Instant};

use gpui::{
    div, ease_in_out, prelude::*, px, Animation, AnimationExt as _, AnyElement, Context, Focusable,
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
        format_minutes, on_model, player,
        ring::{mini_ring, ring},
        root::RootView,
    },
};

const RING: f32 = 300.;
const PLAYER_RADIUS: gpui::Pixels = px(18.);

pub fn render(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let intention_focused = view.intention_input.focus_handle(cx).is_focused(window);
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

    let extend_button = (!timer.is_idle()).then(|| {
        let hover = theme.surface_hover;
        let text_color = theme.text;
        div()
            .id("timer-extend")
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .h(px(28.))
            .px(px(12.))
            .rounded_full()
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text_muted)
            .map(|d| {
                if liquid {
                    d.child(glass_styled("timer-extend", px(14.), true, theme))
                        .hover(move |s| s.bg(with_alpha(gpui::white(), 0.08)).text_color(text_color))
                } else {
                    d.bg(with_alpha(theme.surface, 0.8))
                        .border_1()
                        .border_color(theme.border)
                        .hover(move |s| s.bg(hover).text_color(text_color))
                }
            })
            .cursor_pointer()
            .active(|s| s.opacity(0.85))
            .on_mouse_down(MouseButton::Left, |_, _, _| native::haptic(Haptic::Generic))
            .on_click(on_model(&model, |m, cx| {
                m.extend_timer(Duration::from_secs(5 * 60), cx)
            }))
            .child("+5 min")
    });

    let controls_block = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.))
        .child(controls)
        .child(
            div()
                .h(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .children(extend_button),
        );

    // The current task (or a quick focus intention) and, when something is playing, the player.
    let current_task = enki.current_task_title().map(|t| t.to_string());
    let has_intention = !enki.intention.trim().is_empty();
    let intention_chip = (current_task.is_none() && phase == Phase::Focus).then(|| {
        let input = view.intention_input.clone();
        let clear_input = input.clone();
        let clear_model = model.clone();
        div().flex().justify_center().child(
            div()
                .id("session-intention")
                .relative()
                .flex()
                .items_center()
                .gap(px(8.))
                .w(px(320.))
                .h(px(34.))
                .px(px(12.))
                .rounded_full()
                .map(|d| {
                    if liquid {
                        d.child(glass_styled("session-intention", px(17.), true, theme))
                    } else {
                        d.bg(if has_wallpaper {
                            with_alpha(theme.surface, 0.8)
                        } else {
                            theme.surface
                        })
                    }
                })
                .border_1()
                .border_color(if intention_focused {
                    theme.accent
                } else if liquid {
                    gpui::transparent_black()
                } else {
                    theme.border
                })
                .text_size(px(13.))
                .text_color(theme.text)
                .child(
                    icon(Icon::Target)
                        .size(px(13.))
                        .text_color(if intention_focused || has_intention {
                            theme.accent_text
                        } else {
                            theme.text_faint
                        }),
                )
                .child(input)
                .when(has_intention, |d| {
                    d.child(
                        div()
                            .id("clear-intention")
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(18.))
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.7))
                            .child(icon(Icon::X).size(px(12.)).text_color(theme.text_muted))
                            .on_click(move |_, _, cx| {
                                clear_input.update(cx, |i, cx| i.clear(cx));
                                clear_model.update(cx, |m, cx| m.set_intention(String::new(), cx));
                            }),
                    )
                }),
        )
    });
    let below = div()
        .flex()
        .flex_col()
        .w_full()
        .max_w(px(560.))
        .gap(px(8.))
        .children(intention_chip)
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
                                .id("complete-current-task")
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(18.))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.opacity(0.7))
                                .on_mouse_down(MouseButton::Left, |_, _, _| {
                                    native::haptic(Haptic::Generic)
                                })
                                .child(
                                    icon(Icon::Check)
                                        .size(px(12.))
                                        .text_color(theme.accent_text),
                                )
                                .on_click(on_model(&model, |m, cx| m.complete_current_task(cx))),
                        )
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
        .when_some(enki.now_playing.as_ref(), |d, np| {
            // The glass goes first, under the row.
            d.child(
                div()
                    .relative()
                    .h(px(60.))
                    .rounded(PLAYER_RADIUS)
                    .map(|d| {
                        if liquid {
                            d.child(glass("player", PLAYER_RADIUS, theme))
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
                    .child(
                        player::row(np, &model, theme, true)
                            .relative()
                            .size_full()
                            .pl(px(8.))
                            .pr(px(10.)),
                    ),
            )
        });

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
                .child(controls_block)
                .child(below),
        )
        .child(status_bar)
        .into_any_element()
}
