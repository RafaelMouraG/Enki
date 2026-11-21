//! User preferences, persisted as JSON in the platform config directory.

use std::{
    fs::File,
    io::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

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
    pub auto_pause_media: bool,
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
            auto_pause_media: false,
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
        Self::load_from(&Self::path())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::path())
    }

    fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = tmp_path(path);
        let bytes = serde_json::to_vec_pretty(self)?;
        {
            let mut file = File::create(&tmp)?;
            file.write_all(&bytes)?;
            let _ = file.sync_data();
        }
        if let Err(err) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(err.into());
        }
        Ok(())
    }

    fn path() -> PathBuf {
        crate::paths::config_dir().join("settings.json")
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "settings.json".into());
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_settings_path(label: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "enki-settings-test-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        let path = dir.join("settings.json");
        (dir, path)
    }

    #[test]
    fn atomic_save_and_load_roundtrip() {
        let (dir, path) = temp_settings_path("roundtrip");
        let custom = Settings {
            color_scheme: ColorScheme::Dark,
            accent: Accent::Purple,
            match_wallpaper: false,
            glass: Glass::Solid,
            focus_minutes: 45,
            short_break_minutes: 10,
            long_break_minutes: 20,
            long_break_every: 3,
            auto_start_breaks: false,
            auto_start_focus: true,
            daily_goal: 12,
            sound: Sound::Bell,
            mini_on_top: false,
            ..Settings::default()
        };

        custom.save_to(&path).expect("save settings");
        assert!(path.exists());
        assert!(!tmp_path(&path).exists(), "temp file should be renamed away");
        assert_eq!(Settings::load_from(&path), custom);

        // Overwrite atomically with updated values.
        let updated = Settings {
            focus_minutes: 50,
            mini_on_top: true,
            ..custom
        };
        updated.save_to(&path).expect("overwrite settings");
        assert!(!tmp_path(&path).exists());
        assert_eq!(Settings::load_from(&path), updated);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_and_missing_json_fall_back_to_default() {
        let (dir, path) = temp_settings_path("invalid");
        assert_eq!(Settings::load_from(&path), Settings::default());

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, b"{ not valid json").unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());

        // Partial JSON fills missing fields from Default.
        std::fs::write(&path, br#"{"focus_minutes": 30, "mini_on_top": false}"#).unwrap();
        let loaded = Settings::load_from(&path);
        assert_eq!(loaded.focus_minutes, 30);
        assert!(!loaded.mini_on_top);
        assert_eq!(loaded.short_break_minutes, Settings::default().short_break_minutes);

        let _ = std::fs::remove_dir_all(dir);
    }
}
