//! Settings, laid out like Zeron's: section title, labeled cards of rows.

use gpui::{
    anchored, deferred, div, img, prelude::*, px, AnyElement, App, Context, Corner, Entity,
    FontWeight, Hsla, ImageSource, ObjectFit, PathPromptOptions, SharedString, Window,
};

use crate::{
    assets::Icon,
    notify,
    settings::{Accent, BackgroundEffect, ColorScheme, Glass, Settings, Sound},
    state::{Enki, SettingsSection},
    theme::{radius, with_alpha, Theme},
    ui::{
        components::StyledExt as _,
        components::{
            button, card, icon, icon_sized, keycap, page_title, section_label, setting_row, slider,
            stepper, toggle, ButtonKind,
        },
        root::RootView,
    },
};

/// Click handler that edits settings.
fn set(
    model: &Entity<Enki>,
    f: impl Fn(&mut Settings) + 'static,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    let model = model.clone();
    move |_, _, cx| model.update(cx, |m, cx| m.update_settings(cx, |s| f(s)))
}

/// Fixed positions for a settings slider.
struct Stops {
    id: &'static str,
    values: &'static [u32],
    unit: &'static str,
}

const FOCUS_STOPS: Stops = Stops {
    id: "focus-min",
    values: &[5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 60, 75, 90],
    unit: " min",
};
const SHORT_STOPS: Stops = Stops {
    id: "short-min",
    values: &[1, 2, 3, 4, 5, 7, 10, 15],
    unit: " min",
};
const LONG_STOPS: Stops = Stops {
    id: "long-min",
    values: &[5, 10, 15, 20, 25, 30, 45, 60],
    unit: " min",
};
const EVERY_STOPS: Stops = Stops {
    id: "every",
    values: &[2, 3, 4, 5, 6, 7, 8],
    unit: " focos",
};
const DIM_STOPS: Stops = Stops {
    id: "dim",
    values: &[0, 10, 20, 30, 40, 50, 60, 70, 80, 90],
    unit: "%",
};

/// A detent slider with its value on the left. A value saved off the grid
/// (older settings) shows at the nearest stop until the slider moves.
fn stop_slider(
    stops: &'static Stops,
    current: u32,
    apply: fn(&mut Settings, u32),
    model: &Entity<Enki>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = Theme::get(cx).clone();
    let ix = stops
        .values
        .iter()
        .enumerate()
        .min_by_key(|(_, v)| v.abs_diff(current))
        .map_or(0, |(ix, _)| ix);
    let model = model.clone();
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .child(
            div()
                .min_w(px(64.))
                .text_right()
                .text_size(px(13.))
                .font(crate::theme::tabular_font())
                .text_color(theme.text)
                .child(format!("{current}{}", stops.unit)),
        )
        .child(slider(
            stops.id,
            stops.values.len(),
            ix,
            &theme,
            window,
            cx,
            move |ix, cx| {
                let value = stops.values[ix];
                model.update(cx, |m, cx| m.update_settings(cx, |s| apply(s, value)));
            },
        ))
        .into_any_element()
}

pub fn render(
    view: &mut RootView,
    section: SettingsSection,
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let content: Vec<AnyElement> = match section {
        SettingsSection::General => general(view, window, cx),
        SettingsSection::Appearance => appearance(view, window, cx),
        SettingsSection::Timer => timer(view, window, cx),
        SettingsSection::Notifications => notifications(view, window, cx),
        SettingsSection::Shortcuts => shortcuts(cx),
    };
    let theme = Theme::get(cx);
    div()
        .id("settings-page")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .w_full()
                .max_w(px(760.))
                .mx_auto()
                .px(px(32.))
                .pt(px(24.))
                .pb(px(64.))
                .child(page_title(theme, section.label()).pb(px(12.)))
                .children(content),
        )
        .into_any_element()
}

fn general(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> Vec<AnyElement> {
    let model = view.model.clone();
    let theme = Theme::get(cx).clone();
    let theme = &theme;
    let s = model.read(cx).settings.clone();
    let goal = s.daily_goal;
    let clear_music_btn = button(
        "clear-music-history",
        "Limpar",
        ButtonKind::Ghost,
        theme,
        window,
        cx,
        {
            let model = model.clone();
            move |_, _, cx| model.update(cx, |m, cx| m.clear_music_history(cx))
        },
    );
    vec![
        card("settings-card-1", theme)
            .child(setting_row(
                theme,
                true,
                if cfg!(target_os = "macos") {
                    "Mostrar na barra de menus"
                } else {
                    "Mostrar na bandeja do sistema"
                },
                Some(
                    "Tempo restante sempre visível, com atalhos para iniciar, pausar e pular."
                        .into(),
                ),
                toggle(
                    "set-menubar",
                    s.menu_bar,
                    theme,
                    set(&model, |s| s.menu_bar = !s.menu_bar),
                ),
            ))
            .child(setting_row(
                theme,
                false,
                "Janela mini sempre por cima",
                Some("O modo mini flutua sobre as outras janelas.".into()),
                toggle(
                    "set-mini-top",
                    s.mini_on_top,
                    theme,
                    set(&model, |s| s.mini_on_top = !s.mini_on_top),
                ),
            ))
            .child(setting_row(
                theme,
                false,
                "Música tocando agora",
                Some(if cfg!(target_os = "macos") {
                    "Mostra e controla Spotify e Apple Music. Não precisa de login.".into()
                } else {
                    "Mostra e controla o player ativo (Spotify ou outro via MPRIS).".into()
                }),
                toggle(
                    "set-np",
                    s.now_playing,
                    theme,
                    set(&model, |s| s.now_playing = !s.now_playing),
                ),
            ))
            .child(setting_row(
                theme,
                false,
                "Histórico de música (Wrapped)",
                Some("Registra faixas tocadas para montar seu Wrapped na barra lateral.".into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(clear_music_btn)
                    .child(toggle(
                        "set-track-music",
                        s.track_music,
                        theme,
                        set(&model, |s| s.track_music = !s.track_music),
                    )),
            ))
            .child(setting_row(
                theme,
                false,
                "Meta diária",
                Some("Quantos pomodoros você quer completar por dia.".into()),
                stepper(
                    "goal",
                    format!("{goal}"),
                    theme,
                    set(&model, |s| {
                        s.daily_goal = s.daily_goal.saturating_sub(1).max(1)
                    }),
                    set(&model, |s| s.daily_goal = (s.daily_goal + 1).min(24)),
                ),
            ))
            .into_any_element(),
        section_label(theme, "Sobre").pt(px(20.)).into_any_element(),
        card("settings-card-2", theme)
            .child(setting_row(
                theme,
                true,
                format!("Enki {}", env!("CARGO_PKG_VERSION")),
                Some(format!("Histórico salvo em {}", crate::paths::data_dir().display()).into()),
                div(),
            ))
            .into_any_element(),
    ]
}

fn scheme_tile(
    id: &'static str,
    scheme: ColorScheme,
    selected: bool,
    theme: &Theme,
    model: &Entity<Enki>,
) -> AnyElement {
    let (label, glyph) = match scheme {
        ColorScheme::System => ("Sistema", Icon::Monitor),
        ColorScheme::Light => ("Claro", Icon::Sun),
        ColorScheme::Dark => ("Escuro", Icon::Moon),
    };
    // A miniature window: sidebar lines + content lines.
    let mock = |bg: u32, side: u32, line: u32, panel: u32| {
        let line_color: Hsla = gpui::rgb(line).into();
        div()
            .flex()
            .flex_1()
            .h_full()
            .bg(gpui::rgb(bg))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .w(px(40.))
                    .p(px(9.))
                    .bg(gpui::rgb(side))
                    .children((0..4).map(move |i| {
                        div()
                            .h(px(3.))
                            .w(px(if i == 0 { 16. } else { 22. }))
                            .rounded_full()
                            .bg(line_color)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap(px(6.))
                    .m(px(8.))
                    .p(px(9.))
                    .rounded(px(5.))
                    .bg(gpui::rgb(panel))
                    .children([0.6, 0.9, 0.75, 0.5].into_iter().map(move |w| {
                        div()
                            .h(px(3.))
                            .w(gpui::relative(w))
                            .rounded_full()
                            .bg(line_color)
                    })),
            )
    };
    let light = || mock(0xf3f4f6, 0xf3f4f6, 0xc9ced4, 0xffffff);
    let dark = || mock(0x0d0f12, 0x0d0f12, 0x3a4048, 0x050607);
    let preview = div()
        .flex()
        .h(px(104.))
        .w_full()
        .rounded(radius::MD)
        .overflow_hidden()
        .border_2()
        .border_color(if selected { theme.accent } else { theme.border })
        .map(|d| match scheme {
            ColorScheme::System => d.child(light()).child(dark()),
            ColorScheme::Light => d.child(light()),
            ColorScheme::Dark => d.child(dark()),
        });
    div()
        .id(id)
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.))
        .flex_1()
        .cursor_pointer()
        .child(preview)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(14.))
                .text_color(if selected {
                    theme.accent_text
                } else {
                    theme.text_muted
                })
                .child(icon(glyph).size(px(15.)).text_color(if selected {
                    theme.accent_text
                } else {
                    theme.text_muted
                }))
                .child(label),
        )
        .on_click(set(model, move |s| s.color_scheme = scheme))
        .into_any_element()
}

/// A Zeron-style dropdown: a pill button that opens a small menu below it.
fn dropdown<T: Copy + PartialEq + 'static>(
    id: &'static str,
    view: &RootView,
    cx: &mut Context<RootView>,
    current: T,
    options: &[(T, &'static str)],
    on_select: impl Fn(T, &mut App) + Clone + 'static,
) -> AnyElement {
    let open = view.open_menu.as_ref().map(|m| m.as_ref()) == Some(id);
    let theme = Theme::get(cx).clone();
    let current_label = options
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, l)| *l)
        .unwrap_or("");

    let trigger = div()
        .id(SharedString::from(format!("{id}-trigger")))
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(34.))
        .pl(px(14.))
        .pr(px(10.))
        .rounded(radius::MD)
        .bg(theme.control)
        .border_1()
        .border_color(if open {
            theme.accent
        } else {
            theme.border_strong
        })
        .cursor_pointer()
        .text_size(px(13.5))
        .text_color(theme.text)
        .hover(|s| s.bg(theme.control_hover))
        .child(current_label)
        .child(icon_sized(Icon::ChevronDown, px(14.), theme.text_muted))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.open_menu = if this.open_menu.as_ref().map(|m| m.as_ref()) == Some(id) {
                None
            } else {
                Some(id.into())
            };
            cx.notify();
        }));

    let menu = open.then(|| {
        let items = options.iter().map(|(value, label)| {
            let value = *value;
            let selected = value == current;
            let on_select = on_select.clone();
            let hover = theme.surface_hover;
            div()
                .id(SharedString::from(format!("{id}-{label}")))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .h(px(32.))
                .px(px(10.))
                .rounded(radius::SM)
                .cursor_pointer()
                .text_size(px(13.5))
                .text_color(theme.text)
                .hover(move |s| s.bg(hover))
                .child(*label)
                .when(selected, |d| {
                    d.child(icon_sized(Icon::Check, px(14.), theme.accent_text))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_menu = None;
                    on_select(value, cx);
                    cx.notify();
                }))
        });
        div().absolute().top(px(40.)).right_0().child(
            deferred(
                anchored().anchor(Corner::TopRight).snap_to_window().child(
                    div()
                        .id(SharedString::from(format!("{id}-menu")))
                        .occlude()
                        .flex()
                        .flex_col()
                        .min_w(px(200.))
                        .p(px(4.))
                        .rounded(radius::MD)
                        .bg(theme.control)
                        .border_1()
                        .border_color(theme.border_strong)
                        .shadow(theme.popover_shadow())
                        .children(items)
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.open_menu = None;
                            cx.notify();
                        })),
                ),
            )
            .with_priority(1),
        )
    });

    div()
        .relative()
        .child(trigger)
        .children(menu)
        .into_any_element()
}

fn appearance(
    view: &mut RootView,
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> Vec<AnyElement> {
    let model = view.model.clone();
    let enki = model.read(cx);
    let s = enki.settings.clone();
    let wallpaper = enki.wallpaper.clone();
    let theme = Theme::get(cx).clone();
    let theme = &theme;

    let schemes = div()
        .flex()
        .gap(px(16.))
        .pb(px(12.))
        .child(scheme_tile(
            "scheme-system",
            ColorScheme::System,
            s.color_scheme == ColorScheme::System,
            theme,
            &model,
        ))
        .child(scheme_tile(
            "scheme-light",
            ColorScheme::Light,
            s.color_scheme == ColorScheme::Light,
            theme,
            &model,
        ))
        .child(scheme_tile(
            "scheme-dark",
            ColorScheme::Dark,
            s.color_scheme == ColorScheme::Dark,
            theme,
            &model,
        ));

    let using_wallpaper = s.match_wallpaper && wallpaper.is_some();
    let swatch = |id: SharedString, color: Hsla, selected: bool| {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(30.))
            .rounded(radius::MD)
            .border_2()
            .border_color(if selected {
                theme.text
            } else {
                with_alpha(theme.text, 0.0)
            })
            .cursor_pointer()
            .child(div().size(px(22.)).rounded(px(6.)).bg(color))
    };
    let mut swatches = div().flex().items_center().gap(px(4.));
    if let Some(wp) = &wallpaper {
        let tint: Hsla = wp.tint.into();
        swatches = swatches.child(
            swatch("accent-wallpaper".into(), tint, using_wallpaper)
                .child(
                    div()
                        .absolute()
                        .child(icon_sized(Icon::Image, px(12.), gpui::white())),
                )
                .on_click(set(&model, |s| s.match_wallpaper = true)),
        );
    }
    for accent in Accent::ALL {
        let selected = !using_wallpaper && s.accent == accent;
        swatches = swatches.child(
            swatch(
                format!("accent-{}", accent.label()).into(),
                gpui::rgb(accent.rgb()).into(),
                selected,
            )
            .on_click(set(&model, move |s| {
                s.accent = accent;
                s.match_wallpaper = false;
            })),
        );
    }

    let colors = card("settings-card-3", theme)
        .child(setting_row(
            theme,
            true,
            "Cor de destaque",
            Some(if using_wallpaper {
                "Usando a cor do papel de parede; escolha outra para desligar.".into()
            } else {
                "Usada no anel do timer, seleção e botões.".into()
            }),
            swatches,
        ))
        .child(setting_row(
            theme,
            false,
            "Usar cores do papel de parede",
            Some("Puxa destaque e um leve tom das superfícies da imagem de fundo.".into()),
            toggle(
                "match-wallpaper",
                s.match_wallpaper,
                theme,
                set(&model, |s| s.match_wallpaper = !s.match_wallpaper),
            ),
        ));

    let glass_model = model.clone();
    let glass = dropdown(
        "glass",
        view,
        cx,
        s.glass,
        &Glass::ALL.map(|g| (g, g.label())),
        move |g, cx| glass_model.update(cx, |m, cx| m.update_settings(cx, |s| s.glass = g)),
    );
    let effect_model = model.clone();
    let effect = dropdown(
        "effect",
        view,
        cx,
        s.background_effect,
        &BackgroundEffect::ALL.map(|e| (e, e.label())),
        move |e, cx| {
            effect_model.update(cx, |m, cx| {
                m.update_settings(cx, |s| s.background_effect = e)
            })
        },
    );

    let pick_image = {
        let model = model.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            let paths = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Usar imagem".into()),
            });
            let model = model.clone();
            cx.spawn(async move |cx| {
                if let Ok(Ok(Some(paths))) = paths.await {
                    if let Some(path) = paths.into_iter().next() {
                        model
                            .update(cx, |m, cx| {
                                m.update_settings(cx, |s| s.background_image = Some(path))
                            })
                            .ok();
                    }
                }
            })
            .detach();
        }
    };

    let image_name = s
        .background_image
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());
    let thumb: AnyElement = match &wallpaper {
        Some(wp) => img(ImageSource::from(wp.rendered.clone()))
            .size(px(40.))
            .rounded(radius::MD)
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        None => div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(40.))
            .rounded(radius::MD)
            .bg(theme.surface_active)
            .child(icon_sized(Icon::Image, px(18.), theme.text_muted))
            .into_any_element(),
    };
    let theme_ref = theme;
    let pick_btn = button(
        "pick-image",
        if image_name.is_some() {
            "Substituir imagem"
        } else {
            "Escolher imagem"
        },
        ButtonKind::Secondary,
        theme_ref,
        window,
        cx,
        pick_image,
    );
    let remove_btn = image_name.is_some().then(|| {
        button(
            "remove-image",
            "Remover",
            ButtonKind::Danger,
            theme_ref,
            window,
            cx,
            set(&model, |s| s.background_image = None),
        )
    });
    let image_row = div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(16.))
        .py(px(12.))
        .border_t_1()
        .border_color(theme_ref.border)
        .child(thumb)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Imagem de fundo"),
                )
                .child(
                    div()
                        .ellipsis()
                        .text_size(px(12.5))
                        .text_color(theme_ref.text_muted)
                        .child(
                            image_name
                                .clone()
                                .unwrap_or_else(|| "Nenhuma imagem. Fica atrás do timer.".into()),
                        ),
                ),
        )
        .child(pick_btn)
        .children(remove_btn);

    let dim = stop_slider(
        &DIM_STOPS,
        s.background_dim as u32,
        |s, v| s.background_dim = v as u8,
        &model,
        window,
        cx,
    );
    let material = card("settings-card-4", theme)
        .child(setting_row(
            theme,
            true,
            "Vidro",
            Some(match s.glass {
                Glass::Liquid if crate::native::glass_supported() => {
                    "Barra lateral e controles de vidro do sistema, que refratam o fundo.".into()
                }
                Glass::Liquid => "Barra lateral flutuante e controles com bordas de vidro.".into(),
                Glass::Translucent => {
                    "Com imagem de fundo, a barra lateral fica translúcida sobre ela.".into()
                }
                Glass::Solid => "Superfícies opacas.".into(),
            }),
            glass,
        ))
        .child(image_row)
        .child(setting_row(
            theme,
            false,
            "Efeito de fundo",
            Some(match s.background_effect {
                BackgroundEffect::None => "Mostra a imagem original.".into(),
                _ => "Desfoca a imagem para o timer se destacar.".into(),
            }),
            effect,
        ))
        .child(setting_row(
            theme,
            false,
            "Escurecer fundo",
            Some("Quanto a imagem é coberta pela cor do tema.".into()),
            dim,
        ));

    let motion = card("settings-card-5", theme).child(setting_row(
        theme,
        true,
        "Reduzir animações",
        Some("Desliga transições entre fases.".into()),
        toggle(
            "reduce-motion",
            s.reduce_motion,
            theme,
            set(&model, |s| s.reduce_motion = !s.reduce_motion),
        ),
    ));

    vec![
        section_label(theme, "Esquema de cores").into_any_element(),
        schemes.into_any_element(),
        colors.into_any_element(),
        section_label(theme, "Material e fundo")
            .pt(px(20.))
            .into_any_element(),
        material.into_any_element(),
        section_label(theme, "Movimento")
            .pt(px(20.))
            .into_any_element(),
        motion.into_any_element(),
    ]
}

fn timer(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> Vec<AnyElement> {
    let model = view.model.clone();
    let s = model.read(cx).settings.clone();
    let focus = stop_slider(
        &FOCUS_STOPS,
        s.focus_minutes,
        |s, v| s.focus_minutes = v,
        &model,
        window,
        cx,
    );
    let short = stop_slider(
        &SHORT_STOPS,
        s.short_break_minutes,
        |s, v| s.short_break_minutes = v,
        &model,
        window,
        cx,
    );
    let long = stop_slider(
        &LONG_STOPS,
        s.long_break_minutes,
        |s, v| s.long_break_minutes = v,
        &model,
        window,
        cx,
    );
    let every = stop_slider(
        &EVERY_STOPS,
        s.long_break_every,
        |s, v| s.long_break_every = v,
        &model,
        window,
        cx,
    );
    let theme = Theme::get(cx);
    let presets = [
        ("preset-classic", "Clássico", 25, 5, 15),
        ("preset-deep", "Profundo", 50, 10, 20),
        ("preset-short", "Curto", 15, 3, 10),
    ];
    let preset_row = div().flex().gap(px(12.)).pb(px(12.)).children(
        presets.into_iter().map(|(id, name, f, sb, lb)| {
            let active =
                s.focus_minutes == f && s.short_break_minutes == sb && s.long_break_minutes == lb;
            let model = model.clone();
            let hover = theme.surface_hover;
            div()
                .id(id)
                .flex()
                .flex_col()
                .flex_1()
                .gap(px(4.))
                .px(px(14.))
                .py(px(12.))
                .rounded(radius::LG)
                .bg(if active {
                    theme.accent_soft
                } else {
                    theme.surface
                })
                .border_1()
                .border_color(if active { theme.accent } else { theme.border })
                .cursor_pointer()
                .when(!active, |d| d.hover(move |s| s.bg(hover)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if active {
                                    theme.accent_text
                                } else {
                                    theme.text
                                })
                                .child(name),
                        )
                        .when(active, |d| {
                            d.child(icon_sized(Icon::Check, px(14.), theme.accent_text))
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .font(crate::theme::tabular_font())
                        .text_color(if active {
                            theme.accent_text
                        } else {
                            theme.text_muted
                        })
                        .child(format!("{f}/{sb}/{lb} min")),
                )
                .on_click(move |_, _, cx| {
                    model.update(cx, |m, cx| m.apply_preset(f, sb, lb, cx));
                })
        }),
    );

    vec![
        section_label(theme, "Presets").into_any_element(),
        preset_row.into_any_element(),
        section_label(theme, "Durações")
            .pt(px(8.))
            .into_any_element(),
        card("settings-card-timer", theme)
            .child(setting_row(theme, true, "Foco", None, focus))
            .child(setting_row(theme, false, "Pausa curta", None, short))
            .child(setting_row(theme, false, "Pausa longa", None, long))
            .child(setting_row(
                theme,
                false,
                "Pausa longa a cada",
                Some("Número de focos completos em um ciclo.".into()),
                every,
            ))
            .into_any_element(),
        section_label(theme, "Automação")
            .pt(px(20.))
            .into_any_element(),
        card("settings-card-7", theme)
            .child(setting_row(
                theme,
                true,
                "Iniciar pausas automaticamente",
                Some("A pausa começa assim que o foco termina.".into()),
                toggle(
                    "auto-breaks",
                    s.auto_start_breaks,
                    theme,
                    set(&model, |s| s.auto_start_breaks = !s.auto_start_breaks),
                ),
            ))
            .child(setting_row(
                theme,
                false,
                "Iniciar foco automaticamente",
                Some("O próximo foco começa quando a pausa acaba.".into()),
                toggle(
                    "auto-focus",
                    s.auto_start_focus,
                    theme,
                    set(&model, |s| s.auto_start_focus = !s.auto_start_focus),
                ),
            ))
            .child(setting_row(
                theme,
                false,
                "Pausar música nas pausas",
                Some("Pausa a reprodução ao entrar na pausa e retoma no próximo foco.".into()),
                toggle(
                    "auto-pause-media",
                    s.auto_pause_media,
                    theme,
                    set(&model, |s| s.auto_pause_media = !s.auto_pause_media),
                ),
            ))
            .into_any_element(),
    ]
}

fn notifications(
    view: &mut RootView,
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> Vec<AnyElement> {
    let model = view.model.clone();
    let s = model.read(cx).settings.clone();
    let sound_model = model.clone();
    let sound = dropdown(
        "sound",
        view,
        cx,
        s.sound,
        &Sound::ALL.map(|v| (v, v.label())),
        move |v, cx| {
            notify::play(v);
            sound_model.update(cx, |m, cx| m.update_settings(cx, |s| s.sound = v));
        },
    );
    let theme = Theme::get(cx).clone();
    let theme = &theme;
    let test_btn = button(
        "test-notification",
        "Enviar teste",
        ButtonKind::Secondary,
        theme,
        window,
        cx,
        move |_, _, _| {
            notify::play(s.sound);
            notify::notify("Enki", "As notificações estão funcionando.");
        },
    );
    vec![card("settings-card-8", theme)
        .child(setting_row(
            theme,
            true,
            "Notificações",
            Some("Avisa quando cada fase termina.".into()),
            toggle(
                "notifications",
                s.notifications,
                theme,
                set(&model, |s| s.notifications = !s.notifications),
            ),
        ))
        .child(setting_row(
            theme,
            false,
            "Som",
            Some("Toca ao fim de cada fase.".into()),
            sound,
        ))
        .child(setting_row(
            theme,
            false,
            "Testar",
            Some("Envia uma notificação de exemplo agora.".into()),
            test_btn,
        ))
        .into_any_element()]
}

fn shortcuts(cx: &mut Context<RootView>) -> Vec<AnyElement> {
    let theme = Theme::get(cx);
    let cmd = if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl"
    };
    let rows: Vec<(&str, Vec<String>)> = vec![
        ("Iniciar / pausar", vec!["Espaço".into()]),
        ("Pular fase", vec!["S".into()]),
        ("Reiniciar fase", vec!["R".into()]),
        ("Timer", vec![cmd.into(), "1".into()]),
        ("Tarefas", vec![cmd.into(), "2".into()]),
        ("Estatísticas", vec![cmd.into(), "3".into()]),
        ("Wrapped", vec![cmd.into(), "4".into()]),
        ("Nova tarefa", vec![cmd.into(), "N".into()]),
        (
            "Mostrar/ocultar barra lateral",
            vec![cmd.into(), "B".into()],
        ),
        ("Modo mini", vec![cmd.into(), "⇧".into(), "M".into()]),
        ("Configurações", vec![cmd.into(), ",".into()]),
    ];
    vec![card("settings-card-9", theme)
        .children(rows.into_iter().enumerate().map(|(ix, (label, keys))| {
            setting_row(
                theme,
                ix == 0,
                label,
                None,
                div()
                    .flex()
                    .gap(px(4.))
                    .children(keys.into_iter().map(|k| keycap(theme, k))),
            )
        }))
        .into_any_element()]
}
