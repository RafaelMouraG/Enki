//! Stats: today, streaks, last 7 days and a 20-week heatmap.

use chrono::{Datelike, Weekday};
use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, Hsla, SharedString};

use crate::{
    assets::Icon,
    store::DayStat,
    theme::{radius, tabular_font, with_alpha, Theme},
    ui::{
        components::{card, icon_sized, page_title, section_label, SurfaceExt as _},
        format_minutes,
        root::RootView,
    },
};

pub fn render(view: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    let theme = Theme::get(cx);
    let enki = view.model.read(cx);
    let stats = &enki.stats;
    let today = stats.today();
    let goal = enki.settings.daily_goal.max(1);

    let tile = |glyph: Icon, label: &'static str, value: String, hint: String, color: Hsla| {
        div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(10.))
            .flex_1()
            .min_w(px(150.))
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
                            .items_center()
                            .justify_center()
                            .size(px(24.))
                            .rounded(radius::SM)
                            .bg(with_alpha(color, 0.16))
                            .child(icon_sized(glyph, px(14.), color)),
                    )
                    .child(label),
            )
            .child(
                div()
                    .text_size(px(28.))
                    .font(tabular_font())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .child(value),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme.text_faint)
                    .child(hint),
            )
    };

    let week_seconds: u64 = stats.last(7).iter().map(|(_, s)| s.focus_seconds).sum();
    let tiles = div()
        .flex()
        .flex_wrap()
        .gap(px(12.))
        .child(tile(
            Icon::Tomato,
            "Hoje",
            today.pomodoros.to_string(),
            format!("meta de {goal}"),
            theme.accent,
        ))
        .child(tile(
            Icon::Clock,
            "Foco hoje",
            format_minutes(today.focus_seconds),
            format!("{} nos últimos 7 dias", format_minutes(week_seconds)),
            theme.rest,
        ))
        .child(tile(
            Icon::Flame,
            "Sequência",
            format!(
                "{} {}",
                stats.streak,
                if stats.streak == 1 { "dia" } else { "dias" }
            ),
            "dias seguidos com foco".into(),
            gpui::rgb(0xf0954a).into(),
        ))
        .child(tile(
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
            format!("{} pomodoros no total", stats.total_pomodoros),
            gpui::rgb(0xf2c14e).into(),
        ));

    // Last 7 days as bars.
    let week = stats.last(7);
    let max = week
        .iter()
        .map(|(_, s)| s.focus_seconds)
        .max()
        .unwrap_or(0)
        .max(25 * 60);
    let bars = div()
        .flex()
        .items_end()
        .justify_between()
        .gap(px(10.))
        .h(px(170.))
        .px(px(16.))
        .pt(px(16.))
        .pb(px(12.))
        .children(week.iter().enumerate().map(|(ix, (date, stat))| {
            let fraction = stat.focus_seconds as f32 / max as f32;
            let is_today = ix + 1 == week.len();
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_end()
                .gap(px(6.))
                .flex_1()
                .h_full()
                .child(
                    div()
                        .text_size(px(11.5))
                        .font(tabular_font())
                        .text_color(theme.text_faint)
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
                            theme.surface_active
                        } else if is_today {
                            theme.accent
                        } else {
                            with_alpha(theme.accent, 0.55)
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if is_today {
                            theme.text
                        } else {
                            theme.text_muted
                        })
                        .font_weight(if is_today {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .child(weekday_short(date.weekday())),
                )
        }));

    let heatmap = heatmap(&stats.days, theme);

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
                .max_w(px(820.))
                .mx_auto()
                .px(px(32.))
                .pt(px(24.))
                .pb(px(48.))
                .child(page_title(theme, "Estatísticas"))
                .child(tiles)
                .child(section_label(theme, "Últimos 7 dias"))
                .child(card("stats-bars", theme).child(bars))
                .child(section_label(theme, "Últimos 6 meses"))
                .child(card("stats-heatmap", theme).child(heatmap)),
        )
        .into_any_element()
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

/// GitHub-style grid: one column per week (Monday on top), oldest on the left.
fn heatmap(days: &[(chrono::NaiveDate, DayStat)], theme: &Theme) -> impl IntoElement {
    const CELL: f32 = 16.;
    const GAP: f32 = 4.;
    // Pad the start so the first column begins on a Monday.
    let lead = days
        .first()
        .map(|(d, _)| d.weekday().num_days_from_monday() as usize)
        .unwrap_or(0);
    let mut cells: Vec<Option<(chrono::NaiveDate, DayStat)>> = vec![None; lead];
    cells.extend(days.iter().copied().map(Some));
    let weeks: Vec<_> = cells.chunks(7).map(|w| w.to_vec()).collect();

    let month_label = |week: &[Option<(chrono::NaiveDate, DayStat)>]| -> SharedString {
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
                div().size(px(CELL)).rounded(px(3.)).map(|d| match cell {
                    Some((_, stat)) => d.bg(level_color(theme, stat.pomodoros)),
                    None => d,
                })
            }))
    }));

    let legend = div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(4.))
        .pt(px(10.))
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

    div()
        .flex()
        .flex_col()
        .p(px(16.))
        .child(
            div()
                .id("heatmap-scroll")
                .flex()
                .justify_center()
                .overflow_x_scroll()
                .child(div().flex().child(labels).child(grid)),
        )
        .child(legend)
}

fn month_short(month: u32) -> &'static str {
    [
        "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
    ][(month as usize).saturating_sub(1).min(11)]
}
