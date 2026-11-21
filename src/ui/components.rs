//! Small themed building blocks shared by every page.

use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{
    canvas, div, linear_color_stop, linear_gradient, point, prelude::*, px, svg, Animation,
    AnimationExt as _, AnyElement, App, BoxShadow, ClickEvent, CursorStyle, DispatchPhase, Div,
    ElementId, FontWeight, HitboxBehavior, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, SharedString, Svg, Window,
};

use crate::{
    assets::Icon,
    native::{self, GlassStyle, Haptic},
    theme::{mix_hsla, radius, with_alpha, Theme},
};

/// Smooth cubic deceleration for color, sheen and shadow transitions.
pub fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t.clamp(0.0, 1.0);
    1.0 - inv * inv * inv
}

#[derive(Clone)]
struct HoverState {
    hovered: bool,
    from: f32,
    to: f32,
    current: Rc<Cell<f32>>,
    epoch: u64,
}

impl Default for HoverState {
    fn default() -> Self {
        Self {
            hovered: false,
            from: 0.0,
            to: 0.0,
            current: Rc::new(Cell::new(0.0)),
            epoch: 0,
        }
    }
}

/// Interruptible, keyed hover animation handle. Rapid enter/leave transitions
/// pick up from the exact in-flight visual progress with zero jump.
#[derive(Clone)]
pub struct HoverTrack {
    key: SharedString,
    from: f32,
    to: f32,
    current: Rc<Cell<f32>>,
    epoch: u64,
    reduce_motion: bool,
    state: gpui::Entity<HoverState>,
}

impl HoverTrack {
    pub fn on_hover(&self) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
        let state = self.state.clone();
        let reduce_motion = self.reduce_motion;
        move |entered, _, cx| {
            state.update(cx, |s, cx| {
                if s.hovered == *entered {
                    return;
                }
                s.hovered = *entered;
                let target = if *entered { 1.0 } else { 0.0 };
                s.from = if reduce_motion {
                    target
                } else {
                    s.current.get().clamp(0.0, 1.0)
                };
                s.to = target;
                s.epoch = s.epoch.wrapping_add(1);
                cx.notify();
            });
        }
    }

    /// Runs `apply(el, t)` with `t` smoothly interpolated between `0.0` and `1.0`.
    pub fn animate<E: IntoElement + 'static>(
        &self,
        el: E,
        duration_ms: u64,
        easing: impl Fn(f32) -> f32 + 'static,
        apply: impl Fn(E, f32) -> E + 'static,
    ) -> AnyElement {
        if self.epoch == 0 || self.reduce_motion || (self.to - self.from).abs() < 1e-4 {
            self.current.set(self.to);
            return apply(el, self.to).into_any_element();
        }
        let from = self.from;
        let to = self.to;
        let current = self.current.clone();
        el.with_animation(
            SharedString::from(format!("{}-hv-{}", self.key, self.epoch)),
            Animation::new(Duration::from_millis(duration_ms)).with_easing(easing),
            move |el, delta| {
                let raw = from + (to - from) * delta;
                current.set(raw.clamp(0.0, 1.0));
                apply(el, raw)
            },
        )
        .into_any_element()
    }
}

pub fn hover_track(
    id: impl Into<SharedString>,
    reduce_motion: bool,
    window: &mut Window,
    cx: &mut App,
) -> HoverTrack {
    let key: SharedString = id.into();
    let state = window.use_keyed_state(
        ElementId::Name(format!("hover-track-{key}").into()),
        cx,
        |_, _| HoverState::default(),
    );
    let snap = state.read(cx);
    HoverTrack {
        key,
        from: snap.from,
        to: snap.to,
        current: snap.current.clone(),
        epoch: snap.epoch,
        reduce_motion,
        state,
    }
}

/// Soft, neutral diffused depth shadow for liquid glass controls (never harsh).
pub fn glass_shadow(is_dark: bool, rest: f32, t: f32) -> Vec<BoxShadow> {
    let t = t.clamp(0.0, 1.0);
    let strength = rest + (1.0 - rest) * t;
    if strength <= 0.01 {
        return Vec::new();
    }
    let alpha = if is_dark { 0.34 } else { 0.11 } * strength;
    let ambient = if is_dark { 0.16 } else { 0.05 } * strength;
    vec![
        BoxShadow {
            color: with_alpha(gpui::black(), alpha),
            offset: point(px(0.), px(2. + 6. * t)),
            blur_radius: px(10. + 14. * t),
            spread_radius: px(-3. - t),
        },
        BoxShadow {
            color: with_alpha(gpui::black(), ambient),
            offset: point(px(0.), px(1.)),
            blur_radius: px(3. + 2. * t),
            spread_radius: px(0.),
        },
    ]
}

/// Monochromatic top meniscus highlight + subtle rotating specular sheen that
/// glides smoothly across a rounded liquid-glass surface as `t: 0.0 → 1.0`.
/// Every layer fills the full rounded quad so GPUI's SDF shader clips cleanly
/// to circles and capsules with zero rectangular corner bleed.
pub fn liquid_highlights(radius: Pixels, is_dark: bool, base: f32, t: f32) -> AnyElement {
    let t = t.clamp(0.0, 1.0);
    let top_alpha = if is_dark {
        0.10 * base + 0.14 * t
    } else {
        0.48 * base + 0.30 * t
    };
    let rim_alpha = if is_dark {
        0.18 * base + 0.24 * t
    } else {
        0.70 * base + 0.28 * t
    };
    let sheen_alpha = if is_dark { 0.14 * t } else { 0.38 * t };
    let sheen_angle = 115. + 60. * t;

    div()
        .absolute()
        .inset_0()
        .rounded(radius)
        .bg(linear_gradient(
            180.,
            linear_color_stop(with_alpha(gpui::white(), top_alpha), 0.0),
            linear_color_stop(with_alpha(gpui::white(), 0.0), 0.55),
        ))
        .when(t > 0.01, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(radius)
                    .bg(linear_gradient(
                        sheen_angle,
                        linear_color_stop(with_alpha(gpui::white(), sheen_alpha), 0.0),
                        linear_color_stop(with_alpha(gpui::white(), 0.0), 0.65),
                    )),
            )
        })
        .child(
            // Crisp 1px top catchlight following the rounded edge.
            div()
                .absolute()
                .inset_0()
                .rounded(radius)
                .border_t_1()
                .border_color(with_alpha(gpui::white(), rim_alpha)),
        )
        .into_any_element()
}

/// Single-line text with an ellipsis. GPUI's own `truncate()` caches the
/// unconstrained width when combined with `nowrap` and never truncates, while
/// a one-line clamp is re-measured against the real width.
pub trait StyledExt: Styled + Sized {
    fn ellipsis(self) -> Self {
        self.overflow_hidden().line_clamp(1).text_ellipsis()
    }
}

impl<T: Styled> StyledExt for T {}

pub trait SurfaceExt: Styled + ParentElement + Sized {
    /// A card surface: glass in the liquid style, a filled panel otherwise.
    /// Call it on a `relative` element before adding other children.
    fn surface(self, key: impl Into<SharedString>, radius: Pixels, theme: &Theme) -> Self {
        if theme.liquid() {
            self.child(glass(key, radius, theme))
        } else {
            self.bg(theme.surface).border_1().border_color(theme.border)
        }
    }
}

impl<T: Styled + ParentElement> SurfaceExt for T {}

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Moves a client-decorated window when its title area is dragged. The move
/// starts once the pointer travels a few pixels with the button held, not on
/// the press: on Wayland the compositor grabs the pointer for the move, which
/// would swallow clicks on the buttons inside the area. Keep one per view so
/// the press survives re-renders.
#[derive(Clone, Default)]
pub struct WindowDrag(Rc<std::cell::Cell<Option<gpui::Point<Pixels>>>>);

impl WindowDrag {
    const THRESHOLD: Pixels = px(4.);

    pub fn attach<E: InteractiveElement>(&self, area: E) -> E {
        let (press, origin) = (self.0.clone(), self.0.clone());
        area.on_mouse_down(MouseButton::Left, move |event, _, _| {
            press.set(Some(event.position))
        })
        .on_mouse_move(move |event: &MouseMoveEvent, window, _| {
            let Some(start) = origin.get() else { return };
            if event.pressed_button != Some(MouseButton::Left) {
                origin.set(None);
            } else if (event.position.x - start.x).abs() > Self::THRESHOLD
                || (event.position.y - start.y).abs() > Self::THRESHOLD
            {
                origin.set(None);
                window.start_window_move();
            }
        })
    }
}

/// The backing of a glass surface. Put it first inside a `relative` container
/// with the same corner radius. With native glass it places a Liquid Glass
/// view under the container; otherwise it draws a translucent fill and a rim.
pub fn glass(key: impl Into<SharedString>, radius: Pixels, theme: &Theme) -> AnyElement {
    glass_styled(key, radius, false, theme)
}

/// `clear` is the more see-through variant, for small controls.
pub fn glass_styled(
    key: impl Into<SharedString>,
    radius: Pixels,
    clear: bool,
    theme: &Theme,
) -> AnyElement {
    if theme.native_glass {
        let key = key.into();
        let style = GlassStyle {
            radius,
            tint: theme.glass_tint,
            clear,
            dark: theme.is_dark,
        };
        return canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                // Scrolled out of view: the native view isn't clipped by GPUI.
                let visible = bounds.intersect(&window.content_mask().bounds);
                if visible.size.width > px(1.) && visible.size.height > px(1.) {
                    native::glass_place(window, key.clone(), visible, &style);
                }
            },
        )
        .absolute()
        .inset_0()
        .into_any_element();
    }
    div()
        .absolute()
        .inset_0()
        .rounded(radius)
        .bg(theme.glass_fill)
        .border_1()
        .border_color(theme.glass_rim)
        .when(theme.drawn_glass, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(radius)
                    .border_t_1()
                    .border_color(theme.glass_highlight),
            )
        })
        .into_any_element()
}

pub fn icon(icon: Icon) -> Svg {
    svg().path(icon.path()).size(px(16.)).flex_none()
}

pub fn icon_sized(icon: Icon, size: Pixels, color: Hsla) -> Svg {
    svg()
        .path(icon.path())
        .size(size)
        .flex_none()
        .text_color(color)
}

/// Circular frosted-glass icon button with monochromatic specular hover.
pub fn icon_button(
    id: impl Into<SharedString>,
    glyph: Icon,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    icon_button_sized(id, glyph, px(30.), theme, window, cx, on_click)
}

pub fn icon_button_sized(
    id: impl Into<SharedString>,
    glyph: Icon,
    box_size: Pixels,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let track = hover_track(id.clone(), theme.reduce_motion, window, cx);
    let (muted, text, is_dark) = (theme.text_muted, theme.text, theme.is_dark);
    let radius = box_size / 2.0;
    let rest_fill = with_alpha(gpui::white(), 0.0);
    let hover_fill = with_alpha(gpui::white(), if is_dark { 0.13 } else { 0.72 });
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
        if is_dark { 0.24 } else { 0.12 },
    );

    let visual = track.animate(
        div()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .rounded_full()
            .border_1(),
        220,
        ease_out_cubic,
        move |d, t| {
            let tc = t.clamp(0.0, 1.0);
            let glyph_color = mix_hsla(muted, text, tc);
            d.bg(mix_hsla(rest_fill, hover_fill, tc))
                .border_color(mix_hsla(rest_border, hover_border, tc))
                .shadow(glass_shadow(is_dark, 0.0, tc))
                .when(tc > 0.01, |d| {
                    d.child(liquid_highlights(radius, is_dark, 0.0, tc))
                })
                .child(div().relative().child(icon(glyph).text_color(glyph_color)))
        },
    );

    div()
        .id(id)
        .flex()
        .flex_none()
        .size(box_size)
        .rounded_full()
        .cursor_pointer()
        .active(|s| s.opacity(0.8))
        .on_hover(track.on_hover())
        .on_click(on_click)
        .child(visual)
        .into_any_element()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Secondary,
    Danger,
    Ghost,
}

/// Rounded liquid-glass capsule button: frosted transparency, monochromatic
/// top catchlight, soft diffused shadow and fluid specular sweep on hover.
pub fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let label: SharedString = label.into();
    let track = hover_track(id.clone(), theme.reduce_motion, window, cx);
    let is_dark = theme.is_dark;
    let liquid = theme.liquid();
    let use_native = theme.native_glass && kind != ButtonKind::Ghost;
    let glass_theme = theme.clone();
    let glass_key = SharedString::from(format!("{id}-lg"));

    let (bg, hover_bg, fg, hover_fg, border, hover_border, base_highlight, base_shadow) = match kind
    {
        ButtonKind::Secondary => (
            if use_native {
                with_alpha(gpui::white(), if is_dark { 0.04 } else { 0.25 })
            } else if is_dark {
                with_alpha(gpui::white(), 0.075)
            } else {
                with_alpha(gpui::white(), 0.65)
            },
            if use_native {
                with_alpha(gpui::white(), if is_dark { 0.14 } else { 0.55 })
            } else if is_dark {
                with_alpha(gpui::white(), 0.15)
            } else {
                with_alpha(gpui::white(), 0.88)
            },
            theme.text,
            theme.text,
            with_alpha(
                if is_dark {
                    gpui::white()
                } else {
                    gpui::black()
                },
                if is_dark { 0.15 } else { 0.1 },
            ),
            with_alpha(
                if is_dark {
                    gpui::white()
                } else {
                    gpui::black()
                },
                if is_dark { 0.34 } else { 0.18 },
            ),
            1.0,
            0.35,
        ),
        ButtonKind::Danger => (
            if is_dark {
                with_alpha(gpui::white(), 0.06)
            } else {
                with_alpha(gpui::white(), 0.6)
            },
            if is_dark {
                with_alpha(gpui::white(), 0.13)
            } else {
                with_alpha(gpui::white(), 0.85)
            },
            theme.danger,
            theme.danger,
            with_alpha(
                if is_dark {
                    gpui::white()
                } else {
                    gpui::black()
                },
                if is_dark { 0.14 } else { 0.1 },
            ),
            with_alpha(theme.danger, 0.5),
            0.85,
            0.3,
        ),
        ButtonKind::Ghost => (
            with_alpha(gpui::white(), 0.0),
            with_alpha(gpui::white(), if is_dark { 0.1 } else { 0.65 }),
            theme.text_muted,
            theme.text,
            with_alpha(gpui::white(), 0.0),
            with_alpha(
                if is_dark {
                    gpui::white()
                } else {
                    gpui::black()
                },
                if is_dark { 0.22 } else { 0.12 },
            ),
            0.0,
            0.0,
        ),
    };

    let pill_radius = px(17.);
    let visual = track.animate(
        div()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h_full()
            .px(px(16.))
            .rounded_full()
            .border_1()
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM),
        230,
        ease_out_cubic,
        move |d, t| {
            let tc = t.clamp(0.0, 1.0);
            d.bg(mix_hsla(bg, hover_bg, tc))
                .border_color(mix_hsla(border, hover_border, tc))
                .text_color(mix_hsla(fg, hover_fg, tc))
                .shadow(glass_shadow(is_dark, base_shadow, tc))
                .when(liquid || tc > 0.01 || base_highlight > 0.0, |d| {
                    d.child(liquid_highlights(pill_radius, is_dark, base_highlight, tc))
                })
                .child(div().relative().child(label.clone()))
        },
    );

    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .h(px(34.))
        .rounded_full()
        .cursor_pointer()
        .active(|s| s.opacity(0.84))
        .on_hover(track.on_hover())
        .on_click(on_click)
        .when(use_native, |d| {
            d.child(glass_styled(glass_key, pill_radius, true, &glass_theme))
        })
        .child(visual)
        .into_any_element()
}

/// iOS-style switch.
pub fn toggle(
    id: impl Into<ElementId>,
    on: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    let track = if on {
        theme.accent
    } else {
        theme.surface_active
    };
    let knob = if on || theme.is_dark {
        gpui::white()
    } else {
        theme.control
    };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .w(px(38.))
        .h(px(22.))
        .p(px(2.))
        .rounded_full()
        .bg(track)
        .border_1()
        .border_color(if on { track } else { theme.border_strong })
        .cursor_pointer()
        .when(on, |d| d.justify_end())
        .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {
            native::haptic(Haptic::LevelChange)
        })
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .bg(knob)
                .shadow(vec![gpui::BoxShadow {
                    color: with_alpha(gpui::black(), 0.25),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                }]),
        )
        .on_click(on_click)
}

/// A rounded group of rows, Zeron's settings card.
pub fn card(key: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .w_full()
        .rounded(radius::LG)
        .surface(key, radius::LG, theme)
        .overflow_hidden()
}

/// One labeled row inside a card. Rows after the first get a hairline on top.
pub fn setting_row(
    theme: &Theme,
    first: bool,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .px(px(16.))
        .py(px(12.))
        .min_h(px(56.))
        .when(!first, |d| d.border_t_1().border_color(theme.border))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(title.into()),
                )
                .when_some(description, |d, desc| {
                    d.child(
                        div()
                            .text_size(px(12.5))
                            .text_color(theme.text_muted)
                            .child(desc),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.))
                .child(control),
        )
}

pub fn section_label(theme: &Theme, text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(14.))
        .text_color(theme.text_muted)
        .pt(px(8.))
        .pb(px(10.))
        .child(text.into())
}

/// `−  25 min  +`
pub fn stepper(
    id: impl Into<SharedString>,
    value: impl Into<SharedString>,
    theme: &Theme,
    on_dec: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_inc: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let id: SharedString = id.into();
    let side = |suffix: &str, glyph: Icon, handler: ClickHandler| {
        let hover = theme.control_hover;
        div()
            .id(SharedString::from(format!("{id}-{suffix}")))
            .flex()
            .items_center()
            .justify_center()
            .size(px(28.))
            .rounded(radius::SM)
            .text_color(theme.text_muted)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(icon_sized(glyph, px(14.), theme.text_muted))
            .on_click(handler)
    };
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .rounded(radius::MD)
        .bg(theme.control)
        .border_1()
        .border_color(theme.border_strong)
        .child(side("dec", Icon::Minus, Box::new(on_dec)))
        .child(
            div()
                .min_w(px(64.))
                .text_center()
                .text_size(px(13.))
                .font(crate::theme::tabular_font())
                .text_color(theme.text)
                .child(value.into()),
        )
        .child(side("inc", Icon::Plus, Box::new(on_inc)))
}

pub struct SegmentOption {
    pub label: SharedString,
    pub icon: Option<Icon>,
    pub selected: bool,
    pub on_click: ClickHandler,
}

/// Pill-shaped segmented control with fluid hover transitions.
pub fn segmented(
    id: impl Into<SharedString>,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    options: Vec<SegmentOption>,
) -> Div {
    let id: SharedString = id.into();
    let liquid = theme.liquid();
    let mut items = Vec::with_capacity(options.len());
    for (ix, opt) in options.into_iter().enumerate() {
        let seg_id = SharedString::from(format!("{id}-{ix}"));
        let track = hover_track(seg_id.clone(), theme.reduce_motion, window, cx);
        let selected = opt.selected;
        let is_dark = theme.is_dark;
        let selected_bg = theme.selected_bg;
        let hover_bg = with_alpha(gpui::white(), if is_dark { 0.09 } else { 0.55 });
        let rest_bg = with_alpha(gpui::white(), 0.0);
        let text = theme.text;
        let muted = theme.text_muted;
        let shadow = (!liquid).then(|| theme.card_shadow());
        let glyph = opt.icon;
        let label = opt.label;

        let item = if selected {
            div()
                .id(seg_id)
                .relative()
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(28.))
                .px(px(14.))
                .rounded_full()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .bg(selected_bg)
                .text_color(text)
                .when_some(shadow, |d, s| d.shadow(s))
                .child(liquid_highlights(px(14.), is_dark, 0.65, 0.0))
                .when_some(glyph, |d, g| d.child(icon(g).size(px(14.)).text_color(text)))
                .child(div().relative().child(label))
                .on_click(opt.on_click)
                .into_any_element()
        } else {
            let visual = track.animate(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h_full()
                    .px(px(14.))
                    .rounded_full()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM),
                210,
                ease_out_cubic,
                move |d, t| {
                    let tc = t.clamp(0.0, 1.0);
                    let fg = mix_hsla(muted, text, tc);
                    d.bg(mix_hsla(rest_bg, hover_bg, tc))
                        .text_color(fg)
                        .when(tc > 0.01, |d| {
                            d.child(liquid_highlights(px(14.), is_dark, 0.0, tc * 0.7))
                        })
                        .when_some(glyph, |d, g| d.child(icon(g).size(px(14.)).text_color(fg)))
                        .child(div().relative().child(label.clone()))
                },
            );
            div()
                .id(seg_id)
                .h(px(28.))
                .rounded_full()
                .cursor_pointer()
                .on_hover(track.on_hover())
                .on_mouse_down(gpui::MouseButton::Left, |_, _, _| {
                    native::haptic(Haptic::Alignment)
                })
                .on_click(opt.on_click)
                .child(visual)
                .into_any_element()
        };
        items.push(item);
    }

    div()
        .relative()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(3.))
        .rounded_full()
        .map(|d| {
            if liquid {
                // 28px segments + 2×3px padding + 2×1px border.
                d.border_1()
                    .border_color(gpui::transparent_black())
                    .child(glass_styled(format!("{id}-glass"), px(18.), true, theme))
            } else {
                d.bg(with_alpha(
                    theme.surface,
                    if theme.is_dark { 0.7 } else { 0.85 },
                ))
                .border_1()
                .border_color(theme.border)
            }
        })
        .children(items)
}

pub fn keycap(theme: &Theme, key: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(24.))
        .h(px(24.))
        .px(px(6.))
        .rounded(radius::SM)
        .bg(theme.control)
        .border_1()
        .border_color(theme.border_strong)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text_muted)
        .child(key.into())
}

pub fn page_title(theme: &Theme, title: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(24.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(title.into())
}

#[derive(Default)]
struct SliderDrag {
    active: bool,
    last: usize,
}

const SLIDER_WIDTH: f32 = 260.;
const SLIDER_HEIGHT: f32 = 26.;
const KNOB_WIDTH: f32 = 38.;

/// A track with a dot per stop and a capsule knob, like macOS's effort
/// slider. Dragging snaps to the stops with a trackpad tick at each one.
pub fn slider(
    id: impl Into<SharedString>,
    stops: usize,
    value: usize,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    on_change: impl Fn(usize, &mut App) + 'static,
) -> AnyElement {
    let id: SharedString = id.into();
    let stops = stops.max(2);
    let value = value.min(stops - 1);
    let state = window.use_keyed_state(ElementId::Name(format!("{id}-drag").into()), cx, |_, _| {
        SliderDrag::default()
    });
    let on_change = Rc::new(on_change);

    let pad = KNOB_WIDTH / 2.;
    let span = SLIDER_WIDTH - KNOB_WIDTH;
    let x_of = move |ix: usize| pad + span * ix as f32 / (stops - 1) as f32;
    let (track, dot_off, dot_on) = if theme.is_dark {
        (
            with_alpha(gpui::white(), 0.08),
            with_alpha(gpui::white(), 0.28),
            with_alpha(theme.on_accent, 0.55),
        )
    } else {
        (
            with_alpha(gpui::black(), 0.07),
            with_alpha(gpui::black(), 0.22),
            with_alpha(theme.on_accent, 0.6),
        )
    };
    let dot = 4.;
    let knob_height = SLIDER_HEIGHT - 4.;

    let interaction = canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, _| {
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
            let index_at = move |x: Pixels| {
                let t = ((f32::from(x - bounds.left()) - pad) / span).clamp(0., 1.);
                (t * (stops - 1) as f32).round() as usize
            };
            // Ticks and reports each stop crossed, once.
            let step = {
                let state = state.clone();
                let on_change = on_change.clone();
                move |ix: usize, cx: &mut App| {
                    if state.read(cx).last != ix {
                        state.update(cx, |s, _| s.last = ix);
                        native::haptic(Haptic::LevelChange);
                        on_change(ix, cx);
                    }
                }
            };
            {
                let state = state.clone();
                let step = step.clone();
                window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && e.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        state.update(cx, |s, _| {
                            s.active = true;
                            s.last = value;
                        });
                        step(index_at(e.position.x), cx);
                    }
                });
            }
            {
                let state = state.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble
                        && state.read(cx).active
                        && e.pressed_button == Some(MouseButton::Left)
                    {
                        step(index_at(e.position.x), cx);
                    }
                });
            }
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && state.read(cx).active {
                    state.update(cx, |s, _| s.active = false);
                }
            });
        },
    )
    .absolute()
    .inset_0();

    div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(SLIDER_WIDTH))
        .h(px(SLIDER_HEIGHT))
        .rounded_full()
        .bg(track)
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(x_of(value) + pad))
                .rounded_full()
                .bg(theme.accent),
        )
        .children((0..stops).filter(|&ix| ix != value).map(|ix| {
            div()
                .absolute()
                .left(px(x_of(ix) - dot / 2.))
                .top(px((SLIDER_HEIGHT - dot) / 2.))
                .size(px(dot))
                .rounded_full()
                .bg(if ix < value { dot_on } else { dot_off })
        }))
        .child(
            div()
                .absolute()
                .left(px(x_of(value) - pad))
                .top(px(2.))
                .w(px(KNOB_WIDTH))
                .h(px(knob_height))
                .rounded_full()
                .bg(gpui::white())
                .shadow(vec![gpui::BoxShadow {
                    color: with_alpha(gpui::black(), 0.3),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(4.),
                    spread_radius: px(0.),
                }]),
        )
        .child(interaction)
        .into_any_element()
}
