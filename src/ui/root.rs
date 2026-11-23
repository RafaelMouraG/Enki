//! The main window: title bar, sidebar and the current page.

use std::{path::PathBuf, sync::Arc, time::Instant};

use gpui::{
    canvas, div, img, linear_color_stop, linear_gradient, point, prelude::*, px, AnyElement, App,
    BoxShadow, Context, Decorations, Entity, FocusHandle, Focusable, ImageId, ImageSource,
    MouseButton, ObjectFit, Pixels, RenderImage, ResizeEdge, SharedString, Subscription, Tiling,
    Window, WindowControlArea,
};
use image::{Frame, RgbaImage};

use crate::{
    actions,
    assets::Icon,
    native,
    state::{Enki, Page},
    theme::{radius, with_alpha, Theme, UI_FONT},
    timer::Phase,
    ui::{
        components::StyledExt as _,
        components::{icon, icon_button, icon_sized, WindowDrag},
        on_model, settings_page, sidebar, stats_page, tasks_page,
        text_input::{TextInput, TextInputEvent},
        timer_page, wrapped_page,
    },
    wallpaper::WallpaperContent,
};

pub const TITLEBAR_HEIGHT: f32 = 52.;
/// Room left for the macOS traffic lights.
pub const TRAFFIC_LIGHTS_WIDTH: f32 = 84.;
/// Corner radius of floating client-decorated windows (Linux Wayland/X11).
const WINDOW_RADIUS: Pixels = px(12.);
/// Transparent outer margin for the drop shadow and resize grips on Linux CSD.
const WINDOW_SHADOW_INSET: Pixels = px(12.);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct WindowCorners {
    tl: Pixels,
    tr: Pixels,
    br: Pixels,
    bl: Pixels,
}

impl WindowCorners {
    fn from_tiling(tiling: Tiling) -> Self {
        Self {
            tl: if !(tiling.top || tiling.left) {
                WINDOW_RADIUS
            } else {
                px(0.)
            },
            tr: if !(tiling.top || tiling.right) {
                WINDOW_RADIUS
            } else {
                px(0.)
            },
            br: if !(tiling.bottom || tiling.right) {
                WINDOW_RADIUS
            } else {
                px(0.)
            },
            bl: if !(tiling.bottom || tiling.left) {
                WINDOW_RADIUS
            } else {
                px(0.)
            },
        }
    }

    fn any(self) -> bool {
        self.tl > px(0.) || self.tr > px(0.) || self.br > px(0.) || self.bl > px(0.)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RoundedSourceKey {
    Static(PathBuf),
    Animated(ImageId),
    Video(ImageId),
}

/// Keeps an aspect-matched center-cropped `RenderImage` so GPUI's shader can
/// apply SDF `corner_radii` directly at the window corners on Linux CSD.
#[derive(Default)]
struct RoundedWallpaperCache {
    source_key: Option<RoundedSourceKey>,
    static_bgra: Option<(u32, u32, Vec<u8>)>,
    aspect_bucket: u32,
    last_anim_crop: Option<Instant>,
    cropped: Option<Arc<RenderImage>>,
}

impl RoundedWallpaperCache {
    fn clear(&mut self, cx: &mut App) {
        self.source_key = None;
        self.static_bgra = None;
        self.aspect_bucket = 0;
        self.last_anim_crop = None;
        if let Some(old) = self.cropped.take() {
            cx.drop_image(old, None);
        }
    }

    fn replace_cropped(&mut self, next: Option<Arc<RenderImage>>, cx: &mut App) {
        if let Some(old) = std::mem::replace(&mut self.cropped, next) {
            cx.drop_image(old, None);
        }
    }

    fn get_or_update(
        &mut self,
        wallpaper: &crate::wallpaper::Wallpaper,
        video_frame: Option<&Arc<RenderImage>>,
        dst_w: f32,
        dst_h: f32,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        if dst_w <= 0.0 || dst_h <= 0.0 {
            return None;
        }
        let bucket = ((dst_w / dst_h) * 200.0).round() as u32;

        match &wallpaper.content {
            WallpaperContent::Static => {
                self.update_static(&wallpaper.preview, dst_w, dst_h, bucket, cx)
            }
            WallpaperContent::Animated(render_img) => {
                let key = RoundedSourceKey::Animated(render_img.id);
                let source_changed = self.source_key.as_ref() != Some(&key);
                let aspect_changed = self.aspect_bucket != bucket;
                let debounce_ok = self
                    .last_anim_crop
                    .is_none_or(|t| t.elapsed().as_millis() >= 150);

                if source_changed || (aspect_changed && debounce_ok) || self.cropped.is_none() {
                    let cropped = crop_render_image_to_aspect(render_img, dst_w, dst_h);
                    self.static_bgra = None;
                    self.source_key = Some(key);
                    self.aspect_bucket = bucket;
                    self.last_anim_crop = Some(Instant::now());
                    self.replace_cropped(cropped, cx);
                }
                self.cropped.clone()
            }
            WallpaperContent::Video { .. } => {
                if let Some(frame) = video_frame {
                    let key = RoundedSourceKey::Video(frame.id);
                    if self.source_key.as_ref() != Some(&key)
                        || self.aspect_bucket != bucket
                        || self.cropped.is_none()
                    {
                        let cropped = crop_render_image_to_aspect(frame, dst_w, dst_h);
                        self.static_bgra = None;
                        self.source_key = Some(key);
                        self.aspect_bucket = bucket;
                        self.replace_cropped(cropped, cx);
                    }
                    self.cropped.clone()
                } else {
                    self.update_static(&wallpaper.preview, dst_w, dst_h, bucket, cx)
                }
            }
        }
    }

    fn update_static(
        &mut self,
        preview: &PathBuf,
        dst_w: f32,
        dst_h: f32,
        bucket: u32,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        let key = RoundedSourceKey::Static(preview.clone());
        let source_changed = self.source_key.as_ref() != Some(&key);
        if source_changed || self.static_bgra.is_none() {
            let mut rgba = image::open(preview).ok()?.into_rgba8();
            let (w, h) = rgba.dimensions();
            for px in rgba.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
            self.static_bgra = Some((w, h, rgba.into_raw()));
            self.source_key = Some(key);
            self.aspect_bucket = 0;
        }
        if self.aspect_bucket != bucket || self.cropped.is_none() {
            let (w, h, bytes) = self.static_bgra.as_ref()?;
            let cropped = crop_bgra_frame(bytes, *w, *h, dst_w, dst_h)
                .map(|img| Arc::new(RenderImage::new(vec![Frame::new(img)])));
            self.aspect_bucket = bucket;
            self.replace_cropped(cropped, cx);
        }
        self.cropped.clone()
    }
}

/// Computes a centered crop rectangle `(x, y, crop_w, crop_h)` inside
/// `(src_w, src_h)` matching `dst_w / dst_h`.
fn crop_rect_for_aspect(src_w: u32, src_h: u32, dst_w: f32, dst_h: f32) -> (u32, u32, u32, u32) {
    if src_w == 0 || src_h == 0 || dst_w <= 0.0 || dst_h <= 0.0 {
        return (0, 0, src_w, src_h);
    }
    let src_aspect = src_w as f32 / src_h as f32;
    let dst_aspect = dst_w / dst_h;
    if (src_aspect - dst_aspect).abs() / src_aspect.max(dst_aspect) < 0.005 {
        return (0, 0, src_w, src_h);
    }
    if src_aspect > dst_aspect {
        let crop_w = ((src_h as f32 * dst_aspect).round() as u32).clamp(1, src_w);
        ((src_w - crop_w) / 2, 0, crop_w, src_h)
    } else {
        let crop_h = ((src_w as f32 / dst_aspect).round() as u32).clamp(1, src_h);
        (0, (src_h - crop_h) / 2, src_w, crop_h)
    }
}

fn crop_bgra_frame(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: f32,
    dst_h: f32,
) -> Option<RgbaImage> {
    let (cx, cy, cw, ch) = crop_rect_for_aspect(src_w, src_h, dst_w, dst_h);
    if cw == 0 || ch == 0 {
        return None;
    }
    let src_stride = (src_w as usize) * 4;
    let dst_stride = (cw as usize) * 4;
    if src.len() < src_stride * (src_h as usize) {
        return None;
    }
    if cx == 0 && cy == 0 && cw == src_w && ch == src_h {
        return RgbaImage::from_raw(cw, ch, src[..src_stride * (src_h as usize)].to_vec());
    }
    let mut out = vec![0u8; dst_stride * (ch as usize)];
    let x_off = (cx as usize) * 4;
    for row in 0..(ch as usize) {
        let src_start = ((cy as usize) + row) * src_stride + x_off;
        let dst_start = row * dst_stride;
        out[dst_start..dst_start + dst_stride]
            .copy_from_slice(&src[src_start..src_start + dst_stride]);
    }
    RgbaImage::from_raw(cw, ch, out)
}

fn crop_render_image_to_aspect(
    src: &RenderImage,
    dst_w: f32,
    dst_h: f32,
) -> Option<Arc<RenderImage>> {
    let count = src.frame_count();
    if count == 0 {
        return None;
    }
    let mut frames = Vec::with_capacity(count);
    for i in 0..count {
        let bytes = src.as_bytes(i)?;
        let sz = src.size(i);
        let cropped = crop_bgra_frame(bytes, sz.width.0 as u32, sz.height.0 as u32, dst_w, dst_h)?;
        frames.push(Frame::from_parts(cropped, 0, 0, src.delay(i)));
    }
    Some(Arc::new(RenderImage::new(frames)))
}

pub struct RootView {
    pub model: Entity<Enki>,
    pub task_input: Entity<TextInput>,
    pub new_task_estimate: u32,
    pub editing_task: Option<String>,
    pub edit_task_input: Entity<TextInput>,
    pub intention_input: Entity<TextInput>,
    /// Which dropdown in the settings page is open.
    pub open_menu: Option<SharedString>,
    pub drag: WindowDrag,
    rounded_wallpaper: RoundedWallpaperCache,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl RootView {
    pub fn new(model: Entity<Enki>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let task_input = cx.new(|cx| TextInput::new("Adicionar tarefa…", cx));
        let edit_task_input = cx.new(|cx| TextInput::new("Título da tarefa…", cx));
        let intention_input = cx.new(|cx| TextInput::new("No que você vai focar?", cx));

        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |this, window, cx| {
                let appearance = window.appearance();
                this.model
                    .update(cx, |m, cx| m.set_system_appearance(appearance, cx));
            }),
            cx.subscribe(&task_input, |this, input, event, cx| {
                if let TextInputEvent::Submit = event {
                    let text = input.read(cx).text().to_string();
                    let estimate = this.new_task_estimate;
                    this.model
                        .update(cx, |m, cx| m.add_task_with_estimate(&text, estimate, cx));
                    input.update(cx, |input, cx| input.clear(cx));
                    if !text.trim().is_empty() {
                        this.new_task_estimate = 1;
                        cx.notify();
                    }
                }
            }),
            cx.subscribe(&edit_task_input, |this, input, event, cx| match event {
                TextInputEvent::Submit => {
                    if let Some(id) = this.editing_task.take() {
                        let title = input.read(cx).text().to_string();
                        this.model.update(cx, |m, cx| m.rename_task(&id, &title, cx));
                        cx.notify();
                    }
                }
                TextInputEvent::Cancel => {
                    if this.editing_task.take().is_some() {
                        cx.notify();
                    }
                }
                TextInputEvent::Changed => {}
            }),
            cx.subscribe(&intention_input, |this, input, event, cx| match event {
                TextInputEvent::Changed | TextInputEvent::Submit => {
                    let text = input.read(cx).text().to_string();
                    this.model.update(cx, |m, cx| m.set_intention(text, cx));
                }
                TextInputEvent::Cancel => {}
            }),
        ];

        Self {
            model,
            task_input,
            new_task_estimate: 1,
            editing_task: None,
            edit_task_input,
            intention_input,
            // Dev helper for screenshots of an open dropdown.
            open_menu: cfg!(debug_assertions)
                .then(|| std::env::var("ENKI_DEBUG_MENU").ok())
                .flatten()
                .map(SharedString::from),
            drag: WindowDrag::default(),
            rounded_wallpaper: RoundedWallpaperCache::default(),
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    pub fn focus_task_input(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.task_input.focus_handle(cx));
    }

    pub fn start_editing_task(
        &mut self,
        id: String,
        title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_task = Some(id);
        let title = title.to_string();
        self.edit_task_input
            .update(cx, |input, cx| input.set_text(title, cx));
        window.focus(&self.edit_task_input.focus_handle(cx));
        cx.notify();
    }

    fn titlebar(
        &self,
        title: AnyElement,
        sidebar_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let model = self.model.clone();
        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let lights = cfg!(target_os = "macos");

        let sidebar_btn = (!sidebar_open).then(|| {
            icon_button(
                "titlebar-sidebar",
                Icon::PanelLeft,
                &theme,
                window,
                cx,
                on_model(&model, |m, cx| {
                    m.update_settings(cx, |s| s.sidebar_open = !s.sidebar_open)
                }),
            )
        });
        let mini_btn = icon_button(
            "titlebar-mini",
            Icon::Mini,
            &theme,
            window,
            cx,
            |_, _, cx| crate::toggle_mini_window(cx),
        );

        div()
            .id("titlebar")
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(TITLEBAR_HEIGHT))
            .pr(px(12.))
            .pl(px(if !sidebar_open && lights {
                TRAFFIC_LIGHTS_WIDTH
            } else {
                20.
            }))
            .window_control_area(WindowControlArea::Drag)
            .when(client_decorated, |d| self.drag.attach(d))
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    // Only macOS has a system double-click action; elsewhere it maximizes.
                    if cfg!(target_os = "macos") {
                        window.titlebar_double_click();
                    } else {
                        window.zoom_window();
                    }
                }
            })
            .children(sidebar_btn)
            .child(title)
            .child(div().flex_1())
            .child(mini_btn)
            .when(client_decorated, |d| d.child(window_controls(&theme)))
    }

    fn page_title(&self, cx: &App) -> AnyElement {
        let theme = Theme::get(cx);
        let enki = self.model.read(cx);
        let (glyph, title): (Icon, SharedString) = match enki.page {
            Page::Timer => match enki.timer.phase() {
                Phase::Focus => (
                    Icon::Timer,
                    enki.current_task_title()
                        .map(|t| SharedString::from(t.to_string()))
                        .unwrap_or_else(|| "Sessão de foco".into()),
                ),
                Phase::ShortBreak => (Icon::Coffee, "Pausa curta".into()),
                Phase::LongBreak => (Icon::Coffee, "Pausa longa".into()),
            },
            Page::Tasks => (Icon::ListChecks, "Tarefas".into()),
            Page::Stats => (Icon::Chart, "Estatísticas".into()),
            Page::Wrapped => (Icon::Music, "Wrapped".into()),
            Page::Settings(section) => (
                Icon::Settings,
                format!("Configurações · {}", section.label()).into(),
            ),
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .min_w_0()
            .text_size(px(14.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.text)
            .child(icon(glyph).text_color(theme.accent))
            .child(div().ellipsis().child(title))
            .into_any_element()
    }

    /// The wallpaper drawn by GPUI. When a native backdrop is active (macOS
    /// Liquid Glass or native video player), the backdrop sits under GPUI
    /// instead, so this draws nothing.
    fn background(
        &mut self,
        corners: WindowCorners,
        inner_size: (f32, f32),
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = Theme::get(cx).clone();
        if theme.native_backdrop || self.model.read(cx).wallpaper.is_none() {
            self.rounded_wallpaper.clear(cx);
            return None;
        }

        let dim = self.model.read(cx).settings.background_dim as f32 / 100.0;

        let media: AnyElement = if corners.any() {
            let (wallpaper, video_frame) = {
                let enki = self.model.read(cx);
                (enki.wallpaper.clone()?, enki.video_frame.clone())
            };
            let cropped = {
                let mut cache = std::mem::take(&mut self.rounded_wallpaper);
                let res = cache.get_or_update(
                    &wallpaper,
                    video_frame.as_ref(),
                    inner_size.0,
                    inner_size.1,
                    cx,
                );
                self.rounded_wallpaper = cache;
                res
            };
            if let Some(cropped) = cropped {
                let el = img(ImageSource::Render(cropped.clone()))
                    .size_full()
                    .object_fit(ObjectFit::Fill)
                    .rounded_tl(corners.tl)
                    .rounded_tr(corners.tr)
                    .rounded_br(corners.br)
                    .rounded_bl(corners.bl);
                if cropped.frame_count() > 1 {
                    el.id(("wallpaper-anim", cropped.id.0)).into_any_element()
                } else {
                    el.into_any_element()
                }
            } else {
                img(ImageSource::from(wallpaper.preview))
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element()
            }
        } else {
            self.rounded_wallpaper.clear(cx);
            let enki = self.model.read(cx);
            let wallpaper = enki.wallpaper.as_ref()?;
            match &wallpaper.content {
                WallpaperContent::Static => img(ImageSource::from(wallpaper.preview.clone()))
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                WallpaperContent::Animated(render_img) => {
                    img(ImageSource::Render(render_img.clone()))
                        .id(("wallpaper-anim", render_img.id.0))
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                }
                WallpaperContent::Video { .. } => {
                    if let Some(frame) = &enki.video_frame {
                        img(ImageSource::Render(frame.clone()))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element()
                    } else {
                        img(ImageSource::from(wallpaper.preview.clone()))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element()
                    }
                }
            }
        };

        Some(
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .rounded_tl(corners.tl)
                .rounded_tr(corners.tr)
                .rounded_br(corners.br)
                .rounded_bl(corners.bl)
                .child(media)
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded_tl(corners.tl)
                        .rounded_tr(corners.tr)
                        .rounded_br(corners.br)
                        .rounded_bl(corners.bl)
                        .bg(with_alpha(theme.scrim, dim)),
                )
                .into_any_element(),
        )
    }

    /// Extra shade where text sits on the image: title bar and status row.
    fn shades(&self, corners: WindowCorners, cx: &App) -> Option<AnyElement> {
        let theme = Theme::get(cx);
        self.model.read(cx).wallpaper.as_ref()?;
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(120.))
                        .rounded_tl(corners.tl)
                        .rounded_tr(corners.tr)
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(with_alpha(theme.scrim, 0.35), 0.),
                            linear_color_stop(with_alpha(theme.scrim, 0.), 1.),
                        )),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(px(200.))
                        .rounded_bl(corners.bl)
                        .rounded_br(corners.br)
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(with_alpha(theme.scrim, 0.), 0.),
                            linear_color_stop(with_alpha(theme.scrim, 0.6), 1.),
                        )),
                )
                .into_any_element(),
        )
    }

    /// Floating error toast, visible even when the sidebar is collapsed.
    fn error_toast(&self, cx: &App) -> Option<AnyElement> {
        let message = self.model.read(cx).error.clone()?;
        let theme = Theme::get(cx);
        let model = self.model.clone();
        Some(
            div()
                .id("error-toast")
                .absolute()
                .bottom(px(16.))
                .right(px(16.))
                .max_w(px(420.))
                .rounded(radius::LG)
                .bg(theme.surface)
                .border_1()
                .border_color(with_alpha(theme.danger, 0.45))
                .shadow(theme.popover_shadow())
                .overflow_hidden()
                .cursor_pointer()
                .occlude()
                .on_click(on_model(&model, |m, cx| m.dismiss_error(cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(14.))
                        .py(px(10.))
                        .bg(with_alpha(theme.danger, 0.12))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.))
                                .text_color(theme.danger)
                                .child(message),
                        )
                        .child(icon_sized(Icon::X, px(14.), theme.danger)),
                )
                .into_any_element(),
        )
    }

    /// Starts the native glass / native video backdrop frame.
    fn begin_glass(&self, window: &Window, cx: &App) {
        let theme = Theme::get(cx);
        let enki = self.model.read(cx);
        let backdrop = theme.native_backdrop.then(|| {
            let media = enki.wallpaper.as_ref().map(|w| match &w.content {
                WallpaperContent::Static => native::BackdropMedia::Image(w.preview.as_path()),
                WallpaperContent::Animated(img) => native::BackdropMedia::Animated(img.clone()),
                WallpaperContent::Video {
                    path, blur_sigma, ..
                } => native::BackdropMedia::Video {
                    path: path.as_path(),
                    blur_sigma: *blur_sigma,
                },
            });
            native::Backdrop {
                color: theme.window_bg,
                media,
                dim: with_alpha(theme.scrim, enki.settings.background_dim as f32 / 100.0),
            }
        });
        native::glass_begin(window, theme.native_glass, backdrop);
    }
}

fn window_controls(theme: &Theme) -> impl IntoElement {
    let control = |id: &'static str, glyph: Icon, area: WindowControlArea, danger: bool| {
        let (hover, text, muted) = (
            if danger {
                theme.danger
            } else {
                theme.surface_hover
            },
            if danger { gpui::white() } else { theme.text },
            theme.text_muted,
        );
        div()
            .id(id)
            .group("window-control")
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded_full()
            .hover(move |s| s.bg(hover))
            .window_control_area(area)
            .child(
                icon(glyph)
                    .size(px(14.))
                    .text_color(muted)
                    .group_hover("window-control", move |s| s.text_color(text)),
            )
            .on_click(move |_, window, _| match area {
                WindowControlArea::Close => {
                    native::glass_cleanup(window);
                    window.remove_window();
                }
                WindowControlArea::Max => window.zoom_window(),
                WindowControlArea::Min => window.minimize_window(),
                WindowControlArea::Drag => {}
            })
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .ml(px(6.))
        .child(control(
            "win-min",
            Icon::WinMin,
            WindowControlArea::Min,
            false,
        ))
        .child(control(
            "win-max",
            Icon::WinMax,
            WindowControlArea::Max,
            false,
        ))
        .child(control(
            "win-close",
            Icon::WinClose,
            WindowControlArea::Close,
            true,
        ))
}

/// Invisible strips along untiled edges (covering the shadow inset + a few
/// pixels inside the frame) that start an interactive resize on Linux CSD.
fn resize_handles(tiling: Tiling) -> impl IntoElement {
    let edge = f32::from(WINDOW_SHADOW_INSET) + 4.;
    let corner = edge + 6.;
    let handle = |id: &'static str, edge: ResizeEdge| {
        div()
            .id(id)
            .absolute()
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                window.start_window_resize(edge);
            })
    };
    div()
        .absolute()
        .inset_0()
        .when(!tiling.top, |d| {
            d.child(
                handle("rz-t", ResizeEdge::Top)
                    .top_0()
                    .left(px(corner))
                    .right(px(corner))
                    .h(px(edge))
                    .cursor_ns_resize(),
            )
        })
        .when(!tiling.bottom, |d| {
            d.child(
                handle("rz-b", ResizeEdge::Bottom)
                    .bottom_0()
                    .left(px(corner))
                    .right(px(corner))
                    .h(px(edge))
                    .cursor_ns_resize(),
            )
        })
        .when(!tiling.left, |d| {
            d.child(
                handle("rz-l", ResizeEdge::Left)
                    .left_0()
                    .top(px(corner))
                    .bottom(px(corner))
                    .w(px(edge))
                    .cursor_ew_resize(),
            )
        })
        .when(!tiling.right, |d| {
            d.child(
                handle("rz-r", ResizeEdge::Right)
                    .right_0()
                    .top(px(corner))
                    .bottom(px(corner))
                    .w(px(edge))
                    .cursor_ew_resize(),
            )
        })
        .when(!(tiling.top || tiling.left), |d| {
            d.child(
                handle("rz-tl", ResizeEdge::TopLeft)
                    .top_0()
                    .left_0()
                    .size(px(corner))
                    .cursor_nwse_resize(),
            )
        })
        .when(!(tiling.top || tiling.right), |d| {
            d.child(
                handle("rz-tr", ResizeEdge::TopRight)
                    .top_0()
                    .right_0()
                    .size(px(corner))
                    .cursor_nesw_resize(),
            )
        })
        .when(!(tiling.bottom || tiling.left), |d| {
            d.child(
                handle("rz-bl", ResizeEdge::BottomLeft)
                    .bottom_0()
                    .left_0()
                    .size(px(corner))
                    .cursor_nesw_resize(),
            )
        })
        .when(!(tiling.bottom || tiling.right), |d| {
            d.child(
                handle("rz-br", ResizeEdge::BottomRight)
                    .bottom_0()
                    .right_0()
                    .size(px(corner))
                    .cursor_nwse_resize(),
            )
        })
}

impl Focusable for RootView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (page, sidebar_open) = {
            let enki = self.model.read(cx);
            (enki.page, enki.settings.sidebar_open)
        };
        self.begin_glass(window, cx);

        let client_tiling = match window.window_decorations() {
            Decorations::Client { tiling } => Some(tiling),
            Decorations::Server => None,
        };
        if client_tiling.is_some() {
            window.set_client_inset(WINDOW_SHADOW_INSET);
        }
        let corners = client_tiling
            .map(WindowCorners::from_tiling)
            .unwrap_or_default();

        let mut inner = window.viewport_size();
        if let Some(tiling) = client_tiling {
            if !tiling.left {
                inner.width -= WINDOW_SHADOW_INSET;
            }
            if !tiling.right {
                inner.width -= WINDOW_SHADOW_INSET;
            }
            if !tiling.top {
                inner.height -= WINDOW_SHADOW_INSET;
            }
            if !tiling.bottom {
                inner.height -= WINDOW_SHADOW_INSET;
            }
        }
        let inner_size = (f32::from(inner.width).max(1.), f32::from(inner.height).max(1.));

        let title = self.page_title(cx);
        let background = self.background(corners, inner_size, cx);
        let shades = self.shades(corners, cx);
        let titlebar = self
            .titlebar(title, sidebar_open, window, cx)
            .into_any_element();
        let body: AnyElement = match page {
            Page::Timer => timer_page::render(self, window, cx),
            Page::Tasks => tasks_page::render(self, window, cx),
            Page::Stats => stats_page::render(self, window, cx),
            Page::Wrapped => wrapped_page::render(self, window, cx),
            Page::Settings(section) => settings_page::render(self, section, window, cx),
        };
        let sidebar =
            sidebar_open.then(|| sidebar::render(self, corners.tl, corners.bl, window, cx));
        let error_toast = self.error_toast(cx);

        let theme = Theme::get(cx).clone();
        let liquid = theme.liquid();

        let has_untiled_edge = |t: Tiling| !(t.top && t.bottom && t.left && t.right);
        let window_outline = client_tiling.and_then(|tiling| {
            has_untiled_edge(tiling).then(|| {
                let rim = if theme.is_dark {
                    with_alpha(gpui::white(), 0.12)
                } else {
                    with_alpha(gpui::black(), 0.14)
                };
                div()
                    .absolute()
                    .inset_0()
                    .rounded_tl(corners.tl)
                    .rounded_tr(corners.tr)
                    .rounded_br(corners.br)
                    .rounded_bl(corners.bl)
                    .when(!tiling.top, |d| d.border_t_1())
                    .when(!tiling.bottom, |d| d.border_b_1())
                    .when(!tiling.left, |d| d.border_l_1())
                    .when(!tiling.right, |d| d.border_r_1())
                    .border_color(rim)
            })
        });

        let root = div()
            .id("root")
            .key_context("Enki")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &actions::NewTask, window, cx| {
                this.model.update(cx, |m, cx| m.navigate(Page::Tasks, cx));
                this.focus_task_input(window, cx);
            }))
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .font_family(UI_FONT)
            .text_size(px(14.))
            .text_color(theme.text)
            .when(!theme.native_backdrop, |d| d.bg(theme.window_bg))
            .children(background)
            // Over the whole window, or the floating sidebar's gaps show a seam.
            .children(shades)
            .children(sidebar)
            .child(
                div()
                    .id("main")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(sidebar_open && !liquid, |d| {
                        d.border_l_1().border_color(theme.border)
                    })
                    .child(titlebar)
                    .child(div().flex().flex_col().flex_1().min_h_0().child(body)),
            )
            .children(error_toast)
            .children(window_outline)
            // Painted last: hides glass views nothing placed this frame.
            .child(
                canvas(|_, _, _| {}, |_, _, window, _| native::glass_end(window))
                    .absolute()
                    .size_0(),
            )
            .when(corners.any(), |d| {
                d.rounded_tl(corners.tl)
                    .rounded_tr(corners.tr)
                    .rounded_br(corners.br)
                    .rounded_bl(corners.bl)
                    .overflow_hidden()
            })
            .when(
                client_tiling.is_some_and(|t| !t.is_tiled()),
                |d| {
                    d.shadow(vec![BoxShadow {
                        color: with_alpha(
                            gpui::black(),
                            if theme.is_dark { 0.45 } else { 0.22 },
                        ),
                        offset: point(px(0.), px(2.)),
                        blur_radius: px(10.),
                        spread_radius: px(-1.),
                    }])
                },
            );

        if let Some(tiling) = client_tiling {
            div()
                .relative()
                .size_full()
                .bg(gpui::transparent_black())
                .when(!tiling.top, |d| d.pt(WINDOW_SHADOW_INSET))
                .when(!tiling.bottom, |d| d.pb(WINDOW_SHADOW_INSET))
                .when(!tiling.left, |d| d.pl(WINDOW_SHADOW_INSET))
                .when(!tiling.right, |d| d.pr(WINDOW_SHADOW_INSET))
                .child(root)
                .when(has_untiled_edge(tiling), |d| {
                    d.child(resize_handles(tiling))
                })
                .into_any_element()
        } else {
            root.into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_rect_for_aspect_centers_wide_and_tall_sources() {
        // 16:9 image into 3:2 window -> trims left & right equally.
        assert_eq!(crop_rect_for_aspect(1920, 1080, 1080., 720.), (150, 0, 1620, 1080));
        // 1:1 image into 3:2 window -> trims top & bottom equally.
        assert_eq!(crop_rect_for_aspect(1200, 1200, 1080., 720.), (0, 200, 1200, 800));
        // Already matching aspect -> no crop.
        assert_eq!(crop_rect_for_aspect(1080, 720, 1080., 720.), (0, 0, 1080, 720));
    }

    #[test]
    fn crop_bgra_frame_extracts_centered_pixels() {
        // 4x2 image: each pixel encodes its (x, y) coordinates.
        let mut src = Vec::with_capacity(4 * 2 * 4);
        for y in 0..2u8 {
            for x in 0..4u8 {
                src.extend_from_slice(&[x, y, 0, 255]);
            }
        }
        // Target 1:1 -> center 2x2 slice (x = 1..3, y = 0..2).
        let cropped = crop_bgra_frame(&src, 4, 2, 100., 100.).expect("cropped");
        assert_eq!(cropped.dimensions(), (2, 2));
        assert_eq!(cropped.get_pixel(0, 0).0, [1, 0, 0, 255]);
        assert_eq!(cropped.get_pixel(1, 1).0, [2, 1, 0, 255]);
    }
}
