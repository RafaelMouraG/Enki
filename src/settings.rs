//! User preferences, persisted as JSON in the platform config directory.

use std::{path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};

use crate::timer::Durations;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    Purple,
    Orange,
    Yellow,
    Green,
    Cyan,
    #[default]
    Blue,
    Pink,
}

impl Accent {
    pub const ALL: [Accent; 7] = [
        Accent::Purple,
        Accent::Orange,
        Accent::Yellow,
        Accent::Green,
        Accent::Cyan,
        Accent::Blue,
        Accent::Pink,
    ];

    pub fn rgb(self) -> u32 {
        match self {
            Accent::Purple => 0x8b7cf6,
            Accent::Orange => 0xf0954a,
            Accent::Yellow => 0xf2c14e,
            Accent::Green => 0x6fd38a,
            Accent::Cyan => 0x4fd1e0,
            Accent::Blue => 0x5b9cf0,
            Accent::Pink => 0xec6fb5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Accent::Purple => "Roxo",
            Accent::Orange => "Laranja",
            Accent::Yellow => "Amarelo",
            Accent::Green => "Verde",
            Accent::Cyan => "Ciano",
            Accent::Blue => "Azul",
            Accent::Pink => "Rosa",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Glass {
    /// macOS Liquid Glass where the system has it, drawn glass elsewhere.
    #[default]
    #[serde(alias = "theme_default")]
    Liquid,
    /// See-through surfaces over the background image, no system effects.
    #[serde(alias = "frosted")]
    Translucent,
    Solid,
}

impl Glass {
    pub const ALL: [Glass; 3] = [Glass::Liquid, Glass::Translucent, Glass::Solid];

    pub fn label(self) -> &'static str {
        match self {
            Glass::Liquid => "Vidro líquido",
            Glass::Translucent => "Translúcido",
            Glass::Solid => "Sólido",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundEffect {
    None,
    #[default]
    Blur,
    StrongBlur,
}

impl BackgroundEffect {
    pub const ALL: [BackgroundEffect; 3] = [
        BackgroundEffect::None,
        BackgroundEffect::Blur,
        BackgroundEffect::StrongBlur,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BackgroundEffect::None => "Nenhum",
            BackgroundEffect::Blur => "Desfoque",
            BackgroundEffect::StrongBlur => "Desfoque forte",
        }
    }

    pub fn sigma(self) -> f32 {
        match self {
            BackgroundEffect::None => 0.0,
            BackgroundEffect::Blur => 8.0,
            BackgroundEffect::StrongBlur => 22.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    None,
    #[default]
    Chime,
    Bell,
    Soft,
}

impl Sound {
    pub const ALL: [Sound; 4] = [Sound::None, Sound::Chime, Sound::Bell, Sound::Soft];

    pub fn label(self) -> &'static str {
        match self {
            Sound::None => "Sem som",
            Sound::Chime => "Sino",
            Sound::Bell => "Campainha",
            Sound::Soft => "Suave",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Appearance
    pub color_scheme: ColorScheme,
    pub accent: Accent,
    pub match_wallpaper: bool,
    pub glass: Glass,
    pub background_image: Option<PathBuf>,
    pub background_effect: BackgroundEffect,
    /// How much the background image is darkened (or lightened in light mode), 0..=100.
    pub background_dim: u8,
    pub reduce_motion: bool,
    pub sidebar_open: bool,

    // Timer
    pub focus_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    pub long_break_every: u32,
    pub auto_start_breaks: bool,
    pub auto_start_focus: bool,
    pub daily_goal: u32,

    // Notifications
    pub notifications: bool,
    pub sound: Sound,

    // Integrations
    pub menu_bar: bool,
    pub now_playing: bool,
    pub mini_on_top: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            color_scheme: ColorScheme::System,
            accent: Accent::Blue,
            match_wallpaper: true,
            glass: Glass::Liquid,
            background_image: None,
            background_effect: BackgroundEffect::Blur,
            background_dim: 45,
            reduce_motion: false,
            sidebar_open: true,
            focus_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            long_break_every: 4,
            auto_start_breaks: true,
            auto_start_focus: false,
            daily_goal: 8,
            notifications: true,
            sound: Sound::Chime,
            menu_bar: true,
            now_playing: true,
            mini_on_top: true,
        }
    }
}

impl Settings {
    pub fn durations(&self) -> Durations {
        let mins = |m: u32| Duration::from_secs(u64::from(m.max(1)) * 60);
        Durations {
            focus: mins(self.focus_minutes),
            short_break: mins(self.short_break_minutes),
            long_break: mins(self.long_break_minutes),
            long_break_every: self.long_break_every.max(1),
        }
    }

    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    fn path() -> PathBuf {
        crate::paths::config_dir().join("settings.json")
    }
}
