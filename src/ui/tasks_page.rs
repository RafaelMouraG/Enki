//! Task list: add, complete, estimate and pick the one to focus on.

use gpui::{
    div, prelude::*, px, AnyElement, ClickEvent, Context, Focusable, FontWeight, SharedString,
    Window,
};

use crate::{
    assets::Icon,
    state::Page,
    store::Task,
    theme::{radius, Theme},
    ui::{
        components::StyledExt as _,
        components::{
            button, card, icon, icon_sized, page_title, section_label, ButtonKind, SurfaceExt as _,
        },
        on_model,
        root::RootView,
    },
};

pub fn render(view: &mut RootView, window: &mut Window, cx: &mut Context<RootView>) -> AnyElement {
    let input_focused = view.task_input.focus_handle(cx).is_focused(window);
    let model = view.model.clone();
    let new_estimate = view.new_task_estimate;
    let editing_task = view.editing_task.clone();
    let edit_task_input = view.edit_task_input.clone();
    let theme = Theme::get(cx).clone();
    let (open, done, current): (Vec<Task>, Vec<Task>, Option<String>) = {
        let enki = model.read(cx);
        (
            enki.open_tasks().cloned().collect(),
            enki.tasks.iter().filter(|t| t.done).cloned().collect(),
            enki.current_task.clone(),
        )
    };

    let summary = match (open.len(), done.len()) {
        (0, 0) => {
            "Divida o trabalho em tarefas e estime quantos pomodoros cada uma leva.".to_string()
        }
        (o, d) => format!(
            "{o} {} · {d} {}",
            if o == 1 { "aberta" } else { "abertas" },
            if d == 1 { "concluída" } else { "concluídas" }
        ),
    };

    let hover_bg = theme.surface_hover;
    let dec_enabled = new_estimate > 1;
    let inc_enabled = new_estimate < 99;
    let estimate_picker = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .px(px(4.))
        .h(px(28.))
        .rounded(radius::SM)
        .bg(theme.control)
        .child(
            div()
                .id("new-est-dec")
                .flex()
                .items_center()
                .justify_center()
                .size(px(20.))
                .rounded(radius::SM)
                .when(dec_enabled, |d| {
                    d.cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.new_task_estimate =
                                this.new_task_estimate.saturating_sub(1).max(1);
                            cx.notify();
                        }))
                })
                .child(icon_sized(
                    Icon::Minus,
                    px(12.),
                    if dec_enabled {
                        theme.text_muted
                    } else {
                        theme.text_faint.opacity(0.45)
                    },
                )),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(4.))
                .text_size(px(12.5))
                .font(crate::theme::tabular_font())
                .text_color(if new_estimate > 1 {
                    theme.accent_text
                } else {
                    theme.text_muted
                })
                .child(icon_sized(
                    Icon::Tomato,
                    px(13.),
                    if new_estimate > 1 {
                        theme.accent
                    } else {
                        theme.text_faint
                    },
                ))
                .child(new_estimate.to_string()),
        )
        .child(
            div()
                .id("new-est-inc")
                .flex()
                .items_center()
                .justify_center()
                .size(px(20.))
                .rounded(radius::SM)
                .when(inc_enabled, |d| {
                    d.cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.new_task_estimate = (this.new_task_estimate + 1).min(99);
                            cx.notify();
                        }))
                })
                .child(icon_sized(
                    Icon::Plus,
                    px(12.),
                    if inc_enabled {
                        theme.text_muted
                    } else {
                        theme.text_faint.opacity(0.45)
                    },
                )),
        );

    let add_row = div()
        .relative()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(48.))
        .px(px(14.))
        .rounded(radius::LG)
        .surface("tasks-add", radius::LG, &theme)
        .border_1()
        .border_color(match (input_focused, theme.liquid()) {
            (true, _) => theme.accent,
            (false, true) => gpui::transparent_black(),
            (false, false) => theme.border_strong,
        })
        .text_size(px(14.))
        .child(icon_sized(Icon::Plus, px(16.), theme.text_faint))
        .child(view.task_input.clone())
        .child(estimate_picker)
        .child(
            div()
                .flex_none()
                .text_size(px(12.))
                .text_color(theme.text_faint)
                .child("Enter para adicionar"),
        );

    let row = |task: Task,
               first: bool,
               last: bool,
               can_reorder: bool,
               cx: &mut Context<RootView>|
     -> AnyElement {
        let id = task.id.clone();
        let selected = current.as_deref() == Some(task.id.as_str());
        let is_editing = !task.done && editing_task.as_deref() == Some(task.id.as_str());
        let group = SharedString::from(format!("task-{}", task.id));
        let hover_bg = theme.surface_hover;

        let check = {
            let id = id.clone();
            let done = task.done;
            div()
                .id(SharedString::from(format!("check-{}", task.id)))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(20.))
                .rounded_full()
                .cursor_pointer()
                .map(|d| {
                    if done {
                        d.bg(theme.accent)
                            .child(icon_sized(Icon::Check, px(12.), theme.on_accent))
                    } else {
                        d.border_1()
                            .border_color(theme.border_strong)
                            .hover(|s| s.border_color(theme.accent))
                    }
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.editing_task.as_deref() == Some(id.as_str()) {
                        this.editing_task = None;
                    }
                    this.model
                        .update(cx, |m, cx| m.set_task_done(&id, !done, cx));
                }))
        };

        let title_slot: AnyElement = if is_editing {
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .h(px(32.))
                .px(px(8.))
                .rounded(radius::SM)
                .bg(theme.control)
                .border_1()
                .border_color(theme.accent)
                .text_size(px(14.))
                .text_color(theme.text)
                .child(edit_task_input.clone())
                .into_any_element()
        } else {
            let edit_id = id.clone();
            let edit_title = task.title.clone();
            div()
                .id(SharedString::from(format!("title-{}", task.id)))
                .flex_1()
                .min_w_0()
                .ellipsis()
                .text_size(px(14.))
                .text_color(if task.done {
                    theme.text_faint
                } else {
                    theme.text
                })
                .when(task.done, |d| d.line_through())
                .when(!task.done, |d| {
                    d.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.click_count() == 2 {
                            this.start_editing_task(edit_id.clone(), &edit_title, window, cx);
                        }
                    }))
                })
                .child(task.title.clone())
                .into_any_element()
        };

        if is_editing {
            return div()
                .id(SharedString::from(format!("row-{}", task.id)))
                .group(group)
                .flex()
                .items_center()
                .gap(px(12.))
                .min_h(px(52.))
                .px(px(14.))
                .when(!first, |d| d.border_t_1().border_color(theme.border))
                .when(selected, |d| d.bg(theme.accent_soft.opacity(0.45)))
                .child(check)
                .child(title_slot)
                .child(
                    div()
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(theme.text_faint)
                        .child("Enter salvar · Esc cancelar"),
                )
                .into_any_element();
        }

        let estimate = {
            let (dec_id, inc_id) = (id.clone(), id.clone());
            let small = |suffix: &str, glyph: Icon| {
                div()
                    .id(SharedString::from(format!("{suffix}-{}", task.id)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(20.))
                    .rounded(radius::SM)
                    .cursor_pointer()
                    .invisible()
                    .group_hover(group.clone(), |s| s.visible())
                    .hover(move |s| s.bg(hover_bg))
                    .child(icon_sized(glyph, px(12.), theme.text_muted))
            };
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .map(|d| {
                    if task.done {
                        d.child(div().size(px(20.)))
                    } else {
                        d.child(
                            small("dec", Icon::Minus).on_click(on_model(&model, move |m, cx| {
                                m.adjust_estimate(&dec_id, -1, cx)
                            })),
                        )
                    }
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .px(px(4.))
                        .text_size(px(12.5))
                        .font(crate::theme::tabular_font())
                        .text_color(if task.pomodoros >= task.estimate {
                            theme.rest
                        } else {
                            theme.text_muted
                        })
                        .child(icon_sized(Icon::Tomato, px(13.), theme.text_faint))
                        .child(format!("{}/{}", task.pomodoros, task.estimate)),
                )
                .map(|d| {
                    if task.done {
                        d.child(div().size(px(20.)))
                    } else {
                        d.child(
                            small("inc", Icon::Plus).on_click(on_model(&model, move |m, cx| {
                                m.adjust_estimate(&inc_id, 1, cx)
                            })),
                        )
                    }
                })
        };

        let focus_button = (!task.done).then(|| {
            let id = id.clone();
            div()
                .id(SharedString::from(format!("focus-{}", task.id)))
                .flex()
                .items_center()
                .justify_center()
                .w(px(92.))
                .gap(px(6.))
                .h(px(26.))
                .px(px(10.))
                .rounded_full()
                .cursor_pointer()
                .text_size(px(12.5))
                .font_weight(FontWeight::MEDIUM)
                .map(|d| {
                    if selected {
                        d.bg(theme.accent_soft).text_color(theme.accent_text)
                    } else {
                        d.text_color(theme.text_muted)
                            .invisible()
                            .group_hover(group.clone(), |s| s.visible())
                            .hover(move |s| s.bg(hover_bg))
                    }
                })
                .child(icon(Icon::Target).size(px(13.)).text_color(if selected {
                    theme.accent_text
                } else {
                    theme.text_muted
                }))
                .child(if selected { "Em foco" } else { "Focar" })
                .on_click(on_model(&model, move |m, cx| {
                    m.set_current_task(Some(id.clone()), cx);
                    m.navigate(Page::Timer, cx);
                }))
        });

        let reorder = (!task.done && can_reorder).then(|| {
            let up = if first {
                div().size(px(22.)).into_any_element()
            } else {
                let up_id = id.clone();
                div()
                    .id(SharedString::from(format!("move-up-{}", task.id)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(22.))
                    .rounded(radius::SM)
                    .cursor_pointer()
                    .invisible()
                    .group_hover(group.clone(), |s| s.visible())
                    .hover(move |s| s.bg(hover_bg))
                    .child(icon_sized(Icon::ChevronUp, px(13.), theme.text_muted))
                    .on_click(on_model(&model, move |m, cx| m.move_task(&up_id, -1, cx)))
                    .into_any_element()
            };
            let down = if last {
                div().size(px(22.)).into_any_element()
            } else {
                let down_id = id.clone();
                div()
                    .id(SharedString::from(format!("move-down-{}", task.id)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(22.))
                    .rounded(radius::SM)
                    .cursor_pointer()
                    .invisible()
                    .group_hover(group.clone(), |s| s.visible())
                    .hover(move |s| s.bg(hover_bg))
                    .child(icon_sized(Icon::ChevronDown, px(13.), theme.text_muted))
                    .on_click(on_model(&model, move |m, cx| m.move_task(&down_id, 1, cx)))
                    .into_any_element()
            };
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(2.))
                .child(up)
                .child(down)
        });

        let edit_button = (!task.done).then(|| {
            let edit_id = id.clone();
            let edit_title = task.title.clone();
            div()
                .id(SharedString::from(format!("edit-{}", task.id)))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(24.))
                .rounded(radius::SM)
                .cursor_pointer()
                .invisible()
                .group_hover(group.clone(), |s| s.visible())
                .hover(move |s| s.bg(hover_bg))
                .child(icon_sized(Icon::Pencil, px(13.), theme.text_muted))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.start_editing_task(edit_id.clone(), &edit_title, window, cx);
                }))
        });

        let delete = {
            let id = id.clone();
            div()
                .id(SharedString::from(format!("delete-{}", task.id)))
                .group("task-delete")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(26.))
                .rounded(radius::SM)
                .cursor_pointer()
                .invisible()
                .group_hover(group.clone(), |s| s.visible())
                .hover(move |s| s.bg(hover_bg))
                .child(
                    icon(Icon::Trash)
                        .size(px(14.))
                        .text_color(theme.text_faint)
                        .group_hover("task-delete", |s| s.text_color(theme.danger)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.editing_task.as_deref() == Some(id.as_str()) {
                        this.editing_task = None;
                    }
                    this.model.update(cx, |m, cx| m.delete_task(&id, cx));
                }))
        };

        div()
            .id(SharedString::from(format!("row-{}", task.id)))
            .group(group.clone())
            .flex()
            .items_center()
            .gap(px(10.))
            .min_h(px(52.))
            .px(px(14.))
            .when(!first, |d| d.border_t_1().border_color(theme.border))
            .when(selected, |d| d.bg(theme.accent_soft.opacity(0.45)))
            .child(check)
            .child(title_slot)
            .child(estimate)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .justify_center()
                    .w(px(92.))
                    .children(focus_button),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .children(reorder)
                    .children(edit_button)
                    .child(delete),
            )
            .into_any_element()
    };

    let open_len = open.len();
    let open_list = (!open.is_empty()).then(|| {
        card("tasks-open", &theme).children(
            open.into_iter()
                .enumerate()
                .map(|(ix, t)| row(t, ix == 0, ix + 1 == open_len, open_len > 1, cx)),
        )
    });
    let done_len = done.len();
    let clear_done_btn = (!done.is_empty()).then(|| {
        button(
            "clear-done",
            "Limpar concluídas",
            ButtonKind::Ghost,
            &theme,
            window,
            cx,
            on_model(&model, |m, cx| m.clear_done_tasks(cx)),
        )
    });
    let done_section = (!done.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_label(&theme, "Concluídas"))
                    .children(clear_done_btn),
            )
            .child(card("tasks-done", &theme).children(
                done.into_iter()
                    .enumerate()
                    .map(|(ix, t)| row(t, ix == 0, ix + 1 == done_len, false, cx)),
            ))
    });

    let empty = open_list.is_none().then(|| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .py(px(48.))
            .text_color(theme.text_faint)
            .child(icon_sized(Icon::ListChecks, px(28.), theme.text_faint))
            .child(div().text_size(px(14.)).child("Nada pendente por aqui."))
    });

    div()
        .id("tasks-page")
        .size_full()
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(16.))
                .w_full()
                .max_w(px(720.))
                .mx_auto()
                .px(px(32.))
                .pt(px(24.))
                .pb(px(48.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(page_title(&theme, "Tarefas"))
                        .child(
                            div()
                                .text_size(px(14.))
                                .text_color(theme.text_muted)
                                .child(summary),
                        ),
                )
                .child(add_row)
                .children(open_list)
                .children(empty)
                .children(done_section),
        )
        .into_any_element()
}
