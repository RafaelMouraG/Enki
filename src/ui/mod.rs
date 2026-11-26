pub mod components;
pub mod mini;
mod player;
pub mod ring;
pub mod root;
mod settings_page;
mod sidebar;
mod stats_page;
mod tasks_page;
pub mod text_input;
mod timer_page;
mod wrapped_page;

use gpui::{App, ClickEvent, Context, Entity, Window};

use crate::{state::Enki, theme::sidebar_width};

/// Usable dimensions `(width, height)` of the main content area (excluding the
/// sidebar when open and the title bar).
pub fn content_viewport(window: &Window, sidebar_open: bool) -> (f32, f32) {
    let vp = window.viewport_size();
    let sidebar = if sidebar_open {
        f32::from(sidebar_width())
    } else {
        0.
    };
    let w = (f32::from(vp.width) - sidebar).max(200.);
    let h = (f32::from(vp.height) - root::TITLEBAR_HEIGHT).max(200.);
    (w, h)
}

/// Responsive horizontal page padding for a given main-area width.
pub fn page_padding_x(avail_w: f32) -> f32 {
    if avail_w < 520. {
        16.
    } else if avail_w < 680. {
        24.
    } else {
        32.
    }
}

/// Click handler that runs `f` against the shared model.
pub fn on_model(
    model: &Entity<Enki>,
    f: impl Fn(&mut Enki, &mut Context<Enki>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let model = model.clone();
    move |_, _, cx| model.update(cx, |m, cx| f(m, cx))
}

pub fn format_minutes(seconds: u64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn minutes() {
        assert_eq!(super::format_minutes(59), "0 min");
        assert_eq!(super::format_minutes(25 * 60), "25 min");
        assert_eq!(super::format_minutes(120 * 60), "2 h");
        assert_eq!(super::format_minutes(75 * 60 + 30), "1 h 15 min");
    }
}
