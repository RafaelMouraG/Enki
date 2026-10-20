//! The now playing card: album art, track and media controls. The timer page
//! shows it large under the controls; the sidebar shows it small elsewhere.

use gpui::{div, img, prelude::*, px, AnyElement, Div, Entity, FontWeight, ImageSource, ObjectFit};

use crate::{
    assets::Icon,
    media::{MediaCommand, NowPlaying},
    state::Enki,
    theme::{radius, Theme},
    ui::{components::icon_sized, components::StyledExt as _, on_model},
};

/// The card's contents in a flex row; callers give it a background.
pub fn row(np: &NowPlaying, model: &Entity<Enki>, theme: &Theme, large: bool) -> Div {
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
    let media_button = |id: &'static str, glyph: Icon, cmd: MediaCommand, size: f32| {
        let hover = theme.surface_hover;
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(button_size))
            .rounded_full()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(icon_sized(glyph, px(size), theme.text))
            .on_click(on_model(model, move |m, cx| m.media(cmd, cx)))
    };
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
                .child(media_button(
                    "np-prev",
                    Icon::SkipBack,
                    MediaCommand::Previous,
                    glyph_size,
                ))
                .child(media_button(
                    "np-toggle",
                    if np.playing { Icon::Pause } else { Icon::Play },
                    MediaCommand::PlayPause,
                    glyph_size + 1.,
                ))
                .child(media_button(
                    "np-next",
                    Icon::SkipForward,
                    MediaCommand::Next,
                    glyph_size,
                )),
        )
}
