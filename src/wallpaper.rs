//! Background image processing: blur (GPUI has no image filters) and the
//! dominant color used for "match wallpaper colors".

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context as _, Result};
use gpui::Rgba;
use image::{imageops::FilterType, DynamicImage, GenericImageView};

use crate::settings::BackgroundEffect;

#[derive(Clone, Debug)]
pub struct Wallpaper {
    /// What actually gets drawn (blurred and downscaled copy in the cache).
    pub rendered: PathBuf,
    pub tint: Rgba,
}

const MAX_EDGE: u32 = 2400;
/// Bump when processing changes so cached results are recomputed.
const CACHE_VERSION: u32 = 2;
const MAX_CACHED_WALLPAPERS: usize = 20;

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
    let mut wallpapers: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jpg") {
                return None;
            }
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                (mtime, path)
            })
        })
        .collect();
    if wallpapers.len() <= max_wallpapers {
        return;
    }
    wallpapers.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, jpg) in wallpapers.into_iter().skip(max_wallpapers) {
        let tint = jpg.with_extension("tint");
        let _ = std::fs::remove_file(&jpg);
        let _ = std::fs::remove_file(&tint);
    }
}

pub fn process(source: &Path, effect: BackgroundEffect) -> Result<Wallpaper> {
    let meta = std::fs::metadata(source).with_context(|| format!("reading {source:?}"))?;
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    effect.hash(&mut hasher);
    CACHE_VERSION.hash(&mut hasher);
    let key = hasher.finish();

    let cache = crate::paths::cache_dir().join("wallpapers");
    std::fs::create_dir_all(&cache)?;
    let rendered = cache.join(format!("{key:016x}.jpg"));
    let tint_file = cache.join(format!("{key:016x}.tint"));

    if let (true, Some(tint)) = (rendered.exists(), read_tint(&tint_file)) {
        touch_modified(&rendered);
        return Ok(Wallpaper { rendered, tint });
    }

    let img = match image::open(source) {
        Ok(img) => img,
        Err(err) => decode_with_system(source, &cache)
            .ok_or(err)
            .with_context(|| format!("decoding {source:?}"))?,
    };
    let tint = dominant_color(&img);

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
        let small = out.resize(w / 4, h / 4, FilterType::Triangle);
        let blurred = small.fast_blur(sigma / 4.0);
        out = blurred.resize_exact(w / 2, h / 2, FilterType::Triangle);
    }
    out.to_rgb8()
        .save_with_format(&rendered, image::ImageFormat::Jpeg)
        .context("saving processed wallpaper")?;
    let _ = std::fs::write(&tint_file, format!("{} {} {}", tint.r, tint.g, tint.b));
    prune_wallpaper_cache(&cache, MAX_CACHED_WALLPAPERS);

    Ok(Wallpaper { rendered, tint })
}

/// macOS wallpapers are HEIC, which the `image` crate can't read; `sips` can.
#[cfg(target_os = "macos")]
fn decode_with_system(source: &Path, cache: &Path) -> Option<DynamicImage> {
    let out = cache.join("converted.jpg");
    let ok = std::process::Command::new("sips")
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
    use image::{Rgb, RgbImage};

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
        use std::time::Duration;

        let dir =
            std::env::temp_dir().join(format!("enki-wallpaper-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        for i in 0..4u64 {
            let jpg = dir.join(format!("wall-{i}.jpg"));
            let tint = dir.join(format!("wall-{i}.tint"));
            std::fs::write(&jpg, b"jpg").unwrap();
            std::fs::write(&tint, b"0.1 0.2 0.3").unwrap();
            let f = std::fs::OpenOptions::new().write(true).open(&jpg).unwrap();
            f.set_modified(now - Duration::from_secs((4 - i) * 60))
                .unwrap();
        }

        prune_wallpaper_cache(&dir, 2);

        for i in 0..2 {
            assert!(!dir.join(format!("wall-{i}.jpg")).exists());
            assert!(!dir.join(format!("wall-{i}.tint")).exists());
        }
        for i in 2..4 {
            assert!(dir.join(format!("wall-{i}.jpg")).exists());
            assert!(dir.join(format!("wall-{i}.tint")).exists());
        }

        let _ = std::fs::remove_dir_all(dir);
    }
}
