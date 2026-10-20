//! Desktop notifications and the end-of-phase sound.
//!
//! Both go through the system's own tools so Enki doesn't need an audio stack:
//! `osascript`/`afplay` on macOS, D-Bus notifications and `paplay`/`pw-play` on Linux.

use std::process::{Command, Stdio};

use crate::settings::Sound;

pub fn notify(title: &str, body: &str) {
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        if let Err(err) = send(&title, &body) {
            eprintln!("enki: notification failed: {err}");
        }
    });
}

#[cfg(target_os = "macos")]
fn send(title: &str, body: &str) -> anyhow::Result<()> {
    let script = format!(
        "display notification {} with title {}",
        applescript_string(body),
        applescript_string(title)
    );
    Command::new("osascript")
        .args(["-e", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn send(title: &str, body: &str) -> anyhow::Result<()> {
    notify_rust::Notification::new()
        .appname("Enki")
        .summary(title)
        .body(body)
        .icon("alarm-symbolic")
        .show()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn send(_title: &str, _body: &str) -> anyhow::Result<()> {
    Ok(())
}

pub fn play(sound: Sound) {
    let Some(file) = sound_file(sound) else {
        return;
    };
    std::thread::spawn(move || {
        for player in PLAYERS {
            let ok = Command::new(player)
                .arg(file)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                break;
            }
        }
    });
}

#[cfg(target_os = "macos")]
const PLAYERS: &[&str] = &["afplay"];
#[cfg(not(target_os = "macos"))]
const PLAYERS: &[&str] = &["paplay", "pw-play", "aplay"];

#[cfg(target_os = "macos")]
fn sound_file(sound: Sound) -> Option<&'static str> {
    match sound {
        Sound::None => None,
        Sound::Chime => Some("/System/Library/Sounds/Glass.aiff"),
        Sound::Bell => Some("/System/Library/Sounds/Hero.aiff"),
        Sound::Soft => Some("/System/Library/Sounds/Tink.aiff"),
    }
}

#[cfg(not(target_os = "macos"))]
fn sound_file(sound: Sound) -> Option<&'static str> {
    match sound {
        Sound::None => None,
        Sound::Chime => Some("/usr/share/sounds/freedesktop/stereo/complete.oga"),
        Sound::Bell => Some("/usr/share/sounds/freedesktop/stereo/bell.oga"),
        Sound::Soft => Some("/usr/share/sounds/freedesktop/stereo/message.oga"),
    }
}

/// Quotes a string as an AppleScript literal.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    #[test]
    fn applescript_quoting() {
        assert_eq!(super::applescript_string(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }
}
