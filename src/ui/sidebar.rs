//! Left column: navigation, task list, now playing and the footer.

use gpui::{
    div, prelude::*, px, AnyElement, Context, Decorations, FontWeight, SharedString, Window,
};

use crate::{
    assets::Icon,
    state::{Page, SettingsSection},
    theme::{radius, sidebar_width, with_alpha, Theme},
    ui::{
        components::StyledExt as _,
        components::{glass, icon, icon_button, icon_sized},
        on_model, player,
        root::{RootView, TITLEBAR_HEIGHT, TRAFFIC_LIGHTS_WIDTH},
    },
};

const FLOAT_INSET: f32 = 8.;
const FLOAT_RADIUS: gpui::Pixels = px(18.);

pub fn render(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let theme = Theme::get(cx);
    let enki = model.read(cx);
    let in_settings = matches!(enki.page, Page::Settings(_));
    // The liquid sidebar floats inset from the window edges.
    let inset = if theme.liquid() { FLOAT_INSET } else { 0. };
    let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });

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
        .child(icon_button(
            "sidebar-toggle",
            Icon::PanelLeft,
            theme,
            on_model(&model, |m, cx| {
                m.update_settings(cx, |s| s.sidebar_open = false)
            }),
        ))
        .child(div().flex_1());

    let body = if in_settings {
        settings_nav(view, cx)
    } else {
        main_nav(view, cx)
    };

    let theme = Theme::get(cx);
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
                    .child(glass("sidebar", FLOAT_RADIUS, theme))
            } else {
                d.w(sidebar_width()).h_full().bg(theme.sidebar_bg)
            }
        })
        .child(header)
        .child(body)
        .into_any_element()
}

fn nav_item(
    id: impl Into<SharedString>,
    glyph: Icon,
    label: impl Into<SharedString>,
    trailing: Option<SharedString>,
    selected: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let hover = theme.surface_hover;
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(34.))
        .px(px(10.))
        .rounded(radius::MD)
        .cursor_pointer()
        .text_size(px(14.))
        .map(|d| {
            if selected {
                d.bg(theme.selected_bg)
                    .text_color(theme.text)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                d.text_color(theme.text).hover(move |s| s.bg(hover))
            }
        })
        .child(icon(glyph).text_color(if selected {
            theme.text
        } else {
            theme.text_muted
        }))
        .child(div().flex_1().min_w_0().ellipsis().child(label.into()))
        .when_some(trailing, |d, t| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(if selected {
                        theme.text_muted
                    } else {
                        theme.text_faint
                    })
                    .font(crate::theme::tabular_font())
                    .child(t),
            )
        })
}

fn main_nav(view: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let theme = Theme::get(cx);
    let enki = model.read(cx);

    let running_clock = (!enki.timer.is_idle()).then(|| SharedString::from(enki.clock_text()));
    let open_count = enki.open_tasks().count();

    let nav = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .child(
            nav_item(
                "nav-timer",
                Icon::Timer,
                "Timer",
                running_clock,
                enki.page == Page::Timer,
                theme,
            )
            .on_click(on_model(&model, |m, cx| m.navigate(Page::Timer, cx))),
        )
        .child(
            nav_item(
                "nav-tasks",
                Icon::ListChecks,
                "Tarefas",
                (open_count > 0).then(|| open_count.to_string().into()),
                enki.page == Page::Tasks,
                theme,
            )
            .on_click(on_model(&model, |m, cx| m.navigate(Page::Tasks, cx))),
        )
        .child(
            nav_item(
                "nav-stats",
                Icon::Chart,
                "Estatísticas",
                None,
                enki.page == Page::Stats,
                theme,
            )
            .on_click(on_model(&model, |m, cx| m.navigate(Page::Stats, cx))),
        );

    // Open tasks, like Zeron's "Sessions" list.
    let tasks: Vec<_> = enki.open_tasks().take(8).cloned().collect();
    let current = enki.current_task.clone();
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
                .child({
                    let model = model.clone();
                    icon_button(
                        "sidebar-add-task",
                        Icon::Plus,
                        theme,
                        move |_, window, cx| {
                            model.update(cx, |m, cx| m.navigate(Page::Tasks, cx));
                            window.dispatch_action(Box::new(crate::actions::NewTask), cx);
                        },
                    )
                    .size(px(22.))
                }),
        )
        .when(tasks.is_empty(), |d| {
            d.child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(13.))
                    .text_color(theme.text_faint)
                    .child("Nenhuma tarefa. Adicione uma para acompanhar seus pomodoros."),
            )
        })
        .children(tasks.into_iter().map(|task| {
            let selected = current.as_deref() == Some(task.id.as_str());
            let hover = theme.surface_hover;
            let id = task.id.clone();
            div()
                .id(SharedString::from(format!("side-task-{}", task.id)))
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(32.))
                .px(px(10.))
                .rounded(radius::MD)
                .cursor_pointer()
                .text_size(px(13.5))
                .when(selected, |d| d.bg(theme.selected_bg))
                .when(!selected, |d| d.hover(move |s| s.bg(hover)))
                .child(
                    div()
                        .size(px(6.))
                        .rounded_full()
                        .flex_none()
                        .bg(if selected {
                            theme.accent
                        } else {
                            theme.text_faint
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .ellipsis()
                        .text_color(if selected {
                            theme.text
                        } else {
                            theme.text_muted
                        })
                        .child(task.title.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .font(crate::theme::tabular_font())
                        .text_color(theme.text_faint)
                        .child(format!("{}/{}", task.pomodoros, task.estimate)),
                )
                .on_click(on_model(&model, move |m, cx| {
                    let next = (m.current_task.as_deref() != Some(id.as_str())).then(|| id.clone());
                    m.set_current_task(next, cx);
                }))
        }));

    // The timer page shows the player itself, larger.
    let now_playing = enki
        .now_playing
        .as_ref()
        .filter(|_| enki.page != Page::Timer)
        .map(|np| {
            player::row(np, &model, theme, false)
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

    let today = enki.stats.today();
    let goal = enki.settings.daily_goal.max(1);
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
        .child(icon_button("footer-mini", Icon::Mini, theme, |_, _, cx| {
            crate::toggle_mini_window(cx)
        }))
        .child(icon_button(
            "footer-settings",
            Icon::Settings,
            theme,
            on_model(&model, |m, cx| {
                m.navigate(Page::Settings(SettingsSection::General), cx)
            }),
        ));

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

fn settings_nav(view: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let theme = Theme::get(cx);
    let enki = model.read(cx);
    let current = match enki.page {
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
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(10.))
        .child(
            nav_item(
                "settings-back",
                Icon::ArrowLeft,
                "Voltar",
                None,
                false,
                theme,
            )
            .mb(px(12.))
            .on_click(on_model(&model, |m, cx| m.navigate(Page::Timer, cx))),
        )
        .children(SettingsSection::ALL.into_iter().map(|section| {
            nav_item(
                SharedString::from(format!("settings-{}", section.label())),
                glyph(section),
                section.label(),
                None,
                section == current,
                theme,
            )
            .on_click(on_model(&model, move |m, cx| {
                m.navigate(Page::Settings(section), cx)
            }))
        }))
        .into_any_element()
}
