//! The main screen: phase switcher, ring, controls and the music player.

use std::time::{Duration, Instant};

use gpui::{
    div, ease_in_out, prelude::*, px, Animation, AnimationExt as _, AnyElement, Context, Focusable,
    FontWeight, MouseButton, SharedString, Window,
};

use crate::{
    assets::Icon,
    native::{self, Haptic},
    theme::{clock_font, mix_hsla, with_alpha, Theme},
    timer::Phase,
    ui::{
        components::{
            ease_out_cubic, glass, glass_shadow, glass_styled, hover_track, icon, icon_sized,
            liquid_highlights, segmented, ClickHandler, SegmentOption, StyledExt as _,
        },
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
    let theme = Theme::get(cx).clone();
    let theme = &theme;
    let now = Instant::now();
    let (
        phase,
        is_running,
        is_paused,
        is_idle,
        progress,
        clock_text,
        phase_caption,
        reduce_motion,
        has_wallpaper,
        current_task,
        has_intention,
        now_playing,
        today,
        streak,
        goal,
    ) = {
        let enki = model.read(cx);
        let timer = &enki.timer;
        (
            timer.phase(),
            timer.is_running(),
            timer.is_paused(),
            timer.is_idle(),
            timer.progress(now),
            enki.clock_text(),
            enki.phase_caption(),
            enki.settings.reduce_motion,
            enki.wallpaper.is_some(),
            enki.current_task_title().map(str::to_string),
            !enki.intention.trim().is_empty(),
            enki.now_playing.clone(),
            enki.stats.today(),
            enki.stats.streak,
            enki.settings.daily_goal.max(1),
        )
    };
    let color = theme.phase_color(phase.is_break());
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
    let phase_bar = segmented("phase", theme, window, cx, phases);

    let status = if is_running {
        phase_caption
    } else if is_paused {
        "Pausado".to_string()
    } else {
        match phase {
            Phase::Focus => phase_caption,
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
                .child(clock_text),
        )
        .child(
            div()
                .text_size(px(14.))
                .text_color(if is_paused {
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
            progress,
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

    let mut round = |id: &'static str,
                     glyph: Icon,
                     size: f32,
                     primary: bool,
                     on_click: ClickHandler|
     -> AnyElement {
        let track = hover_track(id, theme.reduce_motion, window, cx);
        let is_dark = theme.is_dark;
        let r = px(size / 2.);
        let (bg, fg, hover_bg) = if primary {
            (
                theme.primary_button,
                theme.primary_button_fg,
                mix_hsla(
                    theme.primary_button,
                    if is_dark { gpui::white() } else { gpui::black() },
                    0.12,
                ),
            )
        } else {
            (
                with_alpha(gpui::white(), if is_dark { 0.075 } else { 0.62 }),
                theme.text,
                with_alpha(gpui::white(), if is_dark { 0.16 } else { 0.86 }),
            )
        };
        let glassy = liquid && !primary;
        let border_rest = if primary {
            with_alpha(gpui::white(), if is_dark { 0.25 } else { 0.12 })
        } else if is_dark {
            with_alpha(gpui::white(), 0.16)
        } else {
            with_alpha(gpui::black(), 0.1)
        };
        let border_hover = if primary {
            with_alpha(gpui::white(), if is_dark { 0.55 } else { 0.28 })
        } else if is_dark {
            with_alpha(gpui::white(), 0.38)
        } else {
            with_alpha(gpui::black(), 0.18)
        };
        let icon_size = px(if primary { 22. } else { 17. });

        // Visual-only hover layer inside a stationary outer button:
        // rock-steady glass/icon (zero jitter) and reliable single-click hitbox.
        let visual = track.animate(
            div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .rounded_full()
                .border_1(),
            230,
            ease_out_cubic,
            move |d, t| {
                let tc = t.clamp(0.0, 1.0);
                d.map(|d| {
                    if glassy {
                        d.bg(with_alpha(
                            gpui::white(),
                            if is_dark {
                                0.03 + 0.12 * tc
                            } else {
                                0.22 + 0.36 * tc
                            },
                        ))
                    } else {
                        d.bg(mix_hsla(bg, hover_bg, tc))
                    }
                })
                .border_color(mix_hsla(border_rest, border_hover, tc))
                .shadow(glass_shadow(is_dark, if primary { 0.55 } else { 0.28 }, tc))
                .when(!primary || is_dark, |d| {
                    d.child(liquid_highlights(
                        r,
                        is_dark,
                        if primary { 0.3 } else { 0.85 },
                        tc,
                    ))
                })
                .child(div().relative().child(icon_sized(glyph, icon_size, fg)))
            },
        );

        div()
            .id(id)
            .relative()
            .flex()
            .flex_none()
            .size(px(size))
            .rounded_full()
            .cursor_pointer()
            .active(|s| s.opacity(0.84))
            .on_hover(track.on_hover())
            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                native::haptic(if primary {
                    Haptic::LevelChange
                } else {
                    Haptic::Generic
                })
            })
            .on_click(on_click)
            .when(glassy, |d| d.child(glass_styled(id, r, true, theme)))
            .child(visual)
            .into_any_element()
    };

    let reset_btn = round(
        "timer-reset",
        Icon::RotateCcw,
        46.,
        false,
        Box::new(on_model(&model, |m, cx| m.reset_timer(cx))),
    );
    let toggle_btn = round(
        "timer-toggle",
        if is_running { Icon::Pause } else { Icon::Play },
        64.,
        true,
        Box::new(on_model(&model, |m, cx| m.toggle_timer(cx))),
    );
    let skip_btn = round(
        "timer-skip",
        Icon::Skip,
        46.,
        false,
        Box::new(on_model(&model, |m, cx| m.skip_phase(cx))),
    );

    let controls = div()
        .flex()
        .items_center()
        .gap(px(20.))
        .child(reset_btn)
        .child(toggle_btn)
        .child(skip_btn);

    let extend_button = (!is_idle).then(|| {
        let track = hover_track("timer-extend", theme.reduce_motion, window, cx);
        let is_dark = theme.is_dark;
        let rest_bg = if liquid {
            with_alpha(gpui::white(), if is_dark { 0.04 } else { 0.28 })
        } else {
            with_alpha(gpui::white(), if is_dark { 0.08 } else { 0.65 })
        };
        let hover_bg = if liquid {
            with_alpha(gpui::white(), if is_dark { 0.14 } else { 0.58 })
        } else {
            with_alpha(gpui::white(), if is_dark { 0.16 } else { 0.88 })
        };
        let rest_border = with_alpha(
            if is_dark {
                gpui::white()
            } else {
                gpui::black()
            },
            if is_dark { 0.16 } else { 0.1 },
        );
        let hover_border = with_alpha(
            if is_dark {
                gpui::white()
            } else {
                gpui::black()
            },
            if is_dark { 0.36 } else { 0.18 },
        );
        let muted = theme.text_muted;
        let text_color = theme.text;

        let visual = track.animate(
            div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .h_full()
                .px(px(14.))
                .rounded_full()
                .border_1()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM),
            230,
            ease_out_cubic,
            move |d, t| {
                let tc = t.clamp(0.0, 1.0);
                d.bg(mix_hsla(rest_bg, hover_bg, tc))
                    .border_color(mix_hsla(rest_border, hover_border, tc))
                    .text_color(mix_hsla(muted, text_color, tc))
                    .shadow(glass_shadow(is_dark, 0.25, tc))
                    .child(liquid_highlights(px(15.), is_dark, 0.85, tc))
                    .child(div().relative().child("+5 min"))
            },
        );

        div()
            .id("timer-extend")
            .relative()
            .flex()
            .h(px(30.))
            .rounded_full()
            .cursor_pointer()
            .active(|s| s.opacity(0.84))
            .on_hover(track.on_hover())
            .on_mouse_down(MouseButton::Left, |_, _, _| native::haptic(Haptic::Generic))
            .on_click(on_model(&model, |m, cx| {
                m.extend_timer(Duration::from_secs(5 * 60), cx)
            }))
            .when(liquid, |d| {
                d.child(glass_styled("timer-extend", px(15.), true, theme))
            })
            .child(visual)
            .into_any_element()
    });

    let controls_block = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.))
        .child(controls)
        .child(
            div()
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .children(extend_button),
        );

    // The current task (or a quick focus intention) and, when something is playing, the player.
    let player_card = now_playing.as_ref().map(|np| {
        let row = player::row(np, &model, theme, true, window, cx)
            .relative()
            .size_full()
            .pl(px(8.))
            .pr(px(10.));
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
            .child(row)
    });

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
        .children(player_card);

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
                        .child(match streak {
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
                .child(phase_bar)
                .child(ring_block)
                .child(controls_block)
                .child(below),
        )
        .child(status_bar)
        .into_any_element()
}
