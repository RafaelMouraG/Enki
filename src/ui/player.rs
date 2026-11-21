//! The now playing card: album art, track and media controls. The timer page
//! shows it large under the controls; the sidebar shows it small elsewhere.

use gpui::{
    div, img, prelude::*, px, AnyElement, App, Div, Entity, FontWeight, ImageSource, ObjectFit,
    SharedString, Window,
};

use crate::{
    assets::Icon,
    media::{MediaCommand, NowPlaying},
    state::Enki,
    theme::{mix_hsla, radius, with_alpha, Theme},
    ui::{
        components::{
            ease_out_cubic, glass_shadow, hover_track, icon_sized, liquid_highlights,
            StyledExt as _,
        },
        on_model,
    },
};

/// The card's contents in a flex row; callers give it a background.
pub fn row(
    np: &NowPlaying,
    model: &Entity<Enki>,
    theme: &Theme,
    large: bool,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let (art_size, title_size, artist_size, button_size, glyph_size) = if large {
        (44., 14.5, 12.5, 34., 15.)
    } else {
        (38., 13., 12., 26., 12.)
    };
    let art: AnyElement = match np.art.clone() {
        Some(path) => img(ImageSource::from(path))
            .size(px(art_size))
            .rounded(radius::SM)
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(art_size))
            .rounded(radius::SM)
            .bg(theme.surface_active)
            .child(icon_sized(Icon::Music, px(16.), theme.text_muted))
            .into_any_element(),
    };

    let mut media_button = |id: &'static str, glyph: Icon, cmd: MediaCommand, size: f32| {
        let key = SharedString::from(if large {
            format!("{id}-lg")
        } else {
            format!("{id}-sm")
        });
        let track = hover_track(key.clone(), theme.reduce_motion, window, cx);
        let is_dark = theme.is_dark;
        let hover_bg = with_alpha(gpui::white(), if is_dark { 0.14 } else { 0.72 });
        let rest_bg = with_alpha(gpui::white(), 0.0);
        let hover_border = with_alpha(
            if is_dark {
                gpui::white()
            } else {
                gpui::black()
            },
            if is_dark { 0.24 } else { 0.12 },
        );
        let rest_border = with_alpha(hover_border, 0.0);
        let fg = theme.text;
        let muted = theme.text_muted;
        let radius = px(button_size / 2.);

        let visual = track.animate(
            div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .rounded_full()
                .border_1(),
            210,
            ease_out_cubic,
            move |d, t| {
                let tc = t.clamp(0.0, 1.0);
                d.bg(mix_hsla(rest_bg, hover_bg, tc))
                    .border_color(mix_hsla(rest_border, hover_border, tc))
                    .shadow(glass_shadow(is_dark, 0.0, tc))
                    .when(tc > 0.01, |d| {
                        d.child(liquid_highlights(radius, is_dark, 0.0, tc))
                    })
                    .child(div().relative().child(icon_sized(
                        glyph,
                        px(size),
                        mix_hsla(muted, fg, 0.65 + 0.35 * tc),
                    )))
            },
        );

        div()
            .id(key)
            .flex()
            .size(px(button_size))
            .rounded_full()
            .cursor_pointer()
            .active(|s| s.opacity(0.8))
            .on_hover(track.on_hover())
            .on_click(on_model(model, move |m, cx| m.media(cmd, cx)))
            .child(visual)
            .into_any_element()
    };

    let prev = media_button("np-prev", Icon::SkipBack, MediaCommand::Previous, glyph_size);
    let toggle = media_button(
        "np-toggle",
        if np.playing { Icon::Pause } else { Icon::Play },
        MediaCommand::PlayPause,
        glyph_size + 1.,
    );
    let next = media_button("np-next", Icon::SkipForward, MediaCommand::Next, glyph_size);

    div()
        .flex()
        .items_center()
        .gap(px(if large { 12. } else { 10. }))
        .child(art)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .ellipsis()
                        .text_size(px(title_size))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(np.title.clone()),
                )
                .child(
                    div()
                        .ellipsis()
                        .text_size(px(artist_size))
                        .text_color(theme.text_muted)
                        .child(np.artist.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(if large { 4. } else { 0. }))
                .child(prev)
                .child(toggle)
                .child(next),
        )
}
