//! Focus Wrapped: most-listened tracks and artists by period.

use std::path::PathBuf;

use gpui::{
    div, img, linear_color_stop, linear_gradient, prelude::*, px, AnyElement, Context, FontWeight,
    Hsla, ImageSource, ObjectFit, Window,
};

use crate::{
    assets::Icon,
    store::{MusicWrapped, TopArtist, TopTrack, WrappedPeriod},
    theme::{radius, tabular_font, with_alpha, Theme},
    ui::{
        components::{
            button, card, icon_sized, page_title, section_label, segmented, ButtonKind,
            SegmentOption, StyledExt as _,
        },
        format_minutes, on_model, page_padding_x, content_viewport,
        root::RootView,
    },
};

const PAGE_MAX_W: f32 = 820.;

pub fn render(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let model = view.model.clone();
    let theme = Theme::get(cx).clone();
    let (wrapped, period, focus_only, tracking_enabled, sidebar_open) = {
        let enki = model.read(cx);
        (
            enki.wrapped.clone(),
            enki.wrapped_period,
            enki.wrapped_focus_only,
            enki.settings.now_playing && enki.settings.track_music,
            enki.settings.sidebar_open,
        )
    };
    let (avail_w, _) = content_viewport(window, sidebar_open);
    let pad_x = page_padding_x(avail_w);
    let inner_w = avail_w.min(PAGE_MAX_W) - 2. * pad_x;

    let period_switcher = segmented(
        "wrapped-period",
        &theme,
        window,
        cx,
        WrappedPeriod::ALL
            .into_iter()
            .map(|p| SegmentOption {
                label: p.label().into(),
                icon: None,
                selected: p == period,
                on_click: Box::new(on_model(&model, move |m, cx| m.set_wrapped_period(p, cx))),
            })
            .collect(),
    );

    let scope_switcher = segmented(
        "wrapped-scope",
        &theme,
        window,
        cx,
        vec![
            SegmentOption {
                label: "Só em foco".into(),
                icon: None,
                selected: focus_only,
                on_click: Box::new(on_model(&model, |m, cx| {
                    m.set_wrapped_focus_only(true, cx)
                })),
            },
            SegmentOption {
                label: "Tudo".into(),
                icon: None,
                selected: !focus_only,
                on_click: Box::new(on_model(&model, |m, cx| {
                    m.set_wrapped_focus_only(false, cx)
                })),
            },
        ],
    );

    let filters = div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .child(period_switcher)
        .child(scope_switcher);

    let body: Vec<AnyElement> = if wrapped.total_plays == 0 {
        let subtitle = if !tracking_enabled {
            "Ative \"Música tocando agora\" e \"Histórico de música\" para montar seu Wrapped de Foco."
        } else if focus_only {
            "Ouça Spotify ou Apple Music enquanto o timer de foco estiver rodando. Faixas tocadas por pelo menos 20s aparecem aqui."
        } else {
            "Nenhuma música registrada neste período ainda. Faixas tocadas por pelo menos 20s com o Enki aberto aparecem aqui."
        };
        let enable_btn = (!tracking_enabled).then(|| {
            button(
                "wrapped-enable-tracking",
                "Ativar rastreamento de música",
                ButtonKind::Secondary,
                &theme,
                window,
                cx,
                on_model(&model, |m, cx| {
                    m.update_settings(cx, |s| {
                        s.now_playing = true;
                        s.track_music = true;
                    });
                }),
            )
        });
        vec![card("wrapped-empty", &theme)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .py(px(44.))
                    .px(px(24.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(48.))
                            .rounded_full()
                            .bg(theme.accent_soft)
                            .child(icon_sized(Icon::Music, px(22.), theme.accent_text)),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(format!(
                                "Sem trilha sonora em {}",
                                period.title().to_lowercase()
                            )),
                    )
                    .child(
                        div()
                            .max_w(px(440.))
                            .text_center()
                            .text_size(px(13.))
                            .text_color(theme.text_muted)
                            .child(subtitle),
                    )
                    .children(enable_btn),
            )
            .into_any_element()]
    } else {
        let hero = wrapped_hero(&wrapped, period, focus_only, inner_w, &theme);

        let two_cols = inner_w >= 620.;
        let tracks_card = div()
            .flex()
            .flex_col()
            .when(two_cols, |d| d.flex_1())
            .min_w_0()
            .child(section_label(&theme, "Top músicas"))
            .child(top_tracks_card(&wrapped.top_tracks, &theme));

        let artists_card = div()
            .flex()
            .flex_col()
            .when(two_cols, |d| d.flex_1())
            .min_w_0()
            .child(section_label(&theme, "Top artistas"))
            .child(top_artists_card(&wrapped.top_artists, &theme));

        let rankings = div()
            .flex()
            .when(!two_cols, |d| d.flex_col())
            .gap(px(16.))
            .child(tracks_card)
            .child(artists_card)
            .into_any_element();

        let focus_pct = if wrapped.total_seconds > 0 {
            ((wrapped.focus_seconds as f64 * 100.0) / wrapped.total_seconds as f64).round() as u32
        } else {
            0
        };
        let footer_text = match (focus_only, wrapped.top_player.as_deref()) {
            (true, Some(player)) => format!(
                "Mostrando apenas músicas ouvidas durante sessões de foco · Player favorito: {player}"
            ),
            (true, None) => "Mostrando apenas músicas ouvidas durante sessões de foco.".to_string(),
            (false, Some(player)) => format!(
                "{focus_pct}% do tempo ouvido foi durante sessões de foco · Player favorito: {player}"
            ),
            (false, None) => format!("{focus_pct}% do tempo ouvido foi durante sessões de foco."),
        };
        let footer = div()
            .px(px(4.))
            .text_size(px(12.5))
            .text_color(theme.text_faint)
            .child(footer_text)
            .into_any_element();

        vec![hero, rankings, footer]
    };

    div()
        .id("wrapped-page")
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
                .child(page_title(&theme, "Wrapped"))
                .child(filters)
                .children(body),
        )
        .into_any_element()
}

fn wrapped_hero(
    wrapped: &MusicWrapped,
    period: WrappedPeriod,
    focus_only: bool,
    inner_w: f32,
    theme: &Theme,
) -> AnyElement {
    let top_track = wrapped.top_tracks.first();
    let top_artist = wrapped.top_artists.first();
    let hero_art = top_track
        .and_then(|t| t.art_path.as_deref())
        .or_else(|| top_artist.and_then(|a| a.art_path.as_deref()));

    let headline = top_artist
        .map(|a| a.artist.clone())
        .or_else(|| top_track.map(|t| t.title.clone()))
        .unwrap_or_else(|| "Sua trilha sonora".to_string());

    let subline = match (top_artist, top_track) {
        (Some(artist), Some(track)) => format!(
            "Artista #1 ({}) · Música #1: {}",
            format_listen_time(artist.seconds),
            track.title
        ),
        (None, Some(track)) => format!(
            "Música #1 · {} ({})",
            track.title,
            format_listen_time(track.seconds)
        ),
        _ => String::new(),
    };

    let compact = inner_w < 460.;
    let stat_chip = |label: &'static str, value: String| {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .flex_1()
            .min_w_0()
            .px(px(12.))
            .py(px(10.))
            .rounded(radius::MD)
            .bg(with_alpha(
                if theme.is_dark {
                    gpui::white()
                } else {
                    gpui::black()
                },
                if theme.is_dark { 0.06 } else { 0.04 },
            ))
            .child(
                div()
                    .text_size(px(if compact { 18. } else { 20. }))
                    .font(tabular_font())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .whitespace_nowrap()
                    .child(value),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.text_muted)
                    .ellipsis_child(label),
            )
    };

    let badge = if focus_only {
        format!("WRAPPED DE FOCO · {}", period.title().to_uppercase())
    } else {
        format!("WRAPPED GERAL · {}", period.title().to_uppercase())
    };

    let c_time = stat_chip("Tempo ouvindo", format_listen_time(wrapped.total_seconds));
    let c_plays = stat_chip("Reproduções", wrapped.total_plays.to_string());
    let c_artists = stat_chip("Artistas", wrapped.distinct_artists.to_string());
    let c_tracks = stat_chip("Músicas", wrapped.distinct_tracks.to_string());

    let stats_grid: AnyElement = if inner_w >= 560. {
        div()
            .flex()
            .gap(px(10.))
            .child(c_time)
            .child(c_plays)
            .child(c_artists)
            .child(c_tracks)
            .into_any_element()
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(div().flex().gap(px(10.)).child(c_time).child(c_plays))
            .child(div().flex().gap(px(10.)).child(c_artists).child(c_tracks))
            .into_any_element()
    };

    card("wrapped-hero", theme)
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .gap(px(if compact { 14. } else { 18. }))
                .p(px(if compact { 16. } else { 20. }))
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(with_alpha(theme.accent, 0.22), 0.),
                    linear_color_stop(with_alpha(theme.rest, 0.14), 1.),
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(if compact { 12. } else { 16. }))
                        .child(album_thumb(
                            hero_art,
                            if compact { 56. } else { 76. },
                            radius::LG,
                            theme,
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.accent_text)
                                        .ellipsis_child(badge),
                                )
                                .child(
                                    div()
                                        .text_size(px(if compact { 21. } else { 26. }))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text)
                                        .ellipsis_child(headline),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .text_color(theme.text_muted)
                                        .ellipsis_child(subline),
                                ),
                        ),
                )
                .child(stats_grid),
        )
        .into_any_element()
}

fn top_tracks_card(tracks: &[TopTrack], theme: &Theme) -> AnyElement {
    let max_secs = tracks.first().map(|t| t.seconds).unwrap_or(1).max(1);
    card("wrapped-top-tracks", theme)
        .children(tracks.iter().enumerate().map(|(ix, track)| {
            let ratio = (track.seconds as f32 / max_secs as f32).clamp(0.08, 1.0);
            let plays_label = format!("{}×", track.plays);
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(14.))
                .py(px(10.))
                .when(ix > 0, |d| d.border_t_1().border_color(theme.border))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(rank_badge(ix + 1, theme.accent, theme))
                        .child(album_thumb(
                            track.art_path.as_deref(),
                            36.,
                            radius::SM,
                            theme,
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .ellipsis_child(track.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.text_muted)
                                        .ellipsis_child(if track.artist.is_empty() {
                                            "Artista desconhecido".to_string()
                                        } else {
                                            track.artist.clone()
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_end()
                                .flex_none()
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font(tabular_font())
                                        .text_color(theme.text)
                                        .child(format_listen_time(track.seconds)),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .font(tabular_font())
                                        .text_color(theme.text_faint)
                                        .child(plays_label),
                                ),
                        ),
                )
                .child(progress_bar(ratio, theme.accent, theme))
        }))
        .into_any_element()
}

fn top_artists_card(artists: &[TopArtist], theme: &Theme) -> AnyElement {
    if artists.is_empty() {
        return card("wrapped-top-artists", theme)
            .child(
                div()
                    .p(px(20.))
                    .text_size(px(13.))
                    .text_color(theme.text_faint)
                    .child("Nenhum artista identificado ainda."),
            )
            .into_any_element();
    }
    let max_secs = artists.first().map(|a| a.seconds).unwrap_or(1).max(1);
    card("wrapped-top-artists", theme)
        .children(artists.iter().enumerate().map(|(ix, artist)| {
            let ratio = (artist.seconds as f32 / max_secs as f32).clamp(0.08, 1.0);
            let tracks_label = if artist.distinct_tracks == 1 {
                "1 música".to_string()
            } else {
                format!("{} músicas", artist.distinct_tracks)
            };
            let plays_label = format!("{}×", artist.plays);
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(14.))
                .py(px(10.))
                .when(ix > 0, |d| d.border_t_1().border_color(theme.border))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(rank_badge(ix + 1, theme.rest, theme))
                        .child(artist_avatar(
                            &artist.artist,
                            artist.art_path.as_deref(),
                            theme,
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.text)
                                        .ellipsis_child(artist.artist.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.text_muted)
                                        .child(tracks_label),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_end()
                                .flex_none()
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font(tabular_font())
                                        .text_color(theme.text)
                                        .child(format_listen_time(artist.seconds)),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .font(tabular_font())
                                        .text_color(theme.text_faint)
                                        .child(plays_label),
                                ),
                        ),
                )
                .child(progress_bar(ratio, theme.rest, theme))
        }))
        .into_any_element()
}

fn rank_badge(rank: usize, color: Hsla, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(20.))
        .text_size(px(12.5))
        .font(tabular_font())
        .font_weight(if rank == 1 {
            FontWeight::BOLD
        } else {
            FontWeight::MEDIUM
        })
        .text_color(if rank == 1 { color } else { theme.text_faint })
        .child(format!("#{rank}"))
        .into_any_element()
}

fn album_thumb(art_path: Option<&str>, size: f32, r: gpui::Pixels, theme: &Theme) -> AnyElement {
    if let Some(path) = art_path.map(PathBuf::from).filter(|p| p.exists()) {
        return img(ImageSource::from(path))
            .size(px(size))
            .flex_none()
            .rounded(r)
            .object_fit(ObjectFit::Cover)
            .into_any_element();
    }
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(r)
        .bg(theme.surface_active)
        .child(icon_sized(
            Icon::Music,
            px((size * 0.42).clamp(14., 28.)),
            theme.text_muted,
        ))
        .into_any_element()
}

fn artist_avatar(name: &str, art_path: Option<&str>, theme: &Theme) -> AnyElement {
    if let Some(path) = art_path.map(PathBuf::from).filter(|p| p.exists()) {
        return img(ImageSource::from(path))
            .size(px(36.))
            .flex_none()
            .rounded_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element();
    }
    let initial = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "♪".to_string());
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(36.))
        .rounded_full()
        .bg(with_alpha(theme.rest, 0.16))
        .text_size(px(14.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.rest)
        .child(initial)
        .into_any_element()
}

fn progress_bar(ratio: f32, color: Hsla, theme: &Theme) -> AnyElement {
    div()
        .w_full()
        .h(px(3.))
        .rounded_full()
        .bg(theme.ring_track)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative(ratio))
                .rounded_full()
                .bg(with_alpha(color, 0.85)),
        )
        .into_any_element()
}

fn format_listen_time(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format_minutes(seconds)
    }
}
