//! Stats: today, streaks, interactive last-7-days bars, 26-week heatmap,
//! recent focus sessions, manual logging and CSV export.

use chrono::{DateTime, Datelike, Duration as Days, Local, NaiveDate, Weekday};
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, ElementId, Entity, FontWeight, Hsla,
    SharedString, Window,
};

use crate::{
    assets::Icon,
    state::Enki,
    store::{DayStat, SessionItem},
    theme::{radius, tabular_font, with_alpha, Theme},
    ui::{
        components::{
            button, card, icon, icon_sized, page_title, section_label, ButtonKind, StyledExt as _,
            SurfaceExt as _,
        },
        content_viewport, format_minutes, on_model, page_padding_x,
        root::RootView,
    },
};

const PAGE_MAX_W: f32 = 820.;

pub fn render(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let bars_hover = window.use_keyed_state(
        ElementId::Name("stats-bars-hover".into()),
        cx,
        |_, _| None::<(NaiveDate, DayStat)>,
    );
    let heatmap_hover = window.use_keyed_state(
        ElementId::Name("stats-heatmap-hover".into()),
        cx,
        |_, _| None::<(NaiveDate, DayStat)>,
    );
    let hovered_bar = *bars_hover.read(cx);
    let hovered_heatmap = *heatmap_hover.read(cx);

    let theme = Theme::get(cx).clone();
    let theme = &theme;
    let (stats, today_stat, today_date, goal, recent_sessions, sidebar_open) = {
        let enki = model.read(cx);
        let stats = enki.stats.clone();
        let today_stat = stats.today();
        let today_date = stats
            .days
            .last()
            .map(|(d, _)| *d)
            .unwrap_or_else(|| Local::now().date_naive());
        (
            stats,
            today_stat,
            today_date,
            enki.settings.daily_goal.max(1),
            enki.recent_sessions.clone(),
            enki.settings.sidebar_open,
        )
    };
    let (avail_w, _) = content_viewport(window, sidebar_open);
    let pad_x = page_padding_x(avail_w);
    let inner_w = avail_w.min(PAGE_MAX_W) - 2. * pad_x;

    let export_csv = {
        let model = model.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            let dir = dirs::document_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let suggested = format!("enki-sessoes-{}.csv", Local::now().format("%Y-%m-%d"));
            let receiver = cx.prompt_for_new_path(&dir, Some(suggested.as_str()));
            let model = model.clone();
            cx.spawn(async move |cx| {
                let path = match receiver.await {
                    Ok(Ok(Some(path))) => path,
                    Ok(Ok(None)) | Err(_) => return,
                    Ok(Err(err)) => {
                        model
                            .update(cx, |m, cx| {
                                m.error = Some(format!("Erro ao escolher arquivo CSV: {err:#}"));
                                cx.notify();
                            })
                            .ok();
                        return;
                    }
                };
                let csv = match model.update(cx, |m, _| m.export_sessions_csv()) {
                    Ok(Ok(content)) => content,
                    Ok(Err(err)) => {
                        model
                            .update(cx, |m, cx| {
                                m.error =
                                    Some(format!("Não foi possível exportar o CSV: {err:#}"));
                                cx.notify();
                            })
                            .ok();
                        return;
                    }
                    Err(_) => return,
                };
                let write_res = cx
                    .background_executor()
                    .spawn(async move { std::fs::write(&path, csv) })
                    .await;
                if let Err(err) = write_res {
                    model
                        .update(cx, |m, cx| {
                            m.error = Some(format!("Não foi possível gravar o CSV: {err}"));
                            cx.notify();
                        })
                        .ok();
                }
            })
            .detach();
        }
    };

    let add_manual_btn = button(
        "stats-add-manual",
        "+ Registrar foco",
        ButtonKind::Secondary,
        theme,
        window,
        cx,
        on_model(&model, |m, cx| {
            let minutes = m.settings.focus_minutes;
            m.add_manual_session(minutes, cx);
        }),
    );
    let export_csv_btn = button(
        "stats-export-csv",
        "Exportar CSV",
        ButtonKind::Secondary,
        theme,
        window,
        cx,
        export_csv,
    );

    let header = div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .child(page_title(theme, "Estatísticas"))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .child(add_manual_btn)
                .child(export_csv_btn),
        );

    let tile_value_size = if inner_w < 440. { 22. } else { 28. };
    let tile = |glyph: Icon, label: &'static str, value: String, hint: String, color: Hsla| {
        div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(10.))
            .flex_1()
            .min_w_0()
            .p(px(16.))
            .rounded(radius::LG)
            .surface(format!("stats-tile-{label}"), radius::LG, theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(24.))
                            .rounded(radius::SM)
                            .bg(with_alpha(color, 0.16))
                            .child(icon_sized(glyph, px(14.), color)),
                    )
                    .child(div().min_w_0().ellipsis_child(label)),
            )
            .child(
                div()
                    .text_size(px(tile_value_size))
                    .font(tabular_font())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .whitespace_nowrap()
                    .child(value),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme.text_faint)
                    .child(hint),
            )
    };

    let week = stats.last(7);
    let week_seconds: u64 = week.iter().map(|(_, s)| s.focus_seconds).sum();
    let record_hint = match stats.completion_rate() {
        Some(rate) => format!("{} pomodoros · {rate}% concluídos", stats.total_pomodoros),
        None => format!("{} pomodoros no total", stats.total_pomodoros),
    };
    let t_today = tile(
        Icon::Tomato,
        "Hoje",
        today_stat.pomodoros.to_string(),
        format!("meta de {goal}"),
        theme.accent,
    );
    let t_focus = tile(
        Icon::Clock,
        "Foco hoje",
        format_minutes(today_stat.focus_seconds),
        format!("{} nos últimos 7 dias", format_minutes(week_seconds)),
        theme.rest,
    );
    let t_streak = tile(
        Icon::Flame,
        "Sequência",
        format!(
            "{} {}",
            stats.streak,
            if stats.streak == 1 { "dia" } else { "dias" }
        ),
        "dias seguidos com foco".into(),
        gpui::rgb(0xf0954a).into(),
    );
    let t_record = tile(
        Icon::Trophy,
        "Recorde",
        format!(
            "{} {}",
            stats.best_streak,
            if stats.best_streak == 1 {
                "dia"
            } else {
                "dias"
            }
        ),
        record_hint,
        gpui::rgb(0xf2c14e).into(),
    );
    let tiles: AnyElement = if inner_w >= 640. {
        div()
            .flex()
            .gap(px(12.))
            .child(t_today)
            .child(t_focus)
            .child(t_streak)
            .child(t_record)
            .into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(div().flex().gap(px(12.)).child(t_today).child(t_focus))
            .child(div().flex().gap(px(12.)).child(t_streak).child(t_record))
            .into_any_element()
    };

    let daily_avg = week_seconds / 7;
    let delta = week_delta_label(week_seconds, stats.prev_week_focus_seconds);
    let week_header = div()
        .flex()
        .flex_wrap()
        .items_baseline()
        .justify_between()
        .gap(px(8.))
        .child(section_label(theme, "Últimos 7 dias"))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.5))
                .text_color(theme.text_faint)
                .child(format!("Média de {}/dia", format_minutes(daily_avg)))
                .when_some(delta, |d, (label, positive)| {
                    d.child("·").child(
                        div()
                            .text_color(if positive {
                                theme.rest
                            } else {
                                theme.text_muted
                            })
                            .font_weight(if positive {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(label),
                    )
                }),
        );

    let bars = bars_content(week, hovered_bar, bars_hover, theme);
    let heatmap = heatmap_content(&stats.days, hovered_heatmap, heatmap_hover, inner_w >= 600., theme);

    let sessions_subtitle = match stats.completion_rate() {
        Some(rate) => format!(
            "{} concluídas · {} parciais · {rate}%",
            stats.completed_focus, stats.skipped_focus
        ),
        None => "Histórico de foco".to_string(),
    };
    let sessions_header = div()
        .flex()
        .flex_wrap()
        .items_baseline()
        .justify_between()
        .gap(px(8.))
        .child(section_label(theme, "Sessões recentes"))
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme.text_faint)
                .child(sessions_subtitle),
        );
    let sessions = recent_sessions_card(&recent_sessions, today_date, &model, theme);

    div()
        .id("stats-page")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .w_full()
                .max_w(px(PAGE_MAX_W))
                .mx_auto()
                .px(px(pad_x))
                .pt(px(24.))
                .pb(px(48.))
                .child(header)
                .child(tiles)
                .child(week_header)
                .child(card("stats-bars", theme).child(bars))
                .child(section_label(theme, "Últimos 6 meses"))
                .child(card("stats-heatmap", theme).child(heatmap))
                .child(sessions_header)
                .child(sessions),
        )
        .into_any_element()
}

fn bars_content(
    week: &[(NaiveDate, DayStat)],
    hovered: Option<(NaiveDate, DayStat)>,
    hover_state: Entity<Option<(NaiveDate, DayStat)>>,
    theme: &Theme,
) -> impl IntoElement {
    let max = week
        .iter()
        .map(|(_, s)| s.focus_seconds)
        .max()
        .unwrap_or(0)
        .max(25 * 60);

    let columns = div()
        .flex()
        .items_end()
        .justify_between()
        .gap(px(8.))
        .h(px(174.))
        .px(px(12.))
        .pt(px(14.))
        .pb(px(10.))
        .children(week.iter().enumerate().map(|(ix, &(date, stat))| {
            let fraction = stat.focus_seconds as f32 / max as f32;
            let is_today = ix + 1 == week.len();
            let is_hovered = hovered.map(|(d, _)| d) == Some(date);
            let hover_state = hover_state.clone();
            div()
                .id(SharedString::from(format!("stats-bar-{ix}")))
                .flex()
                .flex_col()
                .items_center()
                .justify_end()
                .gap(px(6.))
                .flex_1()
                .h_full()
                .py(px(6.))
                .px(px(4.))
                .rounded(radius::MD)
                .cursor_pointer()
                .when(is_hovered, |d| d.bg(theme.surface_hover))
                .on_hover(move |entered, _, cx| {
                    hover_state.update(cx, |slot, cx| {
                        if *entered {
                            *slot = Some((date, stat));
                            cx.notify();
                        } else if slot.map(|(d, _)| d) == Some(date) {
                            *slot = None;
                            cx.notify();
                        }
                    });
                })
                .child(
                    div()
                        .text_size(px(11.5))
                        .font(tabular_font())
                        .text_color(if is_hovered {
                            theme.text
                        } else {
                            theme.text_faint
                        })
                        .child(if stat.pomodoros > 0 {
                            stat.pomodoros.to_string()
                        } else {
                            String::new()
                        }),
                )
                .child(
                    div()
                        .w_full()
                        .max_w(px(36.))
                        .h(px((fraction * 100.).max(3.)))
                        .rounded(radius::SM)
                        .bg(if stat.focus_seconds == 0 {
                            if is_hovered {
                                theme.border_strong
                            } else {
                                theme.surface_active
                            }
                        } else if is_today || is_hovered {
                            theme.accent
                        } else {
                            with_alpha(theme.accent, 0.55)
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if is_today || is_hovered {
                            theme.text
                        } else {
                            theme.text_muted
                        })
                        .font_weight(if is_today || is_hovered {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .child(weekday_short(date.weekday())),
                )
        }));

    let footer = div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(16.))
        .py(px(10.))
        .border_t_1()
        .border_color(theme.border)
        .text_size(px(12.))
        .child(match hovered {
            Some((date, stat)) => div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(format_day_detail(date, stat)),
            None => div()
                .text_color(theme.text_faint)
                .child("Passe o mouse em uma barra para ver os detalhes do dia"),
        });

    div().flex().flex_col().child(columns).child(footer)
}

/// GitHub-style grid: one column per week (Monday on top), oldest on the left.
fn heatmap_content(
    days: &[(NaiveDate, DayStat)],
    hovered: Option<(NaiveDate, DayStat)>,
    hover_state: Entity<Option<(NaiveDate, DayStat)>>,
    center: bool,
    theme: &Theme,
) -> impl IntoElement {
    const CELL: f32 = 16.;
    const GAP: f32 = 4.;
    // Pad the start so the first column begins on a Monday.
    let lead = days
        .first()
        .map(|(d, _)| d.weekday().num_days_from_monday() as usize)
        .unwrap_or(0);
    let mut cells: Vec<Option<(NaiveDate, DayStat)>> = vec![None; lead];
    cells.extend(days.iter().copied().map(Some));
    let weeks: Vec<_> = cells.chunks(7).map(|w| w.to_vec()).collect();

    let month_label = |week: &[Option<(NaiveDate, DayStat)>]| -> SharedString {
        let first = week.iter().flatten().next();
        match first {
            Some((d, _)) if d.day() <= 7 => month_short(d.month()).into(),
            _ => SharedString::default(),
        }
    };

    let labels = div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .pt(px(CELL + GAP + 2.))
        .pr(px(6.))
        .text_size(px(10.5))
        .text_color(theme.text_faint)
        .children(
            ["seg", "", "qua", "", "sex", "", ""]
                .into_iter()
                .map(|l| div().h(px(CELL)).flex().items_center().child(l)),
        );

    let grid = div().flex().gap(px(GAP)).children(weeks.iter().map(|week| {
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(
                div()
                    .h(px(CELL))
                    .text_size(px(10.5))
                    .text_color(theme.text_faint)
                    .whitespace_nowrap()
                    .child(month_label(week)),
            )
            .children((0..7).map(|ix| {
                let cell = week.get(ix).copied().flatten();
                match cell {
                    Some((date, stat)) => {
                        let is_hovered = hovered.map(|(d, _)| d) == Some(date);
                        let hover_state = hover_state.clone();
                        div()
                            .id(SharedString::from(format!("hm-{date}")))
                            .size(px(CELL))
                            .rounded(px(3.))
                            .bg(level_color(theme, stat.pomodoros))
                            .cursor_pointer()
                            .when(is_hovered, |d| d.border_1().border_color(theme.text))
                            .on_hover(move |entered, _, cx| {
                                hover_state.update(cx, |slot, cx| {
                                    if *entered {
                                        *slot = Some((date, stat));
                                        cx.notify();
                                    } else if slot.map(|(d, _)| d) == Some(date) {
                                        *slot = None;
                                        cx.notify();
                                    }
                                });
                            })
                            .into_any_element()
                    }
                    None => div().size(px(CELL)).rounded(px(3.)).into_any_element(),
                }
            }))
    }));

    let legend = div()
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(11.5))
        .text_color(theme.text_faint)
        .child(div().pr(px(4.)).child("Menos"))
        .children([0, 1, 3, 5, 8].into_iter().map(|n| {
            div()
                .size(px(CELL - 2.))
                .rounded(px(3.))
                .bg(level_color(theme, n))
        }))
        .child(div().pl(px(4.)).child("Mais"));

    let footer = div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .pt(px(12.))
        .child(match hovered {
            Some((date, stat)) => div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .child(format_day_detail(date, stat)),
            None => div()
                .text_size(px(12.))
                .text_color(theme.text_faint)
                .child("Passe o mouse em um dia para ver os detalhes"),
        })
        .child(legend);

    div()
        .flex()
        .flex_col()
        .p(px(16.))
        .child(
            div()
                .id("heatmap-scroll")
                .flex()
                .when(center, |d| d.justify_center())
                .overflow_x_scroll()
                .child(div().flex().flex_none().child(labels).child(grid)),
        )
        .child(footer)
}

fn recent_sessions_card(
    sessions: &[SessionItem],
    today: NaiveDate,
    model: &Entity<Enki>,
    theme: &Theme,
) -> AnyElement {
    if sessions.is_empty() {
        return card("stats-recent-sessions", theme)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .py(px(32.))
                    .px(px(16.))
                    .text_color(theme.text_faint)
                    .child(icon_sized(Icon::Clock, px(24.), theme.text_faint))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .child("Nenhuma sessão registrada ainda."),
                    ),
            )
            .into_any_element();
    }

    let hover_bg = theme.surface_hover;
    let danger = theme.danger;

    card("stats-recent-sessions", theme)
        .children(sessions.iter().enumerate().map(|(ix, session)| {
            let first = ix == 0;
            let id = session.id.clone();
            let group = SharedString::from(format!("session-{}", session.id));
            let del_group = SharedString::from(format!("session-del-{}", session.id));

            let title = session_label(session);
            let when = format_session_time(session.started_at, today);
            let duration = format_session_duration(session.seconds);

            let delete_btn = div()
                .id(SharedString::from(format!("del-session-{}", session.id)))
                .group(del_group.clone())
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(26.))
                .rounded(radius::SM)
                .cursor_pointer()
                .invisible()
                .group_hover(group.clone(), |s| s.visible())
                .hover(move |s| s.bg(hover_bg))
                .child(
                    icon(Icon::Trash)
                        .size(px(14.))
                        .text_color(theme.text_faint)
                        .group_hover(del_group, move |s| s.text_color(danger)),
                )
                .on_click(on_model(model, move |m, cx| m.delete_session(&id, cx)));

            div()
                .id(SharedString::from(format!("session-row-{}", session.id)))
                .group(group)
                .flex()
                .items_center()
                .gap(px(12.))
                .min_h(px(52.))
                .px(px(16.))
                .py(px(10.))
                .when(!first, |d| d.border_t_1().border_color(theme.border))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(28.))
                        .rounded(radius::SM)
                        .bg(if session.completed {
                            with_alpha(theme.accent, 0.14)
                        } else {
                            theme.surface_active
                        })
                        .child(icon_sized(
                            Icon::Tomato,
                            px(14.),
                            if session.completed {
                                theme.accent
                            } else {
                                theme.text_faint
                            },
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if session.completed {
                                    theme.text
                                } else {
                                    theme.text_muted
                                })
                                .ellipsis_child(title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.text_faint)
                                .child(when),
                        ),
                )
                .when(!session.completed, |d| {
                    d.child(
                        div()
                            .flex_none()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded_full()
                            .bg(with_alpha(theme.text_muted, 0.14))
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_muted)
                            .child("Parcial"),
                    )
                })
                .child(
                    div()
                        .flex_none()
                        .text_size(px(13.))
                        .font(tabular_font())
                        .text_color(theme.text_muted)
                        .child(duration),
                )
                .child(delete_btn)
        }))
        .into_any_element()
}

fn session_label(session: &SessionItem) -> String {
    let task = session
        .task_title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let intention = session
        .intention
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (task, intention) {
        (Some(t), Some(i)) => format!("{t} · {i}"),
        (Some(t), None) => t.to_string(),
        (None, Some(i)) => i.to_string(),
        (None, None) => "Sessão de foco".to_string(),
    }
}

fn format_day_detail(date: NaiveDate, stat: DayStat) -> String {
    let pomos = if stat.pomodoros == 1 {
        "1 pomodoro".to_string()
    } else {
        format!("{} pomodoros", stat.pomodoros)
    };
    format!(
        "{} {} · {pomos} · {}",
        date.day(),
        month_short(date.month()),
        format_minutes(stat.focus_seconds),
    )
}

fn format_session_time(ts: i64, today: NaiveDate) -> String {
    let Some(utc) = DateTime::from_timestamp(ts, 0) else {
        return String::new();
    };
    let local = utc.with_timezone(&Local);
    let date = local.date_naive();
    let time = local.format("%H:%M");
    if date == today {
        format!("Hoje, {time}")
    } else if date == today - Days::days(1) {
        format!("Ontem, {time}")
    } else {
        format!("{} {}, {time}", date.day(), month_short(date.month()))
    }
}

fn format_session_duration(seconds: u64) -> String {
    if (1..60).contains(&seconds) {
        format!("{seconds} s")
    } else {
        format_minutes(seconds)
    }
}

/// Returns `(label, is_positive)` comparing current 7-day focus against previous 7 days.
fn week_delta_label(current: u64, previous: u64) -> Option<(String, bool)> {
    if previous == 0 {
        return (current > 0).then(|| ("novo ritmo vs. semana anterior".to_string(), true));
    }
    let pct = (((current as f64 - previous as f64) / previous as f64) * 100.0).round() as i64;
    match pct.cmp(&0) {
        std::cmp::Ordering::Greater => Some((format!("+{pct}% vs. semana anterior"), true)),
        std::cmp::Ordering::Less => Some((format!("{pct}% vs. semana anterior"), false)),
        std::cmp::Ordering::Equal => Some(("0% vs. semana anterior".to_string(), false)),
    }
}

fn weekday_short(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "seg",
        Weekday::Tue => "ter",
        Weekday::Wed => "qua",
        Weekday::Thu => "qui",
        Weekday::Fri => "sex",
        Weekday::Sat => "sáb",
        Weekday::Sun => "dom",
    }
}

fn level_color(theme: &Theme, pomodoros: u32) -> Hsla {
    match pomodoros {
        0 => theme.surface_active,
        1..=2 => with_alpha(theme.accent, 0.3),
        3..=4 => with_alpha(theme.accent, 0.55),
        5..=7 => with_alpha(theme.accent, 0.8),
        _ => theme.accent,
    }
}

fn month_short(month: u32) -> &'static str {
    [
        "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
    ][(month as usize).saturating_sub(1).min(11)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn day_detail_formatting() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let detail = format_day_detail(
            date,
            DayStat {
                focus_seconds: 100 * 60,
                pomodoros: 4,
            },
        );
        assert_eq!(detail, "14 out · 4 pomodoros · 1 h 40 min");

        let single = format_day_detail(
            date,
            DayStat {
                focus_seconds: 25 * 60,
                pomodoros: 1,
            },
        );
        assert_eq!(single, "14 out · 1 pomodoro · 25 min");
    }

    #[test]
    fn week_delta_formatting() {
        assert_eq!(week_delta_label(0, 0), None);
        assert_eq!(
            week_delta_label(1500, 0),
            Some(("novo ritmo vs. semana anterior".to_string(), true))
        );
        assert_eq!(
            week_delta_label(125, 100),
            Some(("+25% vs. semana anterior".to_string(), true))
        );
        assert_eq!(
            week_delta_label(80, 100),
            Some(("-20% vs. semana anterior".to_string(), false))
        );
        assert_eq!(
            week_delta_label(100, 100),
            Some(("0% vs. semana anterior".to_string(), false))
        );
    }

    #[test]
    fn session_time_formatting() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let ts = |d: NaiveDate, h: u32, m: u32| {
            Local
                .from_local_datetime(&d.and_hms_opt(h, m, 0).unwrap())
                .unwrap()
                .timestamp()
        };
        assert_eq!(format_session_time(ts(today, 14, 20), today), "Hoje, 14:20");
        assert_eq!(
            format_session_time(ts(today - Days::days(1), 9, 15), today),
            "Ontem, 09:15"
        );
        assert_eq!(
            format_session_time(ts(today - Days::days(2), 16, 0), today),
            "12 out, 16:00"
        );
    }
}
