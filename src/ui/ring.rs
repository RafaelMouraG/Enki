//! The circular progress ring around the clock.

use std::f32::consts::{FRAC_PI_2, TAU};

use gpui::{canvas, point, px, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Styled};

/// `progress` 0..=1 is drawn clockwise from 12 o'clock with rounded ends.
pub fn ring(
    progress: f32,
    diameter: Pixels,
    stroke: Pixels,
    track: Hsla,
    color: Hsla,
) -> impl IntoElement {
    let progress = progress.clamp(0.0, 1.0);
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let center = bounds.center();
            let radius = (bounds.size.width.min(bounds.size.height) - stroke) / 2.0;

            if let Some(path) = arc(center, radius, 0.0, 1.0, stroke) {
                window.paint_path(path, track);
            }
            if progress <= 0.0 {
                return;
            }
            if let Some(path) = arc(center, radius, 0.0, progress, stroke) {
                window.paint_path(path, color);
            }
            // Round caps.
            for t in [0.0, progress] {
                if let Some(dot) = disc(point_on(center, radius, t), stroke / 2.0) {
                    window.paint_path(dot, color);
                }
            }
        },
    )
    .size(diameter)
    .flex_none()
}

fn point_on(center: Point<Pixels>, radius: Pixels, t: f32) -> Point<Pixels> {
    let angle = t * TAU - FRAC_PI_2;
    point(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}

fn arc(
    center: Point<Pixels>,
    radius: Pixels,
    from: f32,
    to: f32,
    stroke: Pixels,
) -> Option<gpui::Path<Pixels>> {
    let span = (to - from).max(0.0);
    // Enough segments that the polygon reads as a circle at any size we use.
    let steps = ((span * 240.0).ceil() as usize).max(2);
    let mut builder = PathBuilder::stroke(stroke);
    builder.move_to(point_on(center, radius, from));
    for i in 1..=steps {
        builder.line_to(point_on(
            center,
            radius,
            from + span * i as f32 / steps as f32,
        ));
    }
    if span >= 1.0 {
        builder.close();
    }
    builder.build().ok()
}

fn disc(center: Point<Pixels>, radius: Pixels) -> Option<gpui::Path<Pixels>> {
    let mut builder = PathBuilder::fill();
    let steps = 32;
    for i in 0..steps {
        let angle = i as f32 / steps as f32 * TAU;
        let p = point(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        );
        if i == 0 {
            builder.move_to(p);
        } else {
            builder.line_to(p);
        }
    }
    builder.close();
    builder.build().ok()
}

/// Tiny ring used in status rows (like Zeron's "30%" indicators).
pub fn mini_ring(progress: f32, track: Hsla, color: Hsla) -> impl IntoElement {
    ring(progress, px(14.), px(2.), track, color)
}
