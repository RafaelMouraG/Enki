//! Global actions, their shortcuts and the macOS menu bar.

use gpui::{actions, App, Context, KeyBinding, Menu, MenuItem, SystemMenuType};

use crate::{
    state::{model, Enki, Page, SettingsSection},
    ui::text_input,
};

actions!(
    enki,
    [
        ToggleTimer,
        SkipPhase,
        ResetTimer,
        ShowTimer,
        ShowTasks,
        ShowStats,
        OpenSettings,
        ToggleSidebar,
        ToggleMini,
        NewTask,
        CloseWindow,
        Minimize,
        Zoom,
        Hide,
        HideOthers,
        ShowAll,
        Quit,
    ]
);

fn update(cx: &mut App, f: impl FnOnce(&mut Enki, &mut Context<Enki>)) {
    model(cx).update(cx, f);
}

/// Shortcuts that are plain letters must not fire while typing.
const NOT_TYPING: &str = "!TextInput";

pub fn register(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", ToggleTimer, Some(NOT_TYPING)),
        KeyBinding::new("s", SkipPhase, Some(NOT_TYPING)),
        KeyBinding::new("r", ResetTimer, Some(NOT_TYPING)),
        KeyBinding::new("secondary-enter", ToggleTimer, None),
        KeyBinding::new("secondary-1", ShowTimer, None),
        KeyBinding::new("secondary-2", ShowTasks, None),
        KeyBinding::new("secondary-3", ShowStats, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-b", ToggleSidebar, None),
        KeyBinding::new("secondary-shift-m", ToggleMini, None),
        KeyBinding::new("secondary-n", NewTask, None),
        KeyBinding::new("secondary-w", CloseWindow, None),
        KeyBinding::new("secondary-q", Quit, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-m", Minimize, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-h", Hide, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-alt-h", HideOthers, None),
    ]);

    cx.on_action(|_: &ToggleTimer, cx| update(cx, |m, cx| m.toggle_timer(cx)));
    cx.on_action(|_: &SkipPhase, cx| update(cx, |m, cx| m.skip_phase(cx)));
    cx.on_action(|_: &ResetTimer, cx| update(cx, |m, cx| m.reset_timer(cx)));
    cx.on_action(|_: &ShowTimer, cx| {
        crate::show_main_window(cx);
        update(cx, |m, cx| m.navigate(Page::Timer, cx))
    });
    cx.on_action(|_: &ShowTasks, cx| {
        crate::show_main_window(cx);
        update(cx, |m, cx| m.navigate(Page::Tasks, cx))
    });
    cx.on_action(|_: &ShowStats, cx| {
        crate::show_main_window(cx);
        update(cx, |m, cx| m.navigate(Page::Stats, cx))
    });
    cx.on_action(|_: &OpenSettings, cx| {
        crate::show_main_window(cx);
        update(cx, |m, cx| {
            m.navigate(Page::Settings(SettingsSection::General), cx)
        })
    });
    cx.on_action(|_: &ToggleSidebar, cx| {
        update(cx, |m, cx| {
            m.update_settings(cx, |s| s.sidebar_open = !s.sidebar_open)
        })
    });
    cx.on_action(|_: &ToggleMini, cx| crate::toggle_mini_window(cx));
    cx.on_action(|_: &NewTask, cx| {
        // Reached when the main window isn't focused; the root view handles it otherwise.
        crate::show_main_window(cx);
        update(cx, |m, cx| m.navigate(Page::Tasks, cx));
    });
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(window) = cx.active_window() {
            window
                .update(cx, |_, window, _| {
                    crate::native::glass_cleanup(window);
                    window.remove_window();
                })
                .ok();
        }
    });
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = cx.active_window() {
            window
                .update(cx, |_, window, _| window.minimize_window())
                .ok();
        }
    });
    cx.on_action(|_: &Zoom, cx| {
        if let Some(window) = cx.active_window() {
            window.update(cx, |_, window, _| window.zoom_window()).ok();
        }
    });
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Quit, cx| cx.quit());

    cx.set_menus(vec![
        Menu {
            name: "Enki".into(),
            items: vec![
                MenuItem::action("Configurações…", OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu("Serviços", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Ocultar Enki", Hide),
                MenuItem::action("Ocultar outros", HideOthers),
                MenuItem::action("Mostrar todos", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Sair do Enki", Quit),
            ],
        },
        Menu {
            name: "Editar".into(),
            items: vec![
                MenuItem::action("Recortar", text_input::Cut),
                MenuItem::action("Copiar", text_input::Copy),
                MenuItem::action("Colar", text_input::Paste),
                MenuItem::action("Selecionar tudo", text_input::SelectAll),
            ],
        },
        Menu {
            name: "Timer".into(),
            items: vec![
                MenuItem::action("Iniciar / pausar", ToggleTimer),
                MenuItem::action("Pular fase", SkipPhase),
                MenuItem::action("Reiniciar fase", ResetTimer),
                MenuItem::separator(),
                MenuItem::action("Nova tarefa", NewTask),
            ],
        },
        Menu {
            name: "Visualizar".into(),
            items: vec![
                MenuItem::action("Timer", ShowTimer),
                MenuItem::action("Tarefas", ShowTasks),
                MenuItem::action("Estatísticas", ShowStats),
                MenuItem::separator(),
                MenuItem::action("Mostrar/ocultar barra lateral", ToggleSidebar),
                MenuItem::action("Modo mini", ToggleMini),
            ],
        },
        Menu {
            name: "Janela".into(),
            items: vec![
                MenuItem::action("Minimizar", Minimize),
                MenuItem::action("Zoom", Zoom),
                MenuItem::separator(),
                MenuItem::action("Fechar janela", CloseWindow),
            ],
        },
    ]);
}
