//! Background media processing: static images, animated images (GIF, WebP,
//! APNG), videos (MP4, MOV, WebM, ...), blur (GPUI has no image filters) and
//! the dominant color used for "match wallpaper colors".

use std::{
    collections::hash_map::DefaultHasher,
    fs::File,
    hash::{Hash, Hasher},
    io::BufReader,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{bail, Context as _, Result};
use gpui::{RenderImage, Rgba};
use image::{
    codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder},
    imageops::FilterType,
    AnimationDecoder, Delay, DynamicImage, Frame, GenericImageView, ImageFormat, ImageReader,
    RgbaImage,
};

use crate::settings::BackgroundEffect;

#[derive(Clone, Debug)]
pub struct Wallpaper {
    /// Static preview (blurred & downscaled `.jpg` in cache), used for static
    /// display and `ENKI_DEBUG_SYNC`.
    pub preview: PathBuf,
    /// Square center-cropped thumbnail (128×128 JPEG in cache) for the Settings
    /// preview so GPUI's `ObjectFit::Cover` never overflows or clips rounded corners.
    pub thumb: PathBuf,
    pub tint: Rgba,
    pub content: WallpaperContent,
}

#[derive(Clone, Debug)]
pub enum WallpaperContent {
    Static,
    Animated(Arc<RenderImage>),
    Video {
        /// Playable video file (original source or cached transcoded `.mp4`).
        path: PathBuf,
        /// Blur sigma applied during live playback (`0.0` for none).
        blur_sigma: f32,
        /// Natural video frame dimensions (used by software/ffmpeg playback).
        width: u32,
        height: u32,
        /// True when macOS AVFoundation can play `path` directly.
        native_mac: bool,
    },
}

impl Wallpaper {
    /// Whether this wallpaper is played by macOS native `AVPlayerLayer`.
    pub fn is_native_video(&self) -> bool {
        cfg!(target_os = "macos")
            && matches!(
                self.content,
                WallpaperContent::Video {
                    native_mac: true,
                    ..
                }
            )
    }
}

const MAX_EDGE: u32 = 2400;
const THUMB_SIZE: u32 = 128;
const MAX_ANIMATED_EDGE: u32 = 1280;
const MAX_ANIMATED_BLUR_EDGE: u32 = 960;
const MAX_ANIMATED_FRAMES: usize = 180;
/// Cap total uncompressed BGRA memory for animated wallpapers (~96 MiB).
const MAX_ANIMATED_BYTES: u64 = 96 * 1024 * 1024;
const MIN_ANIMATED_EDGE: u32 = 240;
/// Bump when processing changes so cached results are recomputed.
const CACHE_VERSION: u32 = 3;
const MAX_CACHED_WALLPAPERS: usize = 20;
const MAX_CACHED_VIDEOS: usize = 5;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "webm", "mkv", "ogv", "avi", "wmv", "flv", "mpeg", "mpg",
];
const MAC_NATIVE_VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov"];

pub fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| VIDEO_EXTENSIONS.iter().any(|v| ext.eq_ignore_ascii_case(v)))
}

fn is_mac_native_video_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            MAC_NATIVE_VIDEO_EXTENSIONS
                .iter()
                .any(|v| ext.eq_ignore_ascii_case(v))
        })
}

/// Locates `ffmpeg` in `PATH` or common Homebrew/MacPorts locations (macOS app
/// bundles launched from Finder/Dock don't inherit the shell's Homebrew `PATH`).
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("ffmpeg");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        for candidate in [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "/opt/local/bin/ffmpeg",
        ] {
            let p = PathBuf::from(candidate);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn touch_modified(path: &Path) {
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(SystemTime::now()));
}

fn prune_wallpaper_cache(dir: &Path, max_wallpapers: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut wallpapers: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut videos: Vec<(SystemTime, PathBuf)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let is_thumb = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".thumb.jpg"));
        match ext {
            Some("jpg") if !is_thumb => wallpapers.push((mtime, path)),
            Some("mp4") => videos.push((mtime, path)),
            _ => {}
        }
    }

    if wallpapers.len() > max_wallpapers {
        wallpapers.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, jpg) in wallpapers.into_iter().skip(max_wallpapers) {
            let tint = jpg.with_extension("tint");
            let thumb = jpg.with_extension("thumb.jpg");
            let _ = std::fs::remove_file(&jpg);
            let _ = std::fs::remove_file(&tint);
            let _ = std::fs::remove_file(&thumb);
        }
    }
    if videos.len() > MAX_CACHED_VIDEOS {
        videos.sort_by_key(|a| std::cmp::Reverse(a.0));
        for (_, mp4) in videos.into_iter().skip(MAX_CACHED_VIDEOS) {
            let _ = std::fs::remove_file(&mp4);
        }
    }
}

fn source_hash(source: &Path, meta: &std::fs::Metadata) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    CACHE_VERSION.hash(&mut hasher);
    hasher.finish()
}

fn cache_key(source: &Path, meta: &std::fs::Metadata, effect: BackgroundEffect) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    effect.hash(&mut hasher);
    CACHE_VERSION.hash(&mut hasher);
    hasher.finish()
}

pub fn process(source: &Path, effect: BackgroundEffect) -> Result<Wallpaper> {
    let meta = std::fs::metadata(source).with_context(|| format!("reading {source:?}"))?;
    let key = cache_key(source, &meta, effect);
    let src_key = source_hash(source, &meta);

    let cache = crate::paths::cache_dir().join("wallpapers");
    std::fs::create_dir_all(&cache)?;
    let preview = cache.join(format!("{key:016x}.jpg"));
    let thumb = cache.join(format!("{key:016x}.thumb.jpg"));
    let tint_file = cache.join(format!("{key:016x}.tint"));

    if is_video_path(source) {
        return process_video(source, effect, src_key, &cache, preview, thumb, tint_file);
    }

    // Try animated image decoding (GIF, animated WebP, APNG) before falling back to static cache.
    if let Some(frames) = try_decode_animation(source)? {
        return process_animation(frames, effect, &cache, preview, thumb, tint_file);
    }

    if let (true, true, Some(tint)) = (preview.exists(), thumb.exists(), read_tint(&tint_file)) {
        touch_modified(&preview);
        return Ok(Wallpaper {
            preview,
            thumb,
            tint,
            content: WallpaperContent::Static,
        });
    }

    let img = decode_static_image(source, &cache)?;
    let tint = dominant_color(&img);
    let out = apply_static_effect(img, effect);

    save_preview_and_thumb(&out, &preview, &thumb)?;
    let _ = std::fs::write(&tint_file, format!("{} {} {}", tint.r, tint.g, tint.b));
    prune_wallpaper_cache(&cache, MAX_CACHED_WALLPAPERS);

    Ok(Wallpaper {
        preview,
        thumb,
        tint,
        content: WallpaperContent::Static,
    })
}

fn save_preview_and_thumb(img: &DynamicImage, preview: &Path, thumb: &Path) -> Result<()> {
    img.to_rgb8()
        .save_with_format(preview, ImageFormat::Jpeg)
        .context("saving processed wallpaper")?;
    img.resize_to_fill(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle)
        .to_rgb8()
        .save_with_format(thumb, ImageFormat::Jpeg)
        .context("saving wallpaper thumbnail")?;
    Ok(())
}

fn apply_static_effect(img: DynamicImage, effect: BackgroundEffect) -> DynamicImage {
    let (w, h) = img.dimensions();
    let mut out = if w.max(h) > MAX_EDGE {
        img.resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle)
    } else {
        img
    };
    let sigma = effect.sigma();
    if sigma > 0.0 {
        // Blurring at a quarter size is ~16x faster and looks the same once upscaled.
        let (w, h) = out.dimensions();
        let small = out.resize((w / 4).max(1), (h / 4).max(1), FilterType::Triangle);
        let blurred = small.fast_blur(sigma / 4.0);
        out = blurred.resize_exact((w / 2).max(1), (h / 2).max(1), FilterType::Triangle);
    }
    out
}

fn decode_static_image(source: &Path, cache: &Path) -> Result<DynamicImage> {
    let direct = ImageReader::open(source)
        .map_err(image::ImageError::IoError)
        .and_then(|r| r.with_guessed_format().map_err(image::ImageError::IoError))
        .and_then(ImageReader::decode);

    match direct {
        Ok(img) => Ok(img),
        Err(err) => decode_with_system(source, cache)
            .or_else(|| decode_frame_with_ffmpeg(source))
            .ok_or(err)
            .with_context(|| format!("decoding {source:?}")),
    }
}

/// Checks if `source` is a multi-frame GIF, animated WebP, or APNG and returns its frames.
fn try_decode_animation(source: &Path) -> Result<Option<Vec<Frame>>> {
    let format = ImageReader::open(source)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .and_then(|r| r.format());

    match format {
        Some(ImageFormat::Gif) => {
            let file = File::open(source).with_context(|| format!("opening {source:?}"))?;
            let decoder = GifDecoder::new(BufReader::new(file))
                .with_context(|| format!("decoding gif {source:?}"))?;
            let frames: Vec<Frame> = decoder
                .into_frames()
                .collect_frames()
                .with_context(|| format!("reading gif frames {source:?}"))?;
            Ok((frames.len() > 1).then_some(frames))
        }
        Some(ImageFormat::WebP) => {
            let file = File::open(source).with_context(|| format!("opening {source:?}"))?;
            let mut decoder = WebPDecoder::new(BufReader::new(file))
                .with_context(|| format!("decoding webp {source:?}"))?;
            if !decoder.has_animation() {
                return Ok(None);
            }
            let _ = decoder.set_background_color(image::Rgba([0, 0, 0, 0]));
            let frames: Vec<Frame> = decoder
                .into_frames()
                .collect_frames()
                .with_context(|| format!("reading webp frames {source:?}"))?;
            Ok((frames.len() > 1).then_some(frames))
        }
        Some(ImageFormat::Png) => {
            let file = File::open(source).with_context(|| format!("opening {source:?}"))?;
            let decoder = PngDecoder::new(BufReader::new(file))
                .with_context(|| format!("decoding png {source:?}"))?;
            if !decoder.is_apng().unwrap_or(false) {
                return Ok(None);
            }
            let frames: Vec<Frame> = decoder
                .apng()
                .with_context(|| format!("reading apng {source:?}"))?
                .into_frames()
                .collect_frames()
                .with_context(|| format!("reading apng frames {source:?}"))?;
            Ok((frames.len() > 1).then_some(frames))
        }
        _ => Ok(None),
    }
}

fn process_animation(
    frames: Vec<Frame>,
    effect: BackgroundEffect,
    cache: &Path,
    preview: PathBuf,
    thumb: PathBuf,
    tint_file: PathBuf,
) -> Result<Wallpaper> {
    let first_dyn = DynamicImage::ImageRgba8(frames[0].buffer().clone());
    let tint = read_tint(&tint_file).unwrap_or_else(|| dominant_color(&first_dyn));

    let frames = subsample_frames(frames, MAX_ANIMATED_FRAMES);
    let (src_w, src_h) = frames[0].buffer().dimensions();
    let sigma = effect.sigma();
    let (target_w, target_h) = animated_target_dimensions(src_w, src_h, frames.len(), sigma > 0.0);

    let mut processed: Vec<Frame> = Vec::with_capacity(frames.len());
    let mut preview_rgba: Option<RgbaImage> = None;

    for (idx, frame) in frames.into_iter().enumerate() {
        let delay = normalize_delay(frame.delay());
        let rgba = frame.into_buffer();
        let dyn_img = DynamicImage::ImageRgba8(rgba);
        let out_rgba = process_animated_frame(dyn_img, target_w, target_h, sigma);
        if idx == 0 {
            preview_rgba = Some(out_rgba.clone());
        }
        let mut bgra = out_rgba;
        for px in bgra.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        processed.push(Frame::from_parts(bgra, 0, 0, delay));
    }

    if !preview.exists() || !thumb.exists() || !tint_file.exists() {
        if let Some(first) = preview_rgba {
            let first_img = DynamicImage::ImageRgba8(first);
            save_preview_and_thumb(&first_img, &preview, &thumb)?;
        }
        let _ = std::fs::write(&tint_file, format!("{} {} {}", tint.r, tint.g, tint.b));
        prune_wallpaper_cache(cache, MAX_CACHED_WALLPAPERS);
    } else {
        touch_modified(&preview);
    }

    let render_image = Arc::new(RenderImage::new(processed));
    Ok(Wallpaper {
        preview,
        thumb,
        tint,
        content: WallpaperContent::Animated(render_image),
    })
}

fn process_animated_frame(
    img: DynamicImage,
    target_w: u32,
    target_h: u32,
    sigma: f32,
) -> RgbaImage {
    let resized = if img.width() != target_w || img.height() != target_h {
        img.resize_exact(target_w, target_h, FilterType::Triangle)
    } else {
        img
    };
    if sigma > 0.0 {
        let (w, h) = resized.dimensions();
        let small = resized.resize_exact((w / 2).max(1), (h / 2).max(1), FilterType::Triangle);
        let scaled_sigma = (sigma * (w.max(h) as f32 / 1400.0)).max(1.0);
        small
            .fast_blur(scaled_sigma / 2.0)
            .resize_exact(w, h, FilterType::Triangle)
            .into_rgba8()
    } else {
        resized.into_rgba8()
    }
}

/// Normalizes GIF/WebP frame delays: zero delays become 100ms (matching browsers),
/// and sub-20ms delays clamp to 20ms so broken GIFs don't spin at 1000fps.
fn normalize_delay(delay: Delay) -> Delay {
    let ms = Duration::from(delay).as_millis() as u32;
    let clamped = if ms == 0 { 100 } else { ms.max(20) };
    Delay::from_numer_denom_ms(clamped, 1)
}

/// Keeps at most `max_frames`, folding dropped frames' durations into the kept
/// frame before them so overall animation duration stays accurate.
fn subsample_frames(frames: Vec<Frame>, max_frames: usize) -> Vec<Frame> {
    let total = frames.len();
    if total <= max_frames || max_frames == 0 {
        return frames;
    }
    let mut kept: Vec<Frame> = Vec::with_capacity(max_frames);
    let mut next_keep = 0usize;
    let mut acc_ms = 0u32;

    for (i, frame) in frames.into_iter().enumerate() {
        let target_index = (next_keep * total) / max_frames;
        let delay_ms = Duration::from(normalize_delay(frame.delay())).as_millis() as u32;
        if i == target_index && kept.len() < max_frames {
            if let Some(prev) = kept.last_mut() {
                if acc_ms > 0 {
                    let prev_ms = Duration::from(prev.delay()).as_millis() as u32;
                    let merged = Delay::from_numer_denom_ms(prev_ms.saturating_add(acc_ms), 1);
                    let buf = std::mem::replace(prev.buffer_mut(), RgbaImage::new(1, 1));
                    *prev = Frame::from_parts(buf, 0, 0, merged);
                    acc_ms = 0;
                }
            }
            kept.push(Frame::from_parts(
                frame.into_buffer(),
                0,
                0,
                Delay::from_numer_denom_ms(delay_ms, 1),
            ));
            next_keep += 1;
        } else {
            acc_ms = acc_ms.saturating_add(delay_ms);
        }
    }
    if acc_ms > 0 {
        if let Some(last) = kept.last_mut() {
            let last_ms = Duration::from(last.delay()).as_millis() as u32;
            let merged = Delay::from_numer_denom_ms(last_ms.saturating_add(acc_ms), 1);
            let buf = std::mem::replace(last.buffer_mut(), RgbaImage::new(1, 1));
            *last = Frame::from_parts(buf, 0, 0, merged);
        }
    }
    kept
}

/// Computes target dimensions for an animated image that fit both the edge cap
/// and the total uncompressed BGRA byte budget across all frames.
fn animated_target_dimensions(src_w: u32, src_h: u32, frame_count: usize, blurred: bool) -> (u32, u32) {
    let w = src_w.max(1);
    let h = src_h.max(1);
    let edge_cap = if blurred {
        MAX_ANIMATED_BLUR_EDGE
    } else {
        MAX_ANIMATED_EDGE
    };
    let mut scale = if w.max(h) > edge_cap {
        edge_cap as f64 / w.max(h) as f64
    } else {
        1.0
    };

    let bytes_at_scale =
        (w as f64 * scale) * (h as f64 * scale) * 4.0 * (frame_count.max(1) as f64);
    if bytes_at_scale > MAX_ANIMATED_BYTES as f64 {
        let budget_scale = (MAX_ANIMATED_BYTES as f64 / bytes_at_scale).sqrt();
        scale *= budget_scale;
        // Don't crush tiny dimensions below MIN_ANIMATED_EDGE unless the source was already smaller.
        let min_scale = (MIN_ANIMATED_EDGE.min(w.max(h)) as f64) / (w.max(h) as f64);
        scale = scale.max(min_scale);
    }

    (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    )
}

fn process_video(
    source: &Path,
    effect: BackgroundEffect,
    src_key: u64,
    cache: &Path,
    preview: PathBuf,
    thumb: PathBuf,
    tint_file: PathBuf,
) -> Result<Wallpaper> {
    let mut playable = source.to_path_buf();
    let mut native_mac = false;

    if cfg!(target_os = "macos") {
        if is_mac_native_video_ext(source) {
            native_mac = true;
        } else {
            // Non-native containers (WebM, MKV, etc.) on macOS: transcode once to a cached
            // H.264 MP4 so AVFoundation plays it hardware-accelerated under Liquid Glass.
            let cached_mp4 = cache.join(format!("{src_key:016x}.mp4"));
            if cached_mp4.exists() {
                touch_modified(&cached_mp4);
            } else {
                transcode_to_mp4(source, &cached_mp4)?;
                prune_wallpaper_cache(cache, MAX_CACHED_WALLPAPERS);
            }
            playable = cached_mp4;
            native_mac = true;
        }
    } else if find_ffmpeg().is_none() {
        bail!("Para usar vídeos como fundo no Linux, instale o ffmpeg.");
    }

    if let (true, true, Some(tint)) = (preview.exists(), thumb.exists(), read_tint(&tint_file)) {
        touch_modified(&preview);
        let (width, height) = read_cached_dimensions(&tint_file)
            .or_else(|| image::image_dimensions(&preview).ok())
            .unwrap_or((1280, 720));
        return Ok(Wallpaper {
            preview,
            thumb,
            tint,
            content: WallpaperContent::Video {
                path: playable,
                blur_sigma: effect.sigma(),
                width,
                height,
                native_mac,
            },
        });
    }

    let first_frame = extract_video_first_frame(&playable, source)?;
    let (width, height) = first_frame.dimensions();
    let tint = dominant_color(&first_frame);
    let out = apply_static_effect(first_frame, effect);
    save_preview_and_thumb(&out, &preview, &thumb)?;
    let _ = std::fs::write(
        &tint_file,
        format!("{} {} {} {} {}", tint.r, tint.g, tint.b, width, height),
    );
    prune_wallpaper_cache(cache, MAX_CACHED_WALLPAPERS);

    Ok(Wallpaper {
        preview,
        thumb,
        tint,
        content: WallpaperContent::Video {
            path: playable,
            blur_sigma: effect.sigma(),
            width,
            height,
            native_mac,
        },
    })
}

fn extract_video_first_frame(playable: &Path, original: &Path) -> Result<DynamicImage> {
    #[cfg(target_os = "macos")]
    if is_mac_native_video_ext(playable) {
        if let Some(img) = crate::native::extract_video_frame(playable) {
            return Ok(img);
        }
    }

    if let Some(img) = decode_frame_with_ffmpeg(playable) {
        return Ok(img);
    }

    if cfg!(target_os = "macos") {
        bail!(
            "Não foi possível ler o vídeo {:?}. Verifique se o arquivo é um vídeo válido.",
            original.file_name().unwrap_or_default()
        );
    } else {
        bail!("Não foi possível decodificar o vídeo. Verifique se o ffmpeg está instalado.");
    }
}

fn transcode_to_mp4(source: &Path, dest: &Path) -> Result<()> {
    let Some(ffmpeg) = find_ffmpeg() else {
        bail!(
            "Vídeos .mp4 e .mov funcionam nativamente; para este formato, instale o ffmpeg (ex.: brew install ffmpeg)."
        );
    };
    let tmp = dest.with_extension("tmp.mp4");
    let status = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args([
            "-an",
            "-sn",
            "-dn",
            "-vf",
            "scale=min(1920\\,iw):min(1920\\,ih):force_original_aspect_ratio=decrease:force_divisible_by=2",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(&tmp)
        .output()
        .context("running ffmpeg to convert video")?;

    if !status.status.success() {
        let _ = std::fs::remove_file(&tmp);
        let stderr = String::from_utf8_lossy(&status.stderr);
        bail!("Falha ao converter vídeo com ffmpeg: {}", stderr.trim());
    }
    std::fs::rename(&tmp, dest).context("finalizing converted video")?;
    Ok(())
}

/// Extracts frame 0 of a video or uncommon image format (AVIF, HEIC, JXL) via `ffmpeg`.
fn decode_frame_with_ffmpeg(source: &Path) -> Option<DynamicImage> {
    let ffmpeg = find_ffmpeg()?;
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(source)
        .args(["-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "pipe:1"])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    image::load_from_memory_with_format(&output.stdout, ImageFormat::Png).ok()
}

/// macOS wallpapers are HEIC (and AVIF/PSD/JP2), which `sips` decodes natively.
#[cfg(target_os = "macos")]
fn decode_with_system(source: &Path, cache: &Path) -> Option<DynamicImage> {
    let out = cache.join(format!("converted-{}.jpg", uuid::Uuid::new_v4()));
    let ok = Command::new("sips")
        .args(["-s", "format", "jpeg"])
        .arg(source)
        .arg("--out")
        .arg(&out)
        .output()
        .is_ok_and(|o| o.status.success());
    let img = ok.then(|| image::open(&out).ok()).flatten();
    let _ = std::fs::remove_file(&out);
    img
}

#[cfg(not(target_os = "macos"))]
fn decode_with_system(_source: &Path, _cache: &Path) -> Option<DynamicImage> {
    None
}

fn read_tint(path: &Path) -> Option<Rgba> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut parts = text.split_whitespace().map(|p| p.parse::<f32>().ok());
    Some(Rgba {
        r: parts.next()??,
        g: parts.next()??,
        b: parts.next()??,
        a: 1.0,
    })
}

fn read_cached_dimensions(path: &Path) -> Option<(u32, u32)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut parts = text.split_whitespace();
    let _ = (parts.next()?, parts.next()?, parts.next()?);
    let w = parts.next()?.parse::<u32>().ok()?;
    let h = parts.next()?.parse::<u32>().ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

/// The most prominent hue of the image. Pixels are bucketed by hue and
/// weighted towards saturated, mid-brightness ones; the heaviest bucket wins.
/// Averaging across hues instead would turn red + green + blue into mud.
pub fn dominant_color(img: &DynamicImage) -> Rgba {
    const BINS: usize = 18;
    let small = img.resize_exact(64, 64, FilterType::Triangle).to_rgb8();
    let mut bins = [(0f32, 0f32, 0f32, 0f32); BINS];
    let (mut gray, mut gray_weight) = ((0f32, 0f32, 0f32), 0f32);
    for px in small.pixels() {
        let [r, g, b] = px.0.map(|c| c as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
        if saturation < 0.15 || max < 0.12 {
            gray = (gray.0 + r, gray.1 + g, gray.2 + b);
            gray_weight += 1.0;
            continue;
        }
        let hue = hue_of(r, g, b, max, min);
        let bin = ((hue * BINS as f32) as usize).min(BINS - 1);
        let weight = saturation * (1.0 - (max - 0.65).abs()).max(0.05);
        let entry = &mut bins[bin];
        *entry = (
            entry.0 + r * weight,
            entry.1 + g * weight,
            entry.2 + b * weight,
            entry.3 + weight,
        );
    }
    // Let neighbouring bins vote together so a hue split across a bin edge still wins.
    let best = (0..BINS)
        .max_by(|&a, &b| {
            let score = |i: usize| {
                bins[(i + BINS - 1) % BINS].3 * 0.5 + bins[i].3 + bins[(i + 1) % BINS].3 * 0.5
            };
            score(a).total_cmp(&score(b))
        })
        .unwrap_or(0);
    let (r, g, b, w) = bins[best];
    if w <= 0.0 {
        let n = gray_weight.max(1.0);
        return Rgba {
            r: gray.0 / n,
            g: gray.1 / n,
            b: gray.2 / n,
            a: 1.0,
        };
    }
    Rgba {
        r: r / w,
        g: g / w,
        b: b / w,
        a: 1.0,
    }
}

fn hue_of(r: f32, g: f32, b: f32, max: f32, min: f32) -> f32 {
    let d = max - min;
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h / 6.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::gif::GifEncoder, Rgb, RgbImage};

    #[test]
    fn dominant_prefers_saturated_pixels() {
        // Mostly gray with a blue band.
        let img = RgbImage::from_fn(100, 100, |_, y| {
            if y < 30 {
                Rgb([40, 110, 220])
            } else {
                Rgb([128, 128, 128])
            }
        });
        let c = dominant_color(&DynamicImage::ImageRgb8(img));
        assert!(c.b > c.r + 0.15, "expected a blue tint, got {c:?}");
    }

    #[test]
    fn dominant_does_not_average_hues() {
        // Equal-ish thirds of red, green and blue, with blue the largest.
        let img = RgbImage::from_fn(90, 90, |_, y| match y {
            0..=24 => Rgb([210, 50, 50]),
            25..=49 => Rgb([50, 190, 60]),
            _ => Rgb([60, 90, 220]),
        });
        let c = dominant_color(&DynamicImage::ImageRgb8(img));
        assert!(
            c.b > c.r && c.b > c.g,
            "expected the blue band to win, got {c:?}"
        );
    }

    #[test]
    fn gray_images_fall_back_to_gray() {
        let img = RgbImage::from_pixel(10, 10, Rgb([120, 120, 120]));
        let c = dominant_color(&DynamicImage::ImageRgb8(img));
        assert!((c.r - c.b).abs() < 0.01);
    }

    #[test]
    fn prune_wallpaper_cache_keeps_newest_pairs() {
        let dir =
            std::env::temp_dir().join(format!("enki-wallpaper-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        for i in 0..4u64 {
            let jpg = dir.join(format!("wall-{i}.jpg"));
            let tint = dir.join(format!("wall-{i}.tint"));
            let thumb = dir.join(format!("wall-{i}.thumb.jpg"));
            std::fs::write(&jpg, b"jpg").unwrap();
            std::fs::write(&tint, b"0.1 0.2 0.3").unwrap();
            std::fs::write(&thumb, b"thumb").unwrap();
            let f = std::fs::OpenOptions::new().write(true).open(&jpg).unwrap();
            f.set_modified(now - Duration::from_secs((4 - i) * 60))
                .unwrap();
        }

        prune_wallpaper_cache(&dir, 2);

        for i in 0..2 {
            assert!(!dir.join(format!("wall-{i}.jpg")).exists());
            assert!(!dir.join(format!("wall-{i}.tint")).exists());
            assert!(!dir.join(format!("wall-{i}.thumb.jpg")).exists());
        }
        for i in 2..4 {
            assert!(dir.join(format!("wall-{i}.jpg")).exists());
            assert!(dir.join(format!("wall-{i}.tint")).exists());
            assert!(dir.join(format!("wall-{i}.thumb.jpg")).exists());
        }

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn detects_video_extensions_case_insensitively() {
        assert!(is_video_path(Path::new("loop.mp4")));
        assert!(is_video_path(Path::new("clip.MOV")));
        assert!(is_video_path(Path::new("bg.WebM")));
        assert!(!is_video_path(Path::new("anim.gif")));
        assert!(!is_video_path(Path::new("photo.png")));
    }

    #[test]
    fn animated_gif_decodes_frames_and_applies_blur() {
        let dir = std::env::temp_dir().join(format!("enki-gif-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let gif_path = dir.join("anim.gif");
        {
            let file = File::create(&gif_path).unwrap();
            let mut encoder = GifEncoder::new(file);
            for color in [image::Rgba([40, 100, 220, 255]), image::Rgba([220, 80, 40, 255])] {
                let img = RgbaImage::from_pixel(32, 24, color);
                encoder
                    .encode_frame(Frame::from_parts(
                        img,
                        0,
                        0,
                        Delay::from_numer_denom_ms(80, 1),
                    ))
                    .unwrap();
            }
        }

        let frames = try_decode_animation(&gif_path).unwrap().expect("animated gif");
        assert_eq!(frames.len(), 2);

        let preview = dir.join("preview.jpg");
        let thumb = dir.join("preview.thumb.jpg");
        let tint_file = dir.join("preview.tint");
        let wp = process_animation(
            frames,
            BackgroundEffect::Blur,
            &dir,
            preview.clone(),
            thumb.clone(),
            tint_file,
        )
        .unwrap();
        assert!(preview.exists());
        assert_eq!(image::image_dimensions(&thumb).unwrap(), (THUMB_SIZE, THUMB_SIZE));
        assert!(wp.tint.b > wp.tint.r);
        let WallpaperContent::Animated(anim) = &wp.content else {
            panic!("expected animated wallpaper");
        };
        assert_eq!(anim.frame_count(), 2);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn decodes_bmp_and_tiff_static_images() {
        let dir = std::env::temp_dir().join(format!("enki-formats-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = DynamicImage::ImageRgb8(RgbImage::from_pixel(32, 24, Rgb([50, 180, 90])));

        for (name, fmt) in [("sample.bmp", ImageFormat::Bmp), ("sample.tiff", ImageFormat::Tiff)] {
            let path = dir.join(name);
            src.save_with_format(&path, fmt).unwrap();
            let decoded = decode_static_image(&path, &dir).unwrap();
            assert_eq!(decoded.dimensions(), (32, 24));
            let tint = dominant_color(&decoded);
            assert!(tint.g > tint.r && tint.g > tint.b);
        }

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn video_mp4_and_webm_processing_when_ffmpeg_available() {
        let Some(ffmpeg) = find_ffmpeg() else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("enki-video-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let mp4 = dir.join("blue.mp4");
        let ok = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x286ee0:s=160x90:d=0.5",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&mp4)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg failed to synthesize mp4");

        // Exercise native macOS AVFoundation frame extraction when VideoToolbox XPC is permitted.
        #[cfg(target_os = "macos")]
        if let Some(frame) = crate::native::extract_video_frame(&mp4) {
            assert_eq!(frame.dimensions(), (160, 90));
            let tint = dominant_color(&frame);
            assert!(tint.b > tint.r + 0.15, "expected blue tint, got {tint:?}");
        }

        let preview = cache.join("mp4.jpg");
        let thumb = cache.join("mp4.thumb.jpg");
        let tint_file = cache.join("mp4.tint");
        let wp = process_video(
            &mp4,
            BackgroundEffect::Blur,
            1,
            &cache,
            preview.clone(),
            thumb.clone(),
            tint_file,
        )
        .unwrap();
        assert!(preview.exists());
        assert_eq!(image::image_dimensions(&thumb).unwrap(), (THUMB_SIZE, THUMB_SIZE));
        assert!(wp.tint.b > wp.tint.r + 0.15);
        match wp.content {
            WallpaperContent::Video {
                path,
                blur_sigma,
                width,
                height,
                native_mac,
            } => {
                assert_eq!(path, mp4);
                assert_eq!(blur_sigma, BackgroundEffect::Blur.sigma());
                assert_eq!((width, height), (160, 90));
                assert_eq!(native_mac, cfg!(target_os = "macos"));
            }
            other => panic!("expected Video content, got {other:?}"),
        }

        // Also test WebM (transcoded to cached .mp4 on macOS, played directly via ffmpeg on Linux).
        let webm = dir.join("green.webm");
        let ok_webm = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x30c050:s=160x90:d=0.5",
                "-c:v",
                "libvpx",
            ])
            .arg(&webm)
            .status()
            .is_ok_and(|s| s.success());
        if ok_webm {
            let preview = cache.join("webm.jpg");
            let thumb = cache.join("webm.thumb.jpg");
            let tint_file = cache.join("webm.tint");
            let wp = process_video(
                &webm,
                BackgroundEffect::StrongBlur,
                2,
                &cache,
                preview.clone(),
                thumb.clone(),
                tint_file,
            )
            .unwrap();
            assert!(preview.exists());
            assert_eq!(image::image_dimensions(&thumb).unwrap(), (THUMB_SIZE, THUMB_SIZE));
            assert!(wp.tint.g > wp.tint.r);
            if cfg!(target_os = "macos") {
                if let WallpaperContent::Video {
                    path, native_mac, ..
                } = wp.content
                {
                    assert!(native_mac);
                    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("mp4"));
                    assert!(path.exists());
                } else {
                    panic!("expected Video content");
                }
            }
        }

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn subsample_frames_preserves_total_duration() {
        let frames: Vec<Frame> = (0..10)
            .map(|_| {
                Frame::from_parts(
                    RgbaImage::new(4, 4),
                    0,
                    0,
                    Delay::from_numer_denom_ms(50, 1),
                )
            })
            .collect();
        let kept = subsample_frames(frames, 4);
        assert_eq!(kept.len(), 4);
        let total_ms: u128 = kept
            .iter()
            .map(|f| Duration::from(f.delay()).as_millis())
            .sum();
        assert_eq!(total_ms, 500);
    }

    #[test]
    fn animated_target_dimensions_respects_memory_budget() {
        let (w, h) = animated_target_dimensions(1920, 1080, 180, false);
        let bytes = w as u64 * h as u64 * 4 * 180;
        assert!(bytes <= MAX_ANIMATED_BYTES + 1024 * 1024, "got {w}x{h} ({bytes} bytes)");
        assert!(w >= MIN_ANIMATED_EDGE);
    }
}
