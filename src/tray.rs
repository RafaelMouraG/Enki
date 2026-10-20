//! Menu bar (macOS) / system tray (Linux) item showing the remaining time.
//!
//! macOS uses an NSStatusItem through `tray-icon`. Linux uses the
//! StatusNotifierItem D-Bus protocol through `ksni`, which needs no GTK and is
//! shown by Ubuntu's built-in AppIndicator extension.

use crate::timer::Phase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    Toggle,
    Skip,
    Reset,
    Show,
    Mini,
    Quit,
}

/// A small timer glyph, `size`×`size`, as straight RGBA with the given color.
fn icon_rgba(size: u32, color: [u8; 3]) -> Vec<u8> {
    let s = size as f32;
    let center = (s / 2.0, s * 0.56);
    let radius = s * 0.36;
    let stroke = s * 0.095;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (dx, dy) = (px - center.0, py - center.1);
            let dist = (dx * dx + dy * dy).sqrt();
            // Ring.
            let ring = 1.0 - ((dist - radius).abs() - stroke / 2.0).clamp(0.0, 1.0);
            // Hand pointing up-right from the center.
            let hand = segment_coverage(
                (px, py),
                center,
                (center.0 + radius * 0.45, center.1 - radius * 0.55),
                stroke / 2.0,
            );
            // Crown on top.
            let crown = segment_coverage(
                (px, py),
                (s * 0.4, s * 0.1),
                (s * 0.6, s * 0.1),
                stroke / 2.0,
            );
            let alpha = ring.max(hand).max(crown);
            out.extend_from_slice(&[color[0], color[1], color[2], (alpha * 255.0) as u8]);
        }
    }
    out
}

fn segment_coverage(p: (f32, f32), a: (f32, f32), b: (f32, f32), half_width: f32) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (p.0 - a.0, p.1 - a.1);
    let t = ((apx * abx + apy * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + abx * t - p.0, a.1 + aby * t - p.1);
    let dist = (cx * cx + cy * cy).sqrt();
    1.0 - (dist - half_width).clamp(0.0, 1.0)
}

fn toggle_label(running: bool, phase: Phase) -> String {
    match (running, phase) {
        (true, _) => "Pausar".into(),
        (false, Phase::Focus) => "Iniciar foco".into(),
        (false, _) => "Iniciar pausa".into(),
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use anyhow::Result;
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
        Icon, TrayIcon, TrayIconBuilder,
    };

    pub struct Tray {
        icon: TrayIcon,
        toggle: MenuItem,
        status: MenuItem,
        ids: Vec<(MenuId, TrayAction)>,
        last: Option<(Option<String>, bool, Phase)>,
    }

    impl Tray {
        pub fn new() -> Result<Self> {
            let toggle = MenuItem::new("Iniciar foco", true, None);
            let status = MenuItem::new("Enki", false, None);
            let skip = MenuItem::new("Pular fase", true, None);
            let reset = MenuItem::new("Reiniciar", true, None);
            let show = MenuItem::new("Abrir Enki", true, None);
            let mini = MenuItem::new("Modo mini", true, None);
            let quit = MenuItem::new("Sair do Enki", true, None);
            let menu = Menu::new();
            menu.append_items(&[
                &status,
                &PredefinedMenuItem::separator(),
                &toggle,
                &skip,
                &reset,
                &PredefinedMenuItem::separator(),
                &show,
                &mini,
                &PredefinedMenuItem::separator(),
                &quit,
            ])?;
            let ids = vec![
                (toggle.id().clone(), TrayAction::Toggle),
                (skip.id().clone(), TrayAction::Skip),
                (reset.id().clone(), TrayAction::Reset),
                (show.id().clone(), TrayAction::Show),
                (mini.id().clone(), TrayAction::Mini),
                (quit.id().clone(), TrayAction::Quit),
            ];
            let icon = TrayIconBuilder::new()
                .with_icon(Icon::from_rgba(icon_rgba(36, [0, 0, 0]), 36, 36)?)
                .with_icon_as_template(true)
                .with_menu(Box::new(menu))
                .with_tooltip("Enki")
                .build()?;
            Ok(Self {
                icon,
                toggle,
                status,
                ids,
                last: None,
            })
        }

        pub fn poll(&self) -> Vec<TrayAction> {
            let mut actions = Vec::new();
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if let Some((_, action)) = self.ids.iter().find(|(id, _)| *id == event.id) {
                    actions.push(*action);
                }
            }
            actions
        }

        pub fn update(&mut self, label: Option<String>, running: bool, phase: Phase) {
            let next = (label, running, phase);
            if self.last.as_ref() == Some(&next) {
                return;
            }
            let (label, running, phase) = &next;
            // Leading space separates the text from the glyph in the menu bar.
            self.icon.set_title(label.as_ref().map(|l| format!(" {l}")));
            self.toggle.set_text(toggle_label(*running, *phase));
            self.status.set_text(match label {
                Some(l) => format!("{} · {l}", phase.label()),
                None => "Enki".into(),
            });
            self.last = Some(next);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use anyhow::Result;
    use ksni::{blocking::TrayMethods, menu::StandardItem, MenuItem};
    use std::sync::mpsc;

    struct Sni {
        label: Option<String>,
        running: bool,
        phase: Phase,
        tx: mpsc::Sender<TrayAction>,
    }

    impl Sni {
        fn item(&self, label: String, action: TrayAction) -> MenuItem<Self> {
            StandardItem {
                label,
                activate: Box::new(move |this: &mut Self| {
                    let _ = this.tx.send(action);
                }),
                ..Default::default()
            }
            .into()
        }
    }

    impl ksni::Tray for Sni {
        fn id(&self) -> String {
            "enki".into()
        }

        fn title(&self) -> String {
            match &self.label {
                Some(l) => format!("Enki · {l}"),
                None => "Enki".into(),
            }
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            // ARGB32, network byte order. White reads well on the default dark panels.
            let size = 32;
            let rgba = icon_rgba(size, [255, 255, 255]);
            let data = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|&[r, g, b, a]| [a, r, g, b])
                .collect();
            vec![ksni::Icon {
                width: size as i32,
                height: size as i32,
                data,
            }]
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: self.title(),
                description: self.phase.label().into(),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.send(TrayAction::Show);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let status = match &self.label {
                Some(l) => format!("{} · {l}", self.phase.label()),
                None => "Enki".into(),
            };
            vec![
                StandardItem {
                    label: status,
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                self.item(toggle_label(self.running, self.phase), TrayAction::Toggle),
                self.item("Pular fase".into(), TrayAction::Skip),
                self.item("Reiniciar".into(), TrayAction::Reset),
                MenuItem::Separator,
                self.item("Abrir Enki".into(), TrayAction::Show),
                self.item("Modo mini".into(), TrayAction::Mini),
                MenuItem::Separator,
                self.item("Sair do Enki".into(), TrayAction::Quit),
            ]
        }
    }

    pub struct Tray {
        handle: ksni::blocking::Handle<Sni>,
        rx: mpsc::Receiver<TrayAction>,
        last: Option<(Option<String>, bool, Phase)>,
    }

    impl Tray {
        pub fn new() -> Result<Self> {
            let (tx, rx) = mpsc::channel();
            // Without a StatusNotifierWatcher yet (GNOME before the AppIndicator
            // extension loads), keep the service up and register once it appears.
            let handle = Sni {
                label: None,
                running: false,
                phase: Phase::Focus,
                tx,
            }
            .assume_sni_available(true)
            .spawn()?;
            Ok(Self {
                handle,
                rx,
                last: None,
            })
        }

        pub fn poll(&self) -> Vec<TrayAction> {
            self.rx.try_iter().collect()
        }

        pub fn update(&mut self, label: Option<String>, running: bool, phase: Phase) {
            let next = (label, running, phase);
            if self.last.as_ref() == Some(&next) {
                return;
            }
            let (label, running, phase) = next.clone();
            self.handle.update(move |t| {
                t.label = label;
                t.running = running;
                t.phase = phase;
            });
            self.last = Some(next);
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            let _ = self.handle.shutdown();
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;

    pub struct Tray;

    impl Tray {
        pub fn new() -> anyhow::Result<Self> {
            anyhow::bail!("no tray on this platform")
        }
        pub fn poll(&self) -> Vec<TrayAction> {
            Vec::new()
        }
        pub fn update(&mut self, _: Option<String>, _: bool, _: Phase) {}
    }
}

pub use platform::Tray;

#[cfg(test)]
mod tests {
    #[test]
    fn icon_has_opaque_and_clear_pixels() {
        let rgba = super::icon_rgba(32, [0, 0, 0]);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let alphas: Vec<u8> = rgba.chunks(4).map(|p| p[3]).collect();
        assert!(alphas.contains(&255));
        assert_eq!(alphas[0], 0);
    }
}
