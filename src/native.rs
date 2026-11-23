//! Bits GPUI doesn't expose: trackpad haptics, macOS "Liquid Glass", and
//! native backdrop layers (static image, animated GIF/WebP via Core Animation,
//! and hardware-accelerated looping video via AVFoundation).
//!
//! Glass works by stacking native views *under* GPUI's (transparent) Metal
//! view: a backdrop (window color, wallpaper, dim) at the bottom, then one
//! `NSGlassEffectView` per glass surface, positioned every frame from the
//! bounds GPUI laid out. GPUI keeps drawing all the content on top.
//! Elsewhere these calls are no-ops and the UI falls back to drawn glass.

use std::{path::Path, sync::Arc};

use gpui::{Bounds, Hsla, Pixels, RenderImage, SharedString, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    /// A plain tap, for buttons.
    Generic,
    /// Snapping into place.
    Alignment,
    /// Crossing a step, like a slider detent.
    LevelChange,
}

/// Media displayed in the native backdrop layer beneath GPUI.
#[derive(Clone, Debug)]
pub enum BackdropMedia<'a> {
    Image(&'a Path),
    Animated(Arc<RenderImage>),
    Video { path: &'a Path, blur_sigma: f32 },
}

/// What sits under the glass views (or directly under GPUI for native video).
#[derive(Clone, Debug)]
pub struct Backdrop<'a> {
    pub color: Hsla,
    pub media: Option<BackdropMedia<'a>>,
    /// Drawn over the wallpaper; alpha is the amount.
    pub dim: Hsla,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlassStyle {
    pub radius: Pixels,
    pub tint: Option<Hsla>,
    /// The more transparent "clear" variant, for controls over imagery.
    pub clear: bool,
    pub dark: bool,
}

#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(not(target_os = "macos"))]
mod fallback {
    use super::*;

    pub fn haptic(_: Haptic) {}

    pub fn glass_supported() -> bool {
        false
    }

    pub fn glass_begin(_: &Window, _: bool, _: Option<Backdrop>) {}

    pub fn glass_place(_: &Window, _: SharedString, _: Bounds<Pixels>, _: &GlassStyle) {}

    pub fn glass_end(_: &Window) {}

    pub fn glass_cleanup(_: &Window) {}
}

#[cfg(not(target_os = "macos"))]
pub use fallback::*;

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)] // objc 0.2's macros check a cfg this crate doesn't define.
mod mac {
    use super::*;
    use gpui::ImageId;
    use image::{DynamicImage, RgbImage};
    use objc::{
        class, msg_send,
        rc::autoreleasepool,
        runtime::{Class, Object, BOOL, NO, YES},
        sel, sel_impl,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{
        cell::RefCell,
        collections::{HashMap, HashSet},
        ffi::{c_void, CString},
        path::PathBuf,
        sync::OnceLock,
        time::Duration,
    };

    type Id = *mut Object;
    const NIL: Id = std::ptr::null_mut();

    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct CGRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    unsafe impl objc::Encode for CGRect {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct CGSize {
        w: f64,
        h: f64,
    }

    unsafe impl objc::Encode for CGSize {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{CGSize=dd}") }
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct CMTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    unsafe impl objc::Encode for CMTime {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{?=qiIq}") }
        }
    }

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVLayerVideoGravityResizeAspectFill: Id;
    }

    #[link(name = "QuartzCore", kind = "framework")]
    extern "C" {
        static kCAAnimationDiscrete: Id;
    }

    #[link(name = "CoreImage", kind = "framework")]
    extern "C" {}

    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {}

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFDataCreate(allocator: *const c_void, bytes: *const u8, length: isize) -> *const c_void;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGColorSpaceCreateDeviceRGB() -> *const c_void;
        fn CGColorSpaceRelease(space: *const c_void);
        fn CGDataProviderCreateWithCFData(data: *const c_void) -> *const c_void;
        fn CGDataProviderRelease(provider: *const c_void);
        fn CGImageCreate(
            width: usize,
            height: usize,
            bits_per_component: usize,
            bits_per_pixel: usize,
            bytes_per_row: usize,
            space: *const c_void,
            bitmap_info: u32,
            provider: *const c_void,
            decode: *const f64,
            should_interpolate: bool,
            intent: u32,
        ) -> *const c_void;
        fn CGImageRelease(image: *const c_void);
        fn CGImageGetWidth(image: *const c_void) -> usize;
        fn CGImageGetHeight(image: *const c_void) -> usize;
        fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: *const c_void,
            bitmap_info: u32,
        ) -> *const c_void;
        fn CGContextDrawImage(ctx: *const c_void, rect: CGRect, image: *const c_void);
        fn CGContextRelease(ctx: *const c_void);
    }

    const NS_WINDOW_BELOW: isize = -1;
    const NS_VIEW_WIDTH_SIZABLE: usize = 2;
    const NS_VIEW_HEIGHT_SIZABLE: usize = 16;
    /// `kCGImageByteOrder32Little | kCGImageAlphaPremultipliedFirst` (BGRA premultiplied).
    const BITMAP_BGRA_PREMULTIPLIED: u32 = (2 << 12) | 2;
    /// `kCGImageAlphaNoneSkipLast` (big-endian RGBX).
    const BITMAP_RGBX: u32 = 5;

    pub fn haptic(pattern: Haptic) {
        let pattern: isize = match pattern {
            Haptic::Generic => 0,
            Haptic::Alignment => 1,
            Haptic::LevelChange => 2,
        };
        unsafe {
            let performer: Id = msg_send![class!(NSHapticFeedbackManager), defaultPerformer];
            if !performer.is_null() {
                // NSHapticFeedbackPerformanceTimeNow
                let _: () =
                    msg_send![performer, performFeedbackPattern: pattern performanceTime: 1isize];
            }
        }
    }

    fn glass_class() -> Option<&'static Class> {
        static CLASS: OnceLock<Option<usize>> = OnceLock::new();
        CLASS
            .get_or_init(|| Class::get("NSGlassEffectView").map(|c| c as *const Class as usize))
            .map(|ptr| unsafe { &*(ptr as *const Class) })
    }

    /// `NSGlassEffectView` arrived in macOS 26.
    pub fn glass_supported() -> bool {
        glass_class().is_some()
    }

    /// Extracts frame 0 from an AVFoundation-supported video (MP4/MOV/M4V)
    /// without invoking external tools. Safe to call from a background thread.
    pub fn extract_video_frame(path: &Path) -> Option<DynamicImage> {
        autoreleasepool(|| unsafe {
            let url: Id =
                msg_send![class!(NSURL), fileURLWithPath: nsstring(&path.to_string_lossy())];
            if url.is_null() {
                return None;
            }
            let asset: Id = msg_send![class!(AVURLAsset), URLAssetWithURL: url options: NIL];
            if asset.is_null() {
                return None;
            }
            let generator: Id =
                msg_send![class!(AVAssetImageGenerator), assetImageGeneratorWithAsset: asset];
            if generator.is_null() {
                return None;
            }
            let _: () = msg_send![generator, setAppliesPreferredTrackTransform: YES];
            let max_size = CGSize {
                w: 1920.0,
                h: 1920.0,
            };
            let _: () = msg_send![generator, setMaximumSize: max_size];

            let zero = CMTime {
                value: 0,
                timescale: 600,
                flags: 1,
                epoch: 0,
            };
            let mut actual = CMTime {
                value: 0,
                timescale: 0,
                flags: 0,
                epoch: 0,
            };
            let mut err: Id = NIL;
            let cg_image: *const c_void = msg_send![
                generator,
                copyCGImageAtTime: zero
                actualTime: &mut actual
                error: &mut err
            ];
            if cg_image.is_null() {
                return None;
            }

            let w = CGImageGetWidth(cg_image);
            let h = CGImageGetHeight(cg_image);
            if w == 0 || h == 0 {
                CGImageRelease(cg_image);
                return None;
            }

            let space = CGColorSpaceCreateDeviceRGB();
            let mut rgbx = vec![0u8; w * h * 4];
            let ctx = CGBitmapContextCreate(
                rgbx.as_mut_ptr() as *mut c_void,
                w,
                h,
                8,
                w * 4,
                space,
                BITMAP_RGBX,
            );
            let result = if !ctx.is_null() {
                CGContextDrawImage(
                    ctx,
                    CGRect {
                        x: 0.0,
                        y: 0.0,
                        w: w as f64,
                        h: h as f64,
                    },
                    cg_image,
                );
                let mut rgb = Vec::with_capacity(w * h * 3);
                for px in rgbx.as_chunks::<4>().0 {
                    rgb.extend_from_slice(&px[..3]);
                }
                RgbImage::from_raw(w as u32, h as u32, rgb).map(DynamicImage::ImageRgb8)
            } else {
                None
            };

            if !ctx.is_null() {
                CGContextRelease(ctx);
            }
            CGColorSpaceRelease(space);
            CGImageRelease(cg_image);
            result
        })
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum MediaKey {
        Image(PathBuf),
        Animated(ImageId),
        Video(PathBuf),
    }

    struct VideoBackdrop {
        clip_view: Id,
        video_view: Id,
        player_layer: Id,
        player: Id,
        looper: Id,
        blur_sigma: f32,
    }

    struct Surface {
        view: Id,
        frame: CGRect,
        style: GlassStyle,
        hidden: bool,
    }

    struct Host {
        parent: Id,
        color_view: Option<Id>,
        image_view: Option<Id>,
        video: Option<VideoBackdrop>,
        dim_view: Option<Id>,
        color: Option<Hsla>,
        dim: Option<Hsla>,
        media_key: Option<MediaKey>,
        surfaces: HashMap<SharedString, Surface>,
        seen: HashSet<SharedString>,
    }

    impl Default for Host {
        fn default() -> Self {
            Self {
                parent: NIL,
                color_view: None,
                image_view: None,
                video: None,
                dim_view: None,
                color: None,
                dim: None,
                media_key: None,
                surfaces: HashMap::new(),
                seen: HashSet::new(),
            }
        }
    }

    thread_local! {
        /// Keyed by GPUI's NSView, one per window.
        static HOSTS: RefCell<HashMap<usize, Host>> = RefCell::new(HashMap::new());
    }

    fn gpui_view(window: &Window) -> Option<Id> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr() as Id),
            _ => None,
        }
    }

    unsafe fn nscolor(c: Hsla) -> Id {
        let rgb = c.to_rgb();
        msg_send![class!(NSColor), colorWithSRGBRed: rgb.r as f64 green: rgb.g as f64 blue: rgb.b as f64 alpha: c.a as f64]
    }

    unsafe fn nsstring(s: &str) -> Id {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn bounds_of(view: Id) -> CGRect {
        msg_send![view, bounds]
    }

    /// A plain layer-backed view filling `parent`, inserted right below GPUI's view.
    unsafe fn fill_view(gpui: Id) -> Id {
        let parent: Id = msg_send![gpui, superview];
        let view: Id = msg_send![class!(NSView), alloc];
        let view: Id = msg_send![view, initWithFrame: bounds_of(parent)];
        let _: () = msg_send![view, setWantsLayer: YES];
        let _: () =
            msg_send![view, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
        let _: () =
            msg_send![parent, addSubview: view positioned: NS_WINDOW_BELOW relativeTo: gpui];
        view
    }

    unsafe fn set_layer_color(view: Id, color: Hsla) {
        let layer: Id = msg_send![view, layer];
        let cg: *const c_void = msg_send![nscolor(color), CGColor];
        let _: () = msg_send![layer, setBackgroundColor: cg];
    }

    unsafe fn remove(view: Id) {
        let _: () = msg_send![view, removeFromSuperview];
        let _: () = msg_send![view, release];
    }

    unsafe fn teardown_media(host: &mut Host) {
        if let Some(view) = host.image_view.take() {
            remove(view);
        }
        if let Some(video) = host.video.take() {
            let _: () = msg_send![video.player, pause];
            let _: () = msg_send![video.looper, disableLooping];
            let _: () = msg_send![video.player_layer, setPlayer: NIL];
            let _: () = msg_send![video.looper, release];
            let _: () = msg_send![video.player, release];
            remove(video.video_view);
            remove(video.clip_view);
        }
        if let Some(view) = host.dim_view.take() {
            remove(view);
        }
        host.dim = None;
        host.media_key = None;
    }

    /// Starts a frame. `glass_enabled` controls Liquid Glass surfaces; `backdrop`
    /// controls the native backdrop layer (used for Liquid Glass and for native
    /// video playback even when Liquid Glass is off).
    pub fn glass_begin(window: &Window, glass_enabled: bool, backdrop: Option<Backdrop>) {
        let Some(gpui) = gpui_view(window) else {
            return;
        };
        let glass_active = glass_enabled && glass_supported();
        let need_host = glass_active || backdrop.is_some();

        HOSTS.with_borrow_mut(|hosts| {
            let key = gpui as usize;
            let parent: Id = unsafe { msg_send![gpui, superview] };
            // A host left behind by a closed window whose view address got reused.
            let stale = hosts.get(&key).is_some_and(|h| h.parent != parent);
            if !need_host || stale {
                if let Some(host) = hosts.remove(&key) {
                    unsafe { teardown(host) };
                }
                if !need_host {
                    return;
                }
            }
            let host = hosts.entry(key).or_insert_with(|| Host {
                parent,
                ..Host::default()
            });
            host.seen.clear();
            if !glass_active && !host.surfaces.is_empty() {
                for surface in host.surfaces.drain().map(|(_, s)| s) {
                    unsafe { remove(surface.view) };
                }
            }
            if let Some(backdrop) = backdrop {
                unsafe { sync_backdrop(host, gpui, &backdrop) };
            } else {
                unsafe {
                    teardown_media(host);
                    if let Some(view) = host.color_view.take() {
                        remove(view);
                    }
                    host.color = None;
                }
            }
        });
    }

    unsafe fn teardown(mut host: Host) {
        teardown_media(&mut host);
        if let Some(view) = host.color_view.take() {
            remove(view);
        }
        for surface in host.surfaces.into_values() {
            remove(surface.view);
        }
    }

    unsafe fn sync_backdrop(host: &mut Host, gpui: Id, backdrop: &Backdrop) {
        let color_view = *host.color_view.get_or_insert_with(|| fill_view(gpui));
        if host.color != Some(backdrop.color) {
            set_layer_color(color_view, backdrop.color);
            host.color = Some(backdrop.color);
        }

        let desired_key = backdrop.media.as_ref().map(|m| match m {
            BackdropMedia::Image(p) => MediaKey::Image(p.to_path_buf()),
            BackdropMedia::Animated(img) => MediaKey::Animated(img.id),
            BackdropMedia::Video { path, .. } => MediaKey::Video(path.to_path_buf()),
        });

        if host.media_key != desired_key {
            teardown_media(host);
            match &backdrop.media {
                Some(BackdropMedia::Image(path)) => {
                    install_static_image(host, gpui, path);
                }
                Some(BackdropMedia::Animated(render_image)) => {
                    install_animated_image(host, gpui, render_image);
                }
                Some(BackdropMedia::Video { path, blur_sigma }) => {
                    install_video(host, gpui, path, *blur_sigma);
                }
                None => {}
            }
            host.media_key = desired_key;
            // Glass views must stay above the new backdrop layers.
            for surface in host.surfaces.values() {
                let parent: Id = msg_send![gpui, superview];
                let _: () = msg_send![parent, addSubview: surface.view positioned: NS_WINDOW_BELOW relativeTo: gpui];
            }
        } else if let (Some(BackdropMedia::Video { blur_sigma, .. }), Some(video)) =
            (&backdrop.media, host.video.as_mut())
        {
            if (video.blur_sigma - *blur_sigma).abs() > f32::EPSILON {
                apply_video_blur(video, *blur_sigma);
            }
        }

        if let Some(dim_view) = host.dim_view {
            if host.dim != Some(backdrop.dim) {
                set_layer_color(dim_view, backdrop.dim);
                host.dim = Some(backdrop.dim);
            }
        }
    }

    unsafe fn install_static_image(host: &mut Host, gpui: Id, path: &Path) {
        let image: Id = msg_send![class!(NSImage), alloc];
        let image: Id = msg_send![image, initWithContentsOfFile: nsstring(&path.to_string_lossy())];
        if !image.is_null() {
            let view = fill_view(gpui);
            let layer: Id = msg_send![view, layer];
            let _: () = msg_send![layer, setContents: image];
            let _: () = msg_send![layer, setContentsGravity: nsstring("resizeAspectFill")];
            let _: () = msg_send![layer, setMasksToBounds: YES];
            let _: () = msg_send![image, release];
            host.image_view = Some(view);
            host.dim_view = Some(fill_view(gpui));
        }
    }

    /// Creates a `CGImage` from a single BGRA frame of a GPUI `RenderImage`.
    unsafe fn cgimage_from_frame(render_image: &RenderImage, frame_index: usize) -> *const c_void {
        let Some(bytes) = render_image.as_bytes(frame_index) else {
            return std::ptr::null();
        };
        let sz = render_image.size(frame_index);
        let w = sz.width.0 as usize;
        let h = sz.height.0 as usize;
        if w == 0 || h == 0 || bytes.len() < w * h * 4 {
            return std::ptr::null();
        }
        // Premultiply RGB by alpha in case the GIF/WebP has transparency.
        let mut premul = Vec::with_capacity(w * h * 4);
        for px in bytes[..w * h * 4].as_chunks::<4>().0 {
            let a = px[3] as u16;
            if a == 255 {
                premul.extend_from_slice(px);
            } else {
                premul.push(((px[0] as u16 * a + 127) / 255) as u8);
                premul.push(((px[1] as u16 * a + 127) / 255) as u8);
                premul.push(((px[2] as u16 * a + 127) / 255) as u8);
                premul.push(px[3]);
            }
        }
        let cf_data = CFDataCreate(std::ptr::null(), premul.as_ptr(), premul.len() as isize);
        if cf_data.is_null() {
            return std::ptr::null();
        }
        let provider = CGDataProviderCreateWithCFData(cf_data);
        CFRelease(cf_data);
        if provider.is_null() {
            return std::ptr::null();
        }
        let space = CGColorSpaceCreateDeviceRGB();
        let cg = CGImageCreate(
            w,
            h,
            8,
            32,
            w * 4,
            space,
            BITMAP_BGRA_PREMULTIPLIED,
            provider,
            std::ptr::null(),
            true,
            0,
        );
        CGColorSpaceRelease(space);
        CGDataProviderRelease(provider);
        cg
    }

    unsafe fn install_animated_image(host: &mut Host, gpui: Id, render_image: &RenderImage) {
        let count = render_image.frame_count();
        if count == 0 {
            return;
        }
        let mut cg_images: Vec<*const c_void> = Vec::with_capacity(count);
        let mut delays_ms: Vec<f64> = Vec::with_capacity(count);
        for i in 0..count {
            let cg = cgimage_from_frame(render_image, i);
            if !cg.is_null() {
                cg_images.push(cg);
                let ms = Duration::from(render_image.delay(i)).as_secs_f64() * 1000.0;
                delays_ms.push(if ms <= 0.0 { 100.0 } else { ms });
            }
        }
        if cg_images.is_empty() {
            return;
        }

        let view = fill_view(gpui);
        let layer: Id = msg_send![view, layer];
        let _: () = msg_send![layer, setContents: cg_images[0]];
        let _: () = msg_send![layer, setContentsGravity: nsstring("resizeAspectFill")];
        let _: () = msg_send![layer, setMasksToBounds: YES];

        if cg_images.len() > 1 {
            let values: Id = msg_send![class!(NSMutableArray), arrayWithCapacity: cg_images.len()];
            let key_times: Id =
                msg_send![class!(NSMutableArray), arrayWithCapacity: cg_images.len() + 1];
            let total_ms: f64 = delays_ms.iter().sum::<f64>().max(1.0);
            let mut acc = 0.0f64;

            let zero: Id = msg_send![class!(NSNumber), numberWithDouble: 0.0f64];
            let _: () = msg_send![key_times, addObject: zero];
            for (&cg, &ms) in cg_images.iter().zip(delays_ms.iter()) {
                let _: () = msg_send![values, addObject: cg];
                acc += ms;
                let frac: Id =
                    msg_send![class!(NSNumber), numberWithDouble: (acc / total_ms).min(1.0)];
                let _: () = msg_send![key_times, addObject: frac];
            }

            let anim: Id =
                msg_send![class!(CAKeyframeAnimation), animationWithKeyPath: nsstring("contents")];
            let _: () = msg_send![anim, setValues: values];
            let _: () = msg_send![anim, setKeyTimes: key_times];
            let _: () = msg_send![anim, setDuration: total_ms / 1000.0];
            let _: () = msg_send![anim, setCalculationMode: kCAAnimationDiscrete];
            let _: () = msg_send![anim, setRepeatCount: f32::INFINITY];
            let _: () = msg_send![anim, setRemovedOnCompletion: NO];
            let _: () = msg_send![layer, addAnimation: anim forKey: nsstring("enkiWallpaperAnim")];
        }

        for cg in cg_images {
            CGImageRelease(cg);
        }

        host.image_view = Some(view);
        host.dim_view = Some(fill_view(gpui));
    }

    unsafe fn make_blur_filters(blur_sigma: f32) -> Id {
        if blur_sigma <= 0.0 {
            return NIL;
        }
        let filter: Id = msg_send![class!(CIFilter), filterWithName: nsstring("CIGaussianBlur")];
        if filter.is_null() {
            return NIL;
        }
        let _: () = msg_send![filter, setDefaults];
        let radius: Id = msg_send![class!(NSNumber), numberWithDouble: blur_sigma as f64];
        let _: () = msg_send![filter, setValue: radius forKey: nsstring("inputRadius")];
        msg_send![class!(NSArray), arrayWithObject: filter]
    }

    unsafe fn apply_video_blur(video: &mut VideoBackdrop, blur_sigma: f32) {
        let clip_bounds = bounds_of(video.clip_view);
        // Expand the video layer beyond the clipped container so Gaussian blur
        // never feathers the window edges into transparency.
        let pad = if blur_sigma > 0.0 {
            (blur_sigma as f64 * 2.5).ceil()
        } else {
            0.0
        };
        let frame = CGRect {
            x: -pad,
            y: -pad,
            w: clip_bounds.w + pad * 2.0,
            h: clip_bounds.h + pad * 2.0,
        };
        let _: () = msg_send![video.video_view, setFrame: frame];
        let filters = make_blur_filters(blur_sigma);
        let _: () = msg_send![video.player_layer, setFilters: filters];
        video.blur_sigma = blur_sigma;
    }

    unsafe fn install_video(host: &mut Host, gpui: Id, path: &Path, blur_sigma: f32) {
        let url: Id =
            msg_send![class!(NSURL), fileURLWithPath: nsstring(&path.to_string_lossy())];
        if url.is_null() {
            return;
        }
        let item: Id = msg_send![class!(AVPlayerItem), playerItemWithURL: url];
        if item.is_null() {
            return;
        }
        let player: Id = msg_send![class!(AVQueuePlayer), alloc];
        let player: Id = msg_send![player, init];
        let _: () = msg_send![player, setMuted: YES];
        let _: () = msg_send![player, setAutomaticallyWaitsToMinimizeStalling: NO];

        let looper: Id =
            msg_send![class!(AVPlayerLooper), playerLooperWithPlayer: player templateItem: item];
        if looper.is_null() {
            let _: () = msg_send![player, release];
            return;
        }
        let _: Id = msg_send![looper, retain];

        let player_layer: Id = msg_send![class!(AVPlayerLayer), playerLayerWithPlayer: player];
        let _: () =
            msg_send![player_layer, setVideoGravity: AVLayerVideoGravityResizeAspectFill];

        let clip_view = fill_view(gpui);
        let clip_layer: Id = msg_send![clip_view, layer];
        let _: () = msg_send![clip_layer, setMasksToBounds: YES];

        let video_view: Id = msg_send![class!(NSView), alloc];
        let video_view: Id = msg_send![video_view, initWithFrame: bounds_of(clip_view)];
        let _: () = msg_send![
            video_view,
            setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
        ];
        let _: () = msg_send![video_view, setLayer: player_layer];
        let _: () = msg_send![video_view, setWantsLayer: YES];
        let _: () = msg_send![video_view, setLayerUsesCoreImageFilters: YES];
        let _: () = msg_send![clip_view, addSubview: video_view];

        let mut video = VideoBackdrop {
            clip_view,
            video_view,
            player_layer,
            player,
            looper,
            blur_sigma: -1.0,
        };
        apply_video_blur(&mut video, blur_sigma);
        let _: () = msg_send![player, play];

        host.video = Some(video);
        host.dim_view = Some(fill_view(gpui));
    }

    /// Puts a glass surface under `bounds` (window coordinates) for this frame.
    pub fn glass_place(
        window: &Window,
        key: SharedString,
        bounds: Bounds<Pixels>,
        style: &GlassStyle,
    ) {
        let Some(gpui) = gpui_view(window) else {
            return;
        };
        let Some(class) = glass_class() else {
            return;
        };
        HOSTS.with_borrow_mut(|hosts| {
            let Some(host) = hosts.get_mut(&(gpui as usize)) else {
                return;
            };
            unsafe {
                let parent: Id = msg_send![gpui, superview];
                let height = bounds_of(parent).h;
                let frame = CGRect {
                    x: f32::from(bounds.origin.x) as f64,
                    y: height - f32::from(bounds.origin.y + bounds.size.height) as f64,
                    w: f32::from(bounds.size.width) as f64,
                    h: f32::from(bounds.size.height) as f64,
                };
                let surface = host.surfaces.entry(key.clone()).or_insert_with(|| {
                    let view: Id = msg_send![class, alloc];
                    let view: Id = msg_send![view, initWithFrame: frame];
                    let _: () = msg_send![parent, addSubview: view positioned: NS_WINDOW_BELOW relativeTo: gpui];
                    Surface {
                        view,
                        frame,
                        style: GlassStyle {
                            radius: Pixels::from(-1.),
                            tint: None,
                            clear: false,
                            dark: false,
                        },
                        hidden: false,
                    }
                });
                if surface.frame != frame {
                    let _: () = msg_send![surface.view, setFrame: frame];
                    surface.frame = frame;
                }
                if surface.hidden {
                    let _: () = msg_send![surface.view, setHidden: false as BOOL];
                    surface.hidden = false;
                }
                if surface.style != *style {
                    let _: () = msg_send![surface.view, setCornerRadius: f32::from(style.radius) as f64];
                    let tint = style.tint.map(|c| nscolor(c)).unwrap_or(NIL);
                    let _: () = msg_send![surface.view, setTintColor: tint];
                    // NSGlassEffectViewStyleRegular = 0, Clear = 1.
                    let _: () = msg_send![surface.view, setStyle: style.clear as isize];
                    // Follow the app's scheme, which can differ from the system's.
                    let name = if style.dark {
                        "NSAppearanceNameDarkAqua"
                    } else {
                        "NSAppearanceNameAqua"
                    };
                    let appearance: Id =
                        msg_send![class!(NSAppearance), appearanceNamed: nsstring(name)];
                    let _: () = msg_send![surface.view, setAppearance: appearance];
                    surface.style = style.clone();
                }
            }
            host.seen.insert(key);
        });
    }

    /// Hides surfaces that weren't placed this frame (page changed, sidebar closed).
    pub fn glass_end(window: &Window) {
        let Some(gpui) = gpui_view(window) else {
            return;
        };
        HOSTS.with_borrow_mut(|hosts| {
            let Some(host) = hosts.get_mut(&(gpui as usize)) else {
                return;
            };
            for (key, surface) in host.surfaces.iter_mut() {
                if !surface.hidden && !host.seen.contains(key) {
                    unsafe {
                        let _: () = msg_send![surface.view, setHidden: YES];
                    }
                    surface.hidden = true;
                }
            }
        });
    }

    /// Tears down and removes native glass views for a closing window.
    pub fn glass_cleanup(window: &Window) {
        let Some(gpui) = gpui_view(window) else {
            return;
        };
        HOSTS.with_borrow_mut(|hosts| {
            if let Some(host) = hosts.remove(&(gpui as usize)) {
                unsafe { teardown(host) };
            }
        });
    }
}
