//! Design tokens. Every color in the UI comes from here so light/dark, accent
//! and wallpaper tinting stay consistent.

use gpui::{px, rgb, App, BoxShadow, Font, FontFeatures, FontWeight, Global, Hsla, Pixels, Rgba};
use std::sync::Arc;

use crate::settings::{ColorScheme, Glass, Settings};

#[derive(Clone, Debug)]
pub struct Theme {
    pub is_dark: bool,
    pub reduce_motion: bool,
    /// Glass surfaces are real macOS Liquid Glass views under GPUI's content.
    pub native_glass: bool,
    /// Native macOS view draws the backdrop (Liquid Glass or native video player).
    pub native_backdrop: bool,
    /// Glass surfaces are drawn: translucent fill plus a lit rim.
    pub drawn_glass: bool,

    pub window_bg: Hsla,
    pub sidebar_bg: Hsla,
    pub surface: Hsla,
    pub surface_hover: Hsla,
    pub surface_active: Hsla,
    pub control: Hsla,
    pub control_hover: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,

    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,

    pub accent: Hsla,
    pub accent_soft: Hsla,
    /// Accent tuned for text/links on the window background.
    pub accent_text: Hsla,
    pub on_accent: Hsla,
    /// Breaks use a calmer color than focus.
    pub rest: Hsla,
    pub danger: Hsla,

    /// Primary round button (white in dark mode, near-black in light mode).
    pub primary_button: Hsla,
    pub primary_button_fg: Hsla,

    pub ring_track: Hsla,
    pub scrim: Hsla,

    /// Fill and edge of drawn glass.
    pub glass_fill: Hsla,
    pub glass_rim: Hsla,
    /// The brighter top edge that catches the "light".
    pub glass_highlight: Hsla,
    /// Darkens native glass in dark mode so text on it stays readable.
    pub glass_tint: Option<Hsla>,
    /// Selected row or segment; see-through on glass.
    pub selected_bg: Hsla,
}

impl Global for Theme {}

pub const UI_FONT: &str = ".SystemUIFont";

pub mod radius {
    use gpui::{px, Pixels};
    pub const SM: Pixels = px(6.);
    pub const MD: Pixels = px(8.);
    pub const LG: Pixels = px(12.);
}

impl Theme {
    pub fn get(cx: &App) -> &Theme {
        cx.global::<Theme>()
    }

    pub fn build(
        settings: &Settings,
        system_dark: bool,
        wallpaper_tint: Option<Rgba>,
        has_native_video: bool,
    ) -> Self {
        let is_dark = match settings.color_scheme {
            ColorScheme::System => system_dark,
            ColorScheme::Light => false,
            ColorScheme::Dark => true,
        };
        let native_glass = settings.glass == Glass::Liquid && crate::native::glass_supported();
        let native_backdrop = native_glass || has_native_video;
        let drawn_glass = settings.glass == Glass::Liquid && !native_glass;
        // Drawn glass only reads as glass over an image; on a flat color it's just a panel.
        let translucent =
            native_glass || (settings.glass != Glass::Solid && wallpaper_tint.is_some());

        let accent_rgb = match (settings.match_wallpaper, wallpaper_tint) {
            (true, Some(tint)) => accent_from_wallpaper(tint, is_dark),
            _ => rgb(settings.accent.rgb()),
        };
        // Surfaces pick up a whisper of the wallpaper, like Zeron's "subtle surface tints".
        let tint = (settings.match_wallpaper)
            .then_some(wallpaper_tint)
            .flatten()
            .unwrap_or(accent_rgb);
        let tint_amount = if settings.match_wallpaper && wallpaper_tint.is_some() {
            if is_dark {
                0.06
            } else {
                0.035
            }
        } else if is_dark {
            0.025
        } else {
            0.0
        };
        let t = |hex: u32| -> Hsla { mix(rgb(hex), tint, tint_amount).into() };

        let accent: Hsla = accent_rgb.into();

        if is_dark {
            let sidebar_alpha = match (native_glass, translucent) {
                (true, _) => 0.0,
                (false, true) => 0.72,
                (false, false) => 1.0,
            };
            Theme {
                is_dark,
                reduce_motion: settings.reduce_motion,
                native_glass,
                native_backdrop,
                drawn_glass,
                window_bg: t(0x14181d),
                sidebar_bg: with_alpha(t(0x1a1e23), sidebar_alpha),
                surface: t(0x1c2126),
                surface_hover: t(0x23282e),
                surface_active: t(0x2a2f35),
                control: t(0x242a30),
                control_hover: t(0x2d333a),
                border: t(0x272c32),
                border_strong: t(0x353b42),
                text: rgb(0xe9ecef).into(),
                text_muted: rgb(0xa0a7af).into(),
                // Glass is brighter than the flat background.
                text_faint: rgb(if native_glass { 0x8a9199 } else { 0x6c737b }).into(),
                accent,
                accent_soft: with_alpha(accent, 0.16),
                accent_text: lighten(accent, 0.08),
                on_accent: on_color(accent),
                rest: rgb(0x6fd3a0).into(),
                danger: rgb(0xf0625d).into(),
                primary_button: rgb(0xf2f4f6).into(),
                primary_button_fg: rgb(0x14181d).into(),
                ring_track: with_alpha(rgb(0xffffff).into(), 0.08),
                scrim: rgb(0x0b0e11).into(),
                glass_fill: with_alpha(t(0x1c2126), if translucent { 0.55 } else { 1.0 }),
                glass_rim: with_alpha(rgb(0xffffff).into(), 0.09),
                glass_highlight: with_alpha(rgb(0xffffff).into(), 0.22),
                glass_tint: Some(with_alpha(rgb(0x0b0e11).into(), 0.3)),
                selected_bg: if native_glass || drawn_glass {
                    with_alpha(rgb(0xffffff).into(), 0.13)
                } else {
                    t(0x2a2f35)
                },
            }
        } else {
            let sidebar_alpha = match (native_glass, translucent) {
                (true, _) => 0.0,
                (false, true) => 0.7,
                (false, false) => 1.0,
            };
            let accent_text = darken_for_light(accent);
            Theme {
                is_dark,
                reduce_motion: settings.reduce_motion,
                native_glass,
                native_backdrop,
                drawn_glass,
                window_bg: t(0xffffff),
                sidebar_bg: with_alpha(t(0xf3f4f6), sidebar_alpha),
                surface: t(0xf6f7f9),
                surface_hover: t(0xeef0f3),
                surface_active: t(0xe7eaee),
                control: t(0xffffff),
                control_hover: t(0xf1f3f5),
                border: t(0xe3e6ea),
                border_strong: t(0xd2d7dd),
                text: rgb(0x15181c).into(),
                text_muted: rgb(0x5f6670).into(),
                text_faint: rgb(if native_glass { 0x80878f } else { 0x99a0a8 }).into(),
                accent,
                accent_soft: with_alpha(accent, 0.14),
                accent_text,
                on_accent: on_color(accent),
                rest: rgb(0x2fa86c).into(),
                danger: rgb(0xd9443f).into(),
                primary_button: rgb(0x15181c).into(),
                primary_button_fg: rgb(0xffffff).into(),
                ring_track: with_alpha(rgb(0x000000).into(), 0.07),
                scrim: rgb(0xffffff).into(),
                glass_fill: with_alpha(t(0xffffff), if translucent { 0.6 } else { 1.0 }),
                glass_rim: with_alpha(rgb(0x000000).into(), 0.08),
                glass_highlight: with_alpha(rgb(0xffffff).into(), 0.9),
                glass_tint: None,
                selected_bg: if native_glass || drawn_glass {
                    with_alpha(rgb(0xffffff).into(), 0.7)
                } else {
                    t(0xe7eaee)
                },
            }
        }
    }

    /// Liquid style: floating glass sidebar and glass controls.
    pub fn liquid(&self) -> bool {
        self.native_glass || self.drawn_glass
    }

    pub fn card_shadow(&self) -> Vec<BoxShadow> {
        let alpha = if self.is_dark { 0.35 } else { 0.08 };
        vec![BoxShadow {
            color: with_alpha(rgb(0x000000).into(), alpha),
            offset: gpui::point(px(0.), px(8.)),
            blur_radius: px(24.),
            spread_radius: px(-6.),
        }]
    }

    pub fn popover_shadow(&self) -> Vec<BoxShadow> {
        let alpha = if self.is_dark { 0.5 } else { 0.14 };
        vec![
            BoxShadow {
                color: with_alpha(rgb(0x000000).into(), alpha),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(-4.),
            },
            BoxShadow {
                color: with_alpha(rgb(0x000000).into(), alpha * 0.4),
                offset: gpui::point(px(0.), px(1.)),
                blur_radius: px(3.),
                spread_radius: px(0.),
            },
        ]
    }

    /// The color the timer ring uses for a phase.
    pub fn phase_color(&self, is_break: bool) -> Hsla {
        if is_break {
            self.rest
        } else {
            self.accent
        }
    }
}

/// Large timer digits: light weight with tabular figures so they don't jitter.
pub fn clock_font(weight: FontWeight) -> Font {
    Font {
        family: UI_FONT.into(),
        features: FontFeatures(Arc::new(vec![("tnum".into(), 1)])),
        fallbacks: None,
        weight,
        style: Default::default(),
    }
}

pub fn tabular_font() -> Font {
    clock_font(FontWeight::MEDIUM)
}

pub fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}

pub fn mix(a: Rgba, b: Rgba, amount: f32) -> Rgba {
    let t = amount.clamp(0.0, 1.0);
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// Blends two `Hsla` colors in RGBA space so transitions never sweep through unrelated hues.
pub fn mix_hsla(a: Hsla, b: Hsla, amount: f32) -> Hsla {
    let t = amount.clamp(0.0, 1.0);
    let mut ra = a.to_rgb();
    let mut rb = b.to_rgb();
    ra.a = a.a;
    rb.a = b.a;
    let mixed = mix(ra, rb, t);
    let mut out: Hsla = mixed.into();
    out.a = mixed.a;
    out
}

fn lighten(mut c: Hsla, by: f32) -> Hsla {
    c.l = (c.l + by).min(0.95);
    c
}

/// Bright accents (yellow, cyan) are unreadable as text on white.
fn darken_for_light(mut c: Hsla) -> Hsla {
    c.l = c.l.min(0.45);
    c.s = (c.s * 1.05).min(1.0);
    c
}

fn on_color(c: Hsla) -> Hsla {
    let rgb = c.to_rgb();
    let luminance = 0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b;
    if luminance > 0.6 {
        rgb_hsla(0x101317)
    } else {
        rgb_hsla(0xffffff)
    }
}

fn rgb_hsla(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Wallpaper colors are often muddy; push them into a usable accent range.
fn accent_from_wallpaper(tint: Rgba, is_dark: bool) -> Rgba {
    let mut c: Hsla = tint.into();
    if c.s < 0.12 {
        // Practically gray: fall back to a neutral blue-gray accent.
        c.h = 0.58;
        c.s = 0.25;
    } else {
        c.s = c.s.clamp(0.45, 0.8);
    }
    c.l = if is_dark {
        c.l.clamp(0.58, 0.68)
    } else {
        c.l.clamp(0.45, 0.55)
    };
    c.a = 1.0;
    c.to_rgb()
}

pub fn sidebar_width() -> Pixels {
    px(260.)
}
