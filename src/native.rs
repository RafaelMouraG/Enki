//! Bits GPUI doesn't expose: trackpad haptics and macOS "Liquid Glass".
//!
//! Glass works by stacking native views *under* GPUI's (transparent) Metal
//! view: a backdrop (window color, wallpaper, dim) at the bottom, then one
//! `NSGlassEffectView` per glass surface, positioned every frame from the
//! bounds GPUI laid out. GPUI keeps drawing all the content on top.
//! Elsewhere these calls are no-ops and the UI falls back to drawn glass.

use std::path::Path;

use gpui::{Bounds, Hsla, Pixels, SharedString, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    /// A plain tap, for buttons.
    Generic,
    /// Snapping into place.
    Alignment,
    /// Crossing a step, like a slider detent.
    LevelChange,
}

/// What sits under the glass views.
#[derive(Clone, Debug, PartialEq)]
pub struct Backdrop<'a> {
    pub color: Hsla,
    pub wallpaper: Option<&'a Path>,
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
    use objc::{
        class, msg_send,
        runtime::{Class, Object, BOOL, YES},
        sel, sel_impl,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{
        cell::RefCell,
        collections::{HashMap, HashSet},
        ffi::{c_void, CString},
        path::PathBuf,
        sync::OnceLock,
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

    const NS_WINDOW_BELOW: isize = -1;
    const NS_VIEW_WIDTH_SIZABLE: usize = 2;
    const NS_VIEW_HEIGHT_SIZABLE: usize = 16;

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
        dim_view: Option<Id>,
        color: Option<Hsla>,
        dim: Option<Hsla>,
        wallpaper: Option<PathBuf>,
        surfaces: HashMap<SharedString, Surface>,
        seen: HashSet<SharedString>,
    }

    impl Default for Host {
        fn default() -> Self {
            Self {
                parent: NIL,
                color_view: None,
                image_view: None,
                dim_view: None,
                color: None,
                dim: None,
                wallpaper: None,
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

    /// Starts a frame. Disabled tears everything down (glass turned off);
    /// without a backdrop the glass shows whatever is behind the window.
    pub fn glass_begin(window: &Window, enabled: bool, backdrop: Option<Backdrop>) {
        let Some(gpui) = gpui_view(window) else {
            return;
        };
        HOSTS.with_borrow_mut(|hosts| {
            let key = gpui as usize;
            let parent: Id = unsafe { msg_send![gpui, superview] };
            // A host left behind by a closed window whose view address got reused.
            let stale = hosts.get(&key).is_some_and(|h| h.parent != parent);
            if !enabled || !glass_supported() || stale {
                if let Some(host) = hosts.remove(&key) {
                    unsafe { teardown(host) };
                }
                if !enabled || !glass_supported() {
                    return;
                }
            }
            let host = hosts.entry(key).or_insert_with(|| Host {
                parent,
                ..Host::default()
            });
            host.seen.clear();
            if let Some(backdrop) = backdrop {
                unsafe { sync_backdrop(host, gpui, &backdrop) };
            }
        });
    }

    unsafe fn teardown(host: Host) {
        for view in [host.color_view, host.image_view, host.dim_view]
            .into_iter()
            .flatten()
        {
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

        let wallpaper = backdrop.wallpaper.map(Path::to_path_buf);
        if host.wallpaper != wallpaper {
            if let Some(view) = host.image_view.take() {
                remove(view);
            }
            if let Some(view) = host.dim_view.take() {
                remove(view);
            }
            host.dim = None;
            if let Some(path) = &wallpaper {
                let image: Id = msg_send![class!(NSImage), alloc];
                let image: Id =
                    msg_send![image, initWithContentsOfFile: nsstring(&path.to_string_lossy())];
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
            host.wallpaper = wallpaper;
            // Glass views must stay above the new backdrop layers.
            for surface in host.surfaces.values() {
                let parent: Id = msg_send![gpui, superview];
                let _: () = msg_send![parent, addSubview: surface.view positioned: NS_WINDOW_BELOW relativeTo: gpui];
            }
        }
        if let Some(dim_view) = host.dim_view {
            if host.dim != Some(backdrop.dim) {
                set_layer_color(dim_view, backdrop.dim);
                host.dim = Some(backdrop.dim);
            }
        }
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
