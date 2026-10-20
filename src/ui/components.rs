//! Small themed building blocks shared by every page.

use std::rc::Rc;

use gpui::{
    canvas, div, prelude::*, px, svg, AnyElement, App, ClickEvent, CursorStyle, DispatchPhase, Div,
    ElementId, FontWeight, HitboxBehavior, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, SharedString, Stateful, Svg, Window,
};

use crate::{
    assets::Icon,
    native::{self, GlassStyle, Haptic},
    theme::{radius, with_alpha, Theme},
};

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

/// Square ghost button with a single icon, like the ones in Zeron's title bar.
pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: Icon,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (muted, text, hover) = (theme.text_muted, theme.text, theme.surface_hover);
    div()
        .id(id)
        .group("icon-button")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(radius::MD)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.8))
        .child(
            icon(glyph)
                .text_color(muted)
                .group_hover("icon-button", move |s| s.text_color(text)),
        )
        .on_click(on_click)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Secondary,
    Danger,
    Ghost,
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (bg, fg, border, hover_bg) = match kind {
        ButtonKind::Secondary => (
            theme.control,
            theme.text,
            theme.border_strong,
            theme.control_hover,
        ),
        ButtonKind::Danger => (
            theme.control,
            theme.danger,
            theme.border_strong,
            theme.control_hover,
        ),
        ButtonKind::Ghost => (
            with_alpha(theme.control, 0.),
            theme.text_muted,
            with_alpha(theme.control, 0.),
            theme.surface_hover,
        ),
    };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(32.))
        .px(px(12.))
        .rounded(radius::MD)
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .active(|s| s.opacity(0.85))
        .child(label.into())
        .on_click(on_click)
}

/// iOS-style switch.
pub fn toggle(
    id: impl Into<ElementId>,
    on: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
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

/// Pill-shaped segmented control.
pub fn segmented(id: impl Into<SharedString>, theme: &Theme, options: Vec<SegmentOption>) -> Div {
    let id: SharedString = id.into();
    let liquid = theme.liquid();
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
        .children(options.into_iter().enumerate().map(|(ix, opt)| {
            let hover = theme.surface_hover;
            div()
                .id(SharedString::from(format!("{id}-{ix}")))
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(28.))
                .px(px(14.))
                .rounded_full()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .map(|d| {
                    if opt.selected {
                        d.bg(theme.selected_bg)
                            .text_color(theme.text)
                            .when(!liquid, |d| d.shadow(theme.card_shadow()))
                    } else {
                        d.text_color(theme.text_muted).hover(move |s| s.bg(hover))
                    }
                })
                .when_some(opt.icon, |d, glyph| {
                    d.child(icon(glyph).size(px(14.)).text_color(theme.text_muted))
                })
                .child(opt.label)
                .when(!opt.selected, |d| {
                    d.on_mouse_down(gpui::MouseButton::Left, |_, _, _| {
                        native::haptic(Haptic::Alignment)
                    })
                })
                .on_click(opt.on_click)
        }))
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
