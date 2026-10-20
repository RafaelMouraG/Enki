//! "Now playing" for the sidebar card.
//!
//! Talks to the local player instead of the Spotify Web API, so there is no
//! login and it works on free accounts: AppleScript on macOS (Spotify only),
//! MPRIS over D-Bus on Linux (Spotify preferred, any player otherwise).

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
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
        return Some(path);
    }
    std::fs::create_dir_all(&dir).ok()?;
    let mut body = ureq::get(url).call().ok()?.into_body();
    let bytes = body
        .with_config()
        .limit(5 * 1024 * 1024)
        .read_to_vec()
        .ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

#[cfg(target_os = "macos")]
mod backend {
    use super::{MediaCommand, NowPlaying};
    use anyhow::{bail, Result};
    use std::process::Command;

    #[derive(Default)]
    pub struct Backend;

    const QUERY: &str = r#"tell application "Spotify"
    if player state is stopped then return ""
    set t to current track
    return (player state as string) & linefeed & (name of t) & linefeed & (artist of t) & linefeed & (artwork url of t)
end tell"#;

    impl Backend {
        /// Never launches Spotify: AppleScript is only used while it runs,
        /// which also avoids the "Where is Spotify?" dialog when it isn't installed.
        fn running() -> bool {
            Command::new("pgrep")
                .args(["-x", "Spotify"])
                .output()
                .is_ok_and(|o| o.status.success())
        }

        pub fn poll(&mut self) -> Result<Option<(NowPlaying, Option<String>)>> {
            if !Self::running() {
                return Ok(None);
            }
            let out = Command::new("osascript").args(["-e", QUERY]).output()?;
            if !out.status.success() {
                bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
            }
            Ok(parse(&String::from_utf8_lossy(&out.stdout)))
        }

        pub fn command(&mut self, cmd: MediaCommand) -> Result<()> {
            if !Self::running() {
                return Ok(());
            }
            let verb = match cmd {
                MediaCommand::PlayPause => "playpause",
                MediaCommand::Next => "next track",
                MediaCommand::Previous => "previous track",
            };
            Command::new("osascript")
                .args(["-e", &format!("tell application \"Spotify\" to {verb}")])
                .output()?;
            Ok(())
        }
    }

    pub(super) fn parse(out: &str) -> Option<(NowPlaying, Option<String>)> {
        let mut lines = out.trim_end_matches('\n').split('\n');
        let state = lines.next()?.trim();
        if state.is_empty() {
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
                player: "Spotify".into(),
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn parses_osascript_output() {
        let (np, art) = super::backend::parse(
            "playing\nBohemian Rhapsody\nQueen\nhttps://i.scdn.co/image/abc\n",
        )
        .unwrap();
        assert!(np.playing);
        assert_eq!(np.title, "Bohemian Rhapsody");
        assert_eq!(np.artist, "Queen");
        assert_eq!(art.as_deref(), Some("https://i.scdn.co/image/abc"));
        assert!(super::backend::parse("\n").is_none());
        let (paused, art) = super::backend::parse("paused\nSong\nArtist\n\n").unwrap();
        assert!(!paused.playing);
        assert!(art.is_none());
    }
}
