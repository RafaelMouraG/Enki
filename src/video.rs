//! Streaming video decoder backed by the system `ffmpeg` binary.
//!
//! Used on Linux (and as a fallback whenever native AVFoundation playback
//! isn't used) to decode looping background videos into GPUI `RenderImage`
//! frames with optional real-time Gaussian blur, keeping memory strictly O(1).

use std::{
    io::Read as _,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

use gpui::RenderImage;
use image::{Frame, RgbaImage};

use crate::wallpaper::find_ffmpeg;

const TARGET_FPS: u32 = 24;
const MAX_VIDEO_EDGE: u32 = 1280;
const MAX_BLURRED_VIDEO_EDGE: u32 = 960;

pub struct VideoPlayer {
    stop: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<Arc<RenderImage>>>>,
    child: Arc<Mutex<Option<Child>>>,
    thread: Option<JoinHandle<()>>,
}

impl VideoPlayer {
    /// Spawns a background `ffmpeg` process that loops `path` infinitely,
    /// scaling and optionally blurring frames into BGRA buffers for GPUI.
    pub fn spawn(path: &Path, src_w: u32, src_h: u32, blur_sigma: f32) -> Option<Self> {
        let ffmpeg = find_ffmpeg()?;
        let (w, h) = scaled_video_dimensions(src_w, src_h, blur_sigma > 0.0);
        let vf = build_filter_graph(w, h, blur_sigma, TARGET_FPS);

        let mut child = Command::new(ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-stream_loop",
                "-1",
                "-re",
                "-i",
            ])
            .arg(path)
            .args([
                "-an",
                "-sn",
                "-dn",
                "-vf",
                &vf,
                "-f",
                "rawvideo",
                "-pix_fmt",
                "bgra",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let mut stdout = child.stdout.take()?;
        let stop = Arc::new(AtomicBool::new(false));
        let latest: Arc<Mutex<Option<Arc<RenderImage>>>> = Arc::new(Mutex::new(None));
        let child_slot = Arc::new(Mutex::new(Some(child)));

        let stop_bg = stop.clone();
        let latest_bg = latest.clone();
        let frame_bytes = (w as usize) * (h as usize) * 4;

        let thread = thread::spawn(move || {
            while !stop_bg.load(Ordering::Relaxed) {
                let mut buf = vec![0u8; frame_bytes];
                if stdout.read_exact(&mut buf).is_err() {
                    break;
                }
                if stop_bg.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(img) = RgbaImage::from_raw(w, h, buf) {
                    let render = Arc::new(RenderImage::new(vec![Frame::new(img)]));
                    if let Ok(mut guard) = latest_bg.lock() {
                        *guard = Some(render);
                    }
                }
            }
        });

        Some(Self {
            stop,
            latest,
            child: child_slot,
            thread: Some(thread),
        })
    }

    /// Takes the most recently decoded frame, if a new one has arrived.
    pub fn take_frame(&self) -> Option<Arc<RenderImage>> {
        self.latest.lock().ok()?.take()
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Fits video dimensions into the software-decoding budget, rounding to even numbers.
pub fn scaled_video_dimensions(src_w: u32, src_h: u32, blurred: bool) -> (u32, u32) {
    let max_edge = if blurred {
        MAX_BLURRED_VIDEO_EDGE
    } else {
        MAX_VIDEO_EDGE
    };
    let w = src_w.max(2);
    let h = src_h.max(2);
    let longest = w.max(h);
    let (sw, sh) = if longest > max_edge {
        let scale = max_edge as f64 / longest as f64;
        (
            ((w as f64 * scale).round() as u32).max(2),
            ((h as f64 * scale).round() as u32).max(2),
        )
    } else {
        (w, h)
    };
    // Ensure even dimensions (required by many ffmpeg filters/pixel formats).
    ((sw & !1).max(2), (sh & !1).max(2))
}

fn build_filter_graph(w: u32, h: u32, blur_sigma: f32, fps: u32) -> String {
    if blur_sigma > 0.0 {
        // Scale sigma proportionally to frame size so the visual blur matches static images.
        let scaled = (blur_sigma * (w.max(h) as f32 / 1080.0)).clamp(1.0, 40.0);
        format!("scale={w}:{h}:flags=fast_bilinear,fps={fps},gblur=sigma={scaled:.2},format=bgra")
    } else {
        format!("scale={w}:{h}:flags=fast_bilinear,fps={fps},format=bgra")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_video_dimensions_preserves_aspect_and_even_bounds() {
        assert_eq!(scaled_video_dimensions(1920, 1080, false), (1280, 720));
        assert_eq!(scaled_video_dimensions(1920, 1080, true), (960, 540));
        assert_eq!(scaled_video_dimensions(641, 481, false), (640, 480));
        assert_eq!(scaled_video_dimensions(0, 0, false), (2, 2));
    }

    #[test]
    fn filter_graph_adds_blur_only_when_sigma_positive() {
        let plain = build_filter_graph(1280, 720, 0.0, 24);
        assert!(!plain.contains("gblur"));
        assert!(plain.ends_with("format=bgra"));

        let blurred = build_filter_graph(960, 540, 8.0, 24);
        assert!(blurred.contains("gblur=sigma="));
        assert!(blurred.ends_with("format=bgra"));
    }

    #[test]
    fn video_player_streams_frames_when_ffmpeg_is_installed() {
        let Some(ffmpeg) = find_ffmpeg() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("enki-video-stream-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mp4 = dir.join("sample.mp4");
        let ok = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x2266dd:s=160x90:d=1",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&mp4)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg failed to synthesize sample mp4");

        let player = VideoPlayer::spawn(&mp4, 160, 90, 4.0).expect("spawn video player");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut got_frame = None;
        while std::time::Instant::now() < deadline {
            if let Some(frame) = player.take_frame() {
                got_frame = Some(frame);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let frame = got_frame.expect("expected at least one decoded video frame");
        assert_eq!(frame.frame_count(), 1);
        assert_eq!(frame.size(0).width.0, 160);
        assert_eq!(frame.size(0).height.0, 90);
        drop(player);

        let _ = std::fs::remove_dir_all(dir);
    }
}
