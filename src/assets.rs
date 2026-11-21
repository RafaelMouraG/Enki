//! Icons are compiled into the binary so the app is a single file.
//! Most are from Lucide (ISC license), redrawn at a thinner stroke.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

macro_rules! icons {
    ($($variant:ident => $file:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            pub fn path(self) -> SharedString {
                match self {
                    $(Icon::$variant => concat!("icons/", $file, ".svg").into(),)*
                }
            }
        }

        fn embedded(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $file, ".svg") => {
                    Some(include_bytes!(concat!("../assets/icons/", $file, ".svg")))
                })*
                _ => None,
            }
        }

        const ALL_PATHS: &[&str] = &[$(concat!("icons/", $file, ".svg"),)*];
    };
}

icons! {
    ArrowLeft => "arrow-left",
    Bell => "bell",
    Chart => "chart",
    Check => "check",
    ChevronDown => "chevron-down",
    ChevronUp => "chevron-up",
    Clock => "clock",
    Coffee => "coffee",
    Expand => "expand",
    Flame => "flame",
    Image => "image",
    Keyboard => "keyboard",
    ListChecks => "list-checks",
    Mini => "mini",
    Minus => "minus",
    Monitor => "monitor",
    Moon => "moon",
    Music => "music",
    PanelLeft => "panel-left",
    Pause => "pause",
    Pencil => "pencil",
    Play => "play",
    Plus => "plus",
    RotateCcw => "rotate-ccw",
    Settings => "settings",
    Skip => "skip",
    SkipBack => "skip-back",
    SkipForward => "skip-forward",
    Sliders => "sliders",
    Sun => "sun",
    Target => "target",
    Timer => "timer",
    Tomato => "tomato",
    Trash => "trash",
    Trophy => "trophy",
    WinClose => "win-close",
    WinMax => "win-max",
    WinMin => "win-min",
    X => "x",
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(embedded(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ALL_PATHS
            .iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(*p))
            .collect())
    }
}
