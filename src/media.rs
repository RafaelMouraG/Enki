//! "Now playing" for the sidebar card.
//!
//! Talks to the local player instead of the Spotify Web API, so there is no
//! login and it works on free accounts: AppleScript on macOS (Spotify with
//! Apple Music fallback), MPRIS over D-Bus on Linux (Spotify preferred, any
//! player otherwise).

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    time::{Duration, SystemTime},
};

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying {
    pub player: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// Local copy of the album art once downloaded.
    pub art: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
}

/// Polls the player on a background thread.
pub struct MediaWatcher {
    current: Arc<Mutex<Option<NowPlaying>>>,
    enabled: Arc<AtomicBool>,
    commands: mpsc::Sender<MediaCommand>,
}

const POLL_EVERY: Duration = Duration::from_millis(1500);

impl MediaWatcher {
    pub fn spawn(enabled: bool) -> Self {
        let current = Arc::new(Mutex::new(None));
        let enabled = Arc::new(AtomicBool::new(enabled));
        let (tx, rx) = mpsc::channel::<MediaCommand>();
        {
            let current = current.clone();
            let enabled = enabled.clone();
            std::thread::Builder::new()
                .name("enki-media".into())
                .spawn(move || {
                    // A unit struct on macOS, stateful on Linux.
                    #[allow(clippy::default_constructed_unit_structs)]
                    let mut backend = backend::Backend::default();
                    loop {
                        // Wake up early when a command arrives so the card reacts instantly.
                        match rx.recv_timeout(POLL_EVERY) {
                            Ok(cmd) => {
                                if let Err(err) = backend.command(cmd) {
                                    eprintln!("enki: media command failed: {err}");
                                }
                                std::thread::sleep(Duration::from_millis(150));
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                        let next = if enabled.load(Ordering::Relaxed) {
                            backend.poll().ok().flatten().map(with_cached_art)
                        } else {
                            None
                        };
                        *current.lock().unwrap() = next;
                    }
                })
                .expect("spawning media thread");
        }
        Self {
            current,
            enabled,
            commands: tx,
        }
    }

    pub fn current(&self) -> Option<NowPlaying> {
        self.current.lock().unwrap().clone()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            *self.current.lock().unwrap() = None;
        }
    }

    pub fn send(&self, cmd: MediaCommand) {
        let _ = self.commands.send(cmd);
    }
}

/// Backends report the art as a URL; swap it for a cached local file.
fn with_cached_art(mut np: (NowPlaying, Option<String>)) -> NowPlaying {
    if let Some(url) = np.1.take() {
        np.0.art = cache_art(&url);
    }
    np.0
}

const MAX_ART_CACHE_FILES: usize = 100;
const ART_HTTP_TIMEOUT: Duration = Duration::from_secs(5);

fn art_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(ART_HTTP_TIMEOUT))
            .build();
        ureq::Agent::new_with_config(config)
    })
}

fn touch_modified(path: &Path) {
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(SystemTime::now()));
}

fn prune_art_cache(dir: &Path, max_files: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                (mtime, path)
            })
        })
        .collect();
    if files.len() <= max_files {
        return;
    }
    files.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, path) in files.into_iter().skip(max_files) {
        let _ = std::fs::remove_file(path);
    }
}

fn cache_art(url: &str) -> Option<PathBuf> {
    if let Some(path) = url.strip_prefix("file://") {
        return Some(PathBuf::from(path));
    }
    if !url.starts_with("http") {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    let dir = crate::paths::cache_dir().join("art");
    let path = dir.join(format!("{:016x}.jpg", hasher.finish()));
    if path.exists() {
        touch_modified(&path);
        return Some(path);
    }
    std::fs::create_dir_all(&dir).ok()?;
    let mut body = art_agent().get(url).call().ok()?.into_body();
    let bytes = body
        .with_config()
        .limit(5 * 1024 * 1024)
        .read_to_vec()
        .ok()?;
    std::fs::write(&path, bytes).ok()?;
    prune_art_cache(&dir, MAX_ART_CACHE_FILES);
    Some(path)
}

#[cfg(target_os = "macos")]
mod backend {
    use super::{MediaCommand, NowPlaying};
    use anyhow::{bail, Result};
    use std::process::Command;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum MacPlayer {
        Spotify,
        Music,
    }

    impl MacPlayer {
        fn process_name(self) -> &'static str {
            match self {
                MacPlayer::Spotify => "Spotify",
                MacPlayer::Music => "Music",
            }
        }

        fn display_name(self) -> &'static str {
            match self {
                MacPlayer::Spotify => "Spotify",
                MacPlayer::Music => "Apple Music",
            }
        }

        fn query(self) -> &'static str {
            match self {
                MacPlayer::Spotify => SPOTIFY_QUERY,
                MacPlayer::Music => MUSIC_QUERY,
            }
        }
    }

    #[derive(Default)]
    pub struct Backend {
        active: Option<MacPlayer>,
    }

    const SPOTIFY_QUERY: &str = r#"tell application "Spotify"
    if player state is stopped then return ""
    set t to current track
    return (player state as string) & linefeed & (name of t) & linefeed & (artist of t) & linefeed & (artwork url of t)
end tell"#;

    const MUSIC_QUERY: &str = r#"tell application "Music"
    if player state is stopped then return ""
    try
        set t to current track
        return (player state as string) & linefeed & (name of t) & linefeed & (artist of t)
    on error
        return ""
    end try
end tell"#;

    impl Backend {
        /// Never launches the player: AppleScript is only used while the
        /// process runs, avoiding "Where is ...?" dialogs when not installed.
        fn is_running(process: &str) -> bool {
            Command::new("pgrep")
                .args(["-x", process])
                .output()
                .is_ok_and(|o| o.status.success())
        }

        fn poll_player(player: MacPlayer) -> Result<Option<(NowPlaying, Option<String>)>> {
            if !Self::is_running(player.process_name()) {
                return Ok(None);
            }
            let out = Command::new("osascript")
                .args(["-e", player.query()])
                .output()?;
            if !out.status.success() {
                bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
            }
            Ok(parse(
                player.display_name(),
                &String::from_utf8_lossy(&out.stdout),
            ))
        }

        pub fn poll(&mut self) -> Result<Option<(NowPlaying, Option<String>)>> {
            let spotify = Self::poll_player(MacPlayer::Spotify);
            if let Ok(Some(np)) = spotify {
                self.active = Some(MacPlayer::Spotify);
                return Ok(Some(np));
            }
            let music = Self::poll_player(MacPlayer::Music);
            if let Ok(Some(np)) = music {
                self.active = Some(MacPlayer::Music);
                return Ok(Some(np));
            }
            self.active = None;
            spotify.and(music)
        }

        fn command_target(&self) -> Option<MacPlayer> {
            if let Some(active) = self.active {
                if Self::is_running(active.process_name()) {
                    return Some(active);
                }
            }
            [MacPlayer::Spotify, MacPlayer::Music]
                .into_iter()
                .find(|p| Self::is_running(p.process_name()))
        }

        pub fn command(&mut self, cmd: MediaCommand) -> Result<()> {
            let Some(player) = self.command_target() else {
                return Ok(());
            };
            let verb = match cmd {
                MediaCommand::PlayPause => "playpause",
                MediaCommand::Next => "next track",
                MediaCommand::Previous => "previous track",
            };
            Command::new("osascript")
                .args([
                    "-e",
                    &format!("tell application \"{}\" to {verb}", player.process_name()),
                ])
                .output()?;
            Ok(())
        }
    }

    pub(super) fn parse(player: &str, out: &str) -> Option<(NowPlaying, Option<String>)> {
        let mut lines = out.trim_end_matches('\n').split('\n');
        let state = lines.next()?.trim();
        if state.is_empty() || state.eq_ignore_ascii_case("stopped") {
            return None;
        }
        let title = lines.next().unwrap_or_default().to_string();
        let artist = lines.next().unwrap_or_default().to_string();
        let art = lines
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        Some((
            NowPlaying {
                player: player.into(),
                title,
                artist,
                playing: state == "playing",
                art: None,
            },
            art,
        ))
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use super::{MediaCommand, NowPlaying};
    use anyhow::{Context as _, Result};
    use std::collections::HashMap;
    use zbus::{
        blocking::{fdo::DBusProxy, Connection, Proxy},
        zvariant::OwnedValue,
    };

    const PREFIX: &str = "org.mpris.MediaPlayer2.";

    #[derive(Default)]
    pub struct Backend {
        conn: Option<Connection>,
    }

    impl Backend {
        fn conn(&mut self) -> Result<&Connection> {
            if self.conn.is_none() {
                self.conn = Some(Connection::session().context("connecting to the session bus")?);
            }
            Ok(self.conn.as_ref().unwrap())
        }

        fn player(&mut self) -> Result<Option<Proxy<'static>>> {
            let conn = self.conn()?.clone();
            let names = DBusProxy::new(&conn)?.list_names()?;
            let mut players: Vec<String> = names
                .into_iter()
                .map(|n| n.to_string())
                .filter(|n| n.starts_with(PREFIX))
                .collect();
            players.sort_by_key(|n| !n.to_lowercase().contains("spotify"));
            let Some(name) = players.into_iter().next() else {
                return Ok(None);
            };
            let proxy = Proxy::new(
                &conn,
                name,
                "/org/mpris/MediaPlayer2",
                "org.mpris.MediaPlayer2.Player",
            )?;
            Ok(Some(proxy))
        }

        pub fn poll(&mut self) -> Result<Option<(NowPlaying, Option<String>)>> {
            let result = self.poll_inner();
            if result.is_err() {
                // The bus may have gone away (session restart); reconnect next time.
                self.conn = None;
            }
            result
        }

        fn poll_inner(&mut self) -> Result<Option<(NowPlaying, Option<String>)>> {
            let Some(proxy) = self.player()? else {
                return Ok(None);
            };
            let status: String = proxy.get_property("PlaybackStatus")?;
            if status == "Stopped" {
                return Ok(None);
            }
            let meta: HashMap<String, OwnedValue> = proxy.get_property("Metadata")?;
            let text = |key: &str| -> Option<String> {
                meta.get(key)
                    .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            };
            let artist = meta
                .get("xesam:artist")
                .and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok())
                .map(|a| a.join(", "))
                .unwrap_or_default();
            let player = proxy
                .destination()
                .as_str()
                .trim_start_matches(PREFIX)
                .split('.')
                .next()
                .unwrap_or("Player");
            let mut player_name = player.to_string();
            if let Some(first) = player_name.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            Ok(Some((
                NowPlaying {
                    player: player_name,
                    title: text("xesam:title").unwrap_or_default(),
                    artist,
                    playing: status == "Playing",
                    art: None,
                },
                text("mpris:artUrl"),
            )))
        }

        pub fn command(&mut self, cmd: MediaCommand) -> Result<()> {
            let Some(proxy) = self.player()? else {
                return Ok(());
            };
            let method = match cmd {
                MediaCommand::PlayPause => "PlayPause",
                MediaCommand::Next => "Next",
                MediaCommand::Previous => "Previous",
            };
            proxy.call_method(method, &())?;
            Ok(())
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod backend {
    use super::{MediaCommand, NowPlaying};

    #[derive(Default)]
    pub struct Backend;

    impl Backend {
        pub fn poll(&mut self) -> anyhow::Result<Option<(NowPlaying, Option<String>)>> {
            Ok(None)
        }
        pub fn command(&mut self, _: MediaCommand) -> anyhow::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_art_cache_keeps_newest_files() {
        let dir = std::env::temp_dir().join(format!("enki-art-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        for i in 0..5u64 {
            let p = dir.join(format!("art-{i}.jpg"));
            std::fs::write(&p, b"img").unwrap();
            let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
            f.set_modified(now - Duration::from_secs((5 - i) * 60))
                .unwrap();
        }

        prune_art_cache(&dir, 3);

        assert!(!dir.join("art-0.jpg").exists());
        assert!(!dir.join("art-1.jpg").exists());
        assert!(dir.join("art-2.jpg").exists());
        assert!(dir.join("art-3.jpg").exists());
        assert!(dir.join("art-4.jpg").exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parses_spotify_and_apple_music_osascript_output() {
        let (np, art) = backend::parse(
            "Spotify",
            "playing\nBohemian Rhapsody\nQueen\nhttps://i.scdn.co/image/abc\n",
        )
        .unwrap();
        assert_eq!(np.player, "Spotify");
        assert!(np.playing);
        assert_eq!(np.title, "Bohemian Rhapsody");
        assert_eq!(np.artist, "Queen");
        assert_eq!(art.as_deref(), Some("https://i.scdn.co/image/abc"));

        assert!(backend::parse("Spotify", "\n").is_none());
        assert!(backend::parse("Spotify", "stopped\n\n\n").is_none());

        let (paused, art) = backend::parse("Spotify", "paused\nSong\nArtist\n\n").unwrap();
        assert!(!paused.playing);
        assert!(art.is_none());

        // Apple Music returns 3 lines (no artwork URL).
        let (music, music_art) =
            backend::parse("Apple Music", "playing\nClair de Lune\nClaude Debussy\n").unwrap();
        assert_eq!(music.player, "Apple Music");
        assert!(music.playing);
        assert_eq!(music.title, "Clair de Lune");
        assert_eq!(music.artist, "Claude Debussy");
        assert!(music_art.is_none());

        let (music_paused, _) =
            backend::parse("Apple Music", "paused\nGymnopédie No. 1\nErik Satie\n").unwrap();
        assert!(!music_paused.playing);
        assert!(backend::parse("Apple Music", "").is_none());
        assert!(backend::parse("Apple Music", "stopped\n").is_none());
    }
}
