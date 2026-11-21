//! Left column: navigation, task list, now playing and the footer.

use gpui::{
    div, prelude::*, px, AnyElement, App, ClickEvent, Context, Decorations, FontWeight,
    SharedString, Window,
};

use crate::{
    assets::Icon,
    state::{Page, SettingsSection},
    theme::{mix_hsla, radius, sidebar_width, with_alpha, Theme},
    ui::{
        components::{
            ease_out_cubic, glass, glass_shadow, hover_track, icon, icon_button, icon_button_sized,
            icon_sized, liquid_highlights, StyledExt as _,
        },
        on_model, player,
        root::{RootView, TITLEBAR_HEIGHT, TRAFFIC_LIGHTS_WIDTH},
    },
};

const FLOAT_INSET: f32 = 8.;
const FLOAT_RADIUS: gpui::Pixels = px(18.);

pub fn render(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let theme = Theme::get(cx).clone();
    let in_settings = matches!(model.read(cx).page, Page::Settings(_));
    // The liquid sidebar floats inset from the window edges.
    let inset = if theme.liquid() { FLOAT_INSET } else { 0. };
    let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });

    let toggle_btn = icon_button(
        "sidebar-toggle",
        Icon::PanelLeft,
        &theme,
        window,
        cx,
        on_model(&model, |m, cx| {
            m.update_settings(cx, |s| s.sidebar_open = false)
        }),
    );

    let header = div()
        .flex()
        .flex_none()
        .items_center()
        // Inset at the top and the bottom so it centers on the title bar's row.
        .h(px(TITLEBAR_HEIGHT - 2. * inset))
        .pl(px(if cfg!(target_os = "macos") {
            TRAFFIC_LIGHTS_WIDTH - inset
        } else {
            12.
        }))
        .pr(px(10.))
        .window_control_area(gpui::WindowControlArea::Drag)
        .when(client_decorated, |d| view.drag.attach(d))
        .child(toggle_btn)
        .child(div().flex_1());

    let body = if in_settings {
        settings_nav(view, window, cx)
    } else {
        main_nav(view, window, cx)
    };

    div()
        .id("sidebar")
        .relative()
        .flex()
        .flex_col()
        .flex_none()
        .map(|d| {
            if theme.liquid() {
                // The same gap on every side, so it doesn't lean on the content.
                d.w(sidebar_width() - px(2. * inset))
                    .m(px(inset))
                    .child(glass("sidebar", FLOAT_RADIUS, &theme))
            } else {
                d.w(sidebar_width()).h_full().bg(theme.sidebar_bg)
            }
        })
        .child(header)
        .child(body)
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn nav_item(
    id: impl Into<SharedString>,
    glyph: Icon,
    label: impl Into<SharedString>,
    trailing: Option<SharedString>,
    selected: bool,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let label: SharedString = label.into();
    let track = hover_track(id.clone(), theme.reduce_motion, window, cx);

    let text = theme.text;
    let muted = theme.text_muted;
    let faint = theme.text_faint;
    let is_dark = theme.is_dark;
    let selected_bg = theme.selected_bg;
    let hover_bg = with_alpha(gpui::white(), if is_dark { 0.1 } else { 0.65 });
    let rest_bg = with_alpha(gpui::white(), 0.0);
    let rest_border = with_alpha(
        if is_dark {
            gpui::white()
        } else {
            gpui::black()
        },
        0.0,
    );
    let hover_border = with_alpha(
        if is_dark {
            gpui::white()
        } else {
            gpui::black()
        },
        if is_dark { 0.2 } else { 0.1 },
    );

    if selected {
        return div()
            .id(id)
            .relative()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(34.))
            .px(px(12.))
            .rounded_full()
            .border_1()
            .cursor_pointer()
            .text_size(px(14.))
            .bg(selected_bg)
            .border_color(with_alpha(
                if is_dark {
                    gpui::white()
                } else {
                    gpui::black()
                },
                if is_dark { 0.18 } else { 0.08 },
            ))
            .text_color(text)
            .font_weight(FontWeight::MEDIUM)
            .shadow(glass_shadow(is_dark, 0.2, 0.0))
            .on_click(on_click)
            .child(liquid_highlights(px(17.), is_dark, 0.75, 0.0))
            .child(div().relative().child(icon(glyph).text_color(text)))
            .child(div().relative().flex_1().min_w_0().ellipsis().child(label))
            .when_some(trailing, |d, t| {
                d.child(
                    div()
                        .relative()
                        .text_size(px(12.))
                        .text_color(muted)
                        .font(crate::theme::tabular_font())
                        .child(t),
                )
            })
            .into_any_element();
    }

    let visual = track.animate(
        div()
            .relative()
            .flex()
            .items_center()
            .gap(px(10.))
            .size_full()
            .px(px(12.))
            .rounded_full()
            .border_1()
            .text_size(px(14.)),
        220,
        ease_out_cubic,
        move |d, t| {
            let tc = t.clamp(0.0, 1.0);
            let glyph_color = mix_hsla(muted, text, tc);
            d.bg(mix_hsla(rest_bg, hover_bg, tc))
                .border_color(mix_hsla(rest_border, hover_border, tc))
                .text_color(text)
                .shadow(glass_shadow(is_dark, 0.0, tc * 0.7))
                .when(tc > 0.01, |d| {
                    d.child(liquid_highlights(px(17.), is_dark, 0.0, tc * 0.85))
                })
                .child(div().relative().child(icon(glyph).text_color(glyph_color)))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .ellipsis()
                        .child(label.clone()),
                )
                .when_some(trailing.clone(), |d, trail| {
                    d.child(
                        div()
                            .relative()
                            .text_size(px(12.))
                            .text_color(mix_hsla(faint, muted, tc))
                            .font(crate::theme::tabular_font())
                            .child(trail),
                    )
                })
        },
    );

    div()
        .id(id)
        .h(px(34.))
        .rounded_full()
        .cursor_pointer()
        .on_hover(track.on_hover())
        .on_click(on_click)
        .child(visual)
        .into_any_element()
}

fn main_nav(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let theme = Theme::get(cx).clone();
    let (page, running_clock, open_count, tasks, current, now_playing_data, today, goal) = {
        let enki = model.read(cx);
        (
            enki.page,
            (!enki.timer.is_idle()).then(|| SharedString::from(enki.clock_text())),
            enki.open_tasks().count(),
            enki.open_tasks().take(8).cloned().collect::<Vec<_>>(),
            enki.current_task.clone(),
            enki.now_playing.clone().filter(|_| enki.page != Page::Timer),
            enki.stats.today(),
            enki.settings.daily_goal.max(1),
        )
    };

    let nav_timer = nav_item(
        "nav-timer",
        Icon::Timer,
        "Timer",
        running_clock,
        page == Page::Timer,
        &theme,
        window,
        cx,
        on_model(&model, |m, cx| m.navigate(Page::Timer, cx)),
    );
    let nav_tasks = nav_item(
        "nav-tasks",
        Icon::ListChecks,
        "Tarefas",
        (open_count > 0).then(|| open_count.to_string().into()),
        page == Page::Tasks,
        &theme,
        window,
        cx,
        on_model(&model, |m, cx| m.navigate(Page::Tasks, cx)),
    );
    let nav_stats = nav_item(
        "nav-stats",
        Icon::Chart,
        "Estatísticas",
        None,
        page == Page::Stats,
        &theme,
        window,
        cx,
        on_model(&model, |m, cx| m.navigate(Page::Stats, cx)),
    );
    let nav_wrapped = nav_item(
        "nav-wrapped",
        Icon::Music,
        "Wrapped",
        None,
        page == Page::Wrapped,
        &theme,
        window,
        cx,
        on_model(&model, |m, cx| m.navigate(Page::Wrapped, cx)),
    );

    let nav = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .child(nav_timer)
        .child(nav_tasks)
        .child(nav_stats)
        .child(nav_wrapped);

    let add_task_btn = {
        let model = model.clone();
        icon_button_sized(
            "sidebar-add-task",
            Icon::Plus,
            px(22.),
            &theme,
            window,
            cx,
            move |_, window, cx| {
                model.update(cx, |m, cx| m.navigate(Page::Tasks, cx));
                window.dispatch_action(Box::new(crate::actions::NewTask), cx);
            },
        )
    };

    let task_rows: Vec<AnyElement> = tasks
        .into_iter()
        .map(|task| {
            let selected = current.as_deref() == Some(task.id.as_str());
            let id = task.id.clone();
            let row_id = SharedString::from(format!("side-task-{}", task.id));
            let track = hover_track(row_id.clone(), theme.reduce_motion, window, cx);
            let hover_bg = mix_hsla(theme.surface_hover, theme.accent_soft, 0.3);
            let rest_bg = with_alpha(hover_bg, 0.0);
            let accent = theme.accent;
            let text = theme.text;
            let muted = theme.text_muted;
            let faint = theme.text_faint;
            let title = task.title.clone();
            let count = format!("{}/{}", task.pomodoros, task.estimate);

            if selected {
                div()
                    .id(row_id)
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(32.))
                    .px(px(10.))
                    .rounded(radius::MD)
                    .cursor_pointer()
                    .text_size(px(13.5))
                    .bg(theme.selected_bg)
                    .on_click(on_model(&model, move |m, cx| {
                        let next =
                            (m.current_task.as_deref() != Some(id.as_str())).then(|| id.clone());
                        m.set_current_task(next, cx);
                    }))
                    .child(div().size(px(6.)).rounded_full().flex_none().bg(accent))
                    .child(div().flex_1().min_w_0().ellipsis().text_color(text).child(title))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font(crate::theme::tabular_font())
                            .text_color(faint)
                            .child(count),
                    )
                    .into_any_element()
            } else {
                let visual = track.animate(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .size_full()
                        .px(px(10.))
                        .rounded(radius::MD)
                        .text_size(px(13.5)),
                    180,
                    ease_out_cubic,
                    move |d, t| {
                        let tc = t.clamp(0.0, 1.0);
                        d.bg(mix_hsla(rest_bg, hover_bg, tc))
                            .child(
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .flex_none()
                                    .bg(mix_hsla(faint, accent, tc)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .ellipsis()
                                    .text_color(mix_hsla(muted, text, tc))
                                    .child(title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font(crate::theme::tabular_font())
                                    .text_color(mix_hsla(faint, muted, tc))
                                    .child(count.clone()),
                            )
                    },
                );
                div()
                    .id(row_id)
                    .h(px(32.))
                    .rounded(radius::MD)
                    .cursor_pointer()
                    .on_hover(track.on_hover())
                    .on_click(on_model(&model, move |m, cx| {
                        let next =
                            (m.current_task.as_deref() != Some(id.as_str())).then(|| id.clone());
                        m.set_current_task(next, cx);
                    }))
                    .child(visual)
                    .into_any_element()
            }
        })
        .collect();

    let task_section = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .pt(px(18.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .h(px(28.))
                .px(px(10.))
                .text_size(px(13.))
                .text_color(theme.text_faint)
                .child("Tarefas abertas")
                .child(add_task_btn),
        )
        .when(task_rows.is_empty(), |d| {
            d.child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(13.))
                    .text_color(theme.text_faint)
                    .child("Nenhuma tarefa. Adicione uma para acompanhar seus pomodoros."),
            )
        })
        .children(task_rows);

    // The timer page shows the player itself, larger.
    let now_playing = now_playing_data.map(|np| {
        player::row(&np, &model, &theme, false, window, cx)
            .mx(px(10.))
            .mb(px(8.))
            .p(px(8.))
            .rounded(radius::LG)
            // Already on the sidebar's glass; glass on glass looks muddy.
            .map(|d| {
                if theme.liquid() {
                    d.bg(with_alpha(
                        gpui::white(),
                        if theme.is_dark { 0.07 } else { 0.5 },
                    ))
                } else {
                    d.bg(theme.surface).border_1().border_color(theme.border)
                }
            })
    });

    let footer_mini = icon_button(
        "footer-mini",
        Icon::Mini,
        &theme,
        window,
        cx,
        |_, _, cx| crate::toggle_mini_window(cx),
    );
    let footer_settings = icon_button(
        "footer-settings",
        Icon::Settings,
        &theme,
        window,
        cx,
        on_model(&model, |m, cx| {
            m.navigate(Page::Settings(SettingsSection::General), cx)
        }),
    );

    let footer = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .h(px(52.))
        .px(px(16.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.))
                .rounded_full()
                .bg(theme.accent_soft)
                .child(icon_sized(Icon::Tomato, px(14.), theme.accent_text)),
        )
        .child(
            div()
                .flex_1()
                .ellipsis()
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(format!("{} de {goal} hoje", today.pomodoros)),
        )
        .child(footer_mini)
        .child(footer_settings);

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .child(nav)
        .child(
            div()
                .id("sidebar-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(task_section),
        )
        .children(now_playing)
        .child(footer)
        .into_any_element()
}

fn settings_nav(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let theme = Theme::get(cx).clone();
    let current = match model.read(cx).page {
        Page::Settings(s) => s,
        _ => SettingsSection::General,
    };
    let glyph = |s: SettingsSection| match s {
        SettingsSection::General => Icon::Settings,
        SettingsSection::Appearance => Icon::Sliders,
        SettingsSection::Timer => Icon::Timer,
        SettingsSection::Notifications => Icon::Bell,
        SettingsSection::Shortcuts => Icon::Keyboard,
    };
    let back = div().mb(px(12.)).child(nav_item(
        "settings-back",
        Icon::ArrowLeft,
        "Voltar",
        None,
        false,
        &theme,
        window,
        cx,
        on_model(&model, |m, cx| m.navigate(Page::Timer, cx)),
    ));
    let items: Vec<AnyElement> = SettingsSection::ALL
        .into_iter()
        .map(|section| {
            nav_item(
                SharedString::from(format!("settings-{}", section.label())),
                glyph(section),
                section.label(),
                None,
                section == current,
                &theme,
                window,
                cx,
                on_model(&model, move |m, cx| {
                    m.navigate(Page::Settings(section), cx)
                }),
            )
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .child(back)
        .children(items)
        .into_any_element()
}
