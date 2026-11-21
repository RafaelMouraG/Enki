//! Single-line text field. GPUI ships no widgets, so this is adapted and
//! modified from GPUI's `examples/input.rs` (IME, selection, clipboard) with
//! Enki's styling, word navigation, horizontal scrolling, and focus/submit
//! behavior.
//!
//! Portions Copyright (c) Zed Industries, Inc.
//! Licensed under the Apache License, Version 2.0 (see THIRD_PARTY_NOTICES.md).

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style,
    TextRun, UTF16Selection, UnderlineStyle, Window,
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::theme::Theme;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        SelectToStart,
        SelectToEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToStart,
        Paste,
        Cut,
        Copy,
        Submit,
        Cancel,
        ShowCharacterPalette,
    ]
);

const CONTEXT: &str = "TextInput";
const CURSOR_PADDING: f32 = 2.;

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new("left", Left, Some(CONTEXT)),
        KeyBinding::new("right", Right, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("secondary-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("secondary-v", Paste, Some(CONTEXT)),
        KeyBinding::new("secondary-c", Copy, Some(CONTEXT)),
        KeyBinding::new("secondary-x", Cut, Some(CONTEXT)),
        KeyBinding::new("home", Home, Some(CONTEXT)),
        KeyBinding::new("end", End, Some(CONTEXT)),
        KeyBinding::new("shift-home", SelectToStart, Some(CONTEXT)),
        KeyBinding::new("shift-end", SelectToEnd, Some(CONTEXT)),
        KeyBinding::new("enter", Submit, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-left", WordLeft, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-right", WordRight, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-shift-left", SelectWordLeft, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-shift-right", SelectWordRight, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-backspace", DeleteWordLeft, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("alt-delete", DeleteWordRight, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-left", Home, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-right", End, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-left", SelectToStart, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-right", SelectToEnd, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-backspace", DeleteToStart, Some(CONTEXT)),
        #[cfg(target_os = "macos")]
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-left", WordLeft, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-right", WordRight, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-right", SelectWordRight, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, Some(CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-delete", DeleteWordRight, Some(CONTEXT)),
    ]);
}

pub enum TextInputEvent {
    Changed,
    Submit,
    Cancel,
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_offset: Pixels,
    is_selecting: bool,
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_offset: px(0.),
            is_selecting: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        if self.content.is_empty() {
            self.scroll_offset = px(0.);
        }
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text("", cx);
        cx.emit(TextInputEvent::Changed);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_to_start(&mut self, _: &SelectToStart, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(
            previous_word_boundary(&self.content, self.selected_range.start),
            cx,
        );
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(
            next_word_boundary(&self.content, self.selected_range.end),
            cx,
        );
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(
            previous_word_boundary(&self.content, self.cursor_offset()),
            cx,
        );
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(next_word_boundary(&self.content, self.cursor_offset()), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            let target = previous_word_boundary(&self.content, self.cursor_offset());
            if target == self.cursor_offset() {
                return;
            }
            self.select_to(target, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            let target = next_word_boundary(&self.content, self.cursor_offset());
            if target == self.cursor_offset() {
                return;
            }
            self.select_to(target, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_to_start(&mut self, _: &DeleteToStart, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            if self.cursor_offset() == 0 {
                return;
            }
            self.select_to(0, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextInputEvent::Submit);
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        window.blur();
        cx.emit(TextInputEvent::Cancel);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.click_count >= 3 {
            self.is_selecting = false;
            self.selection_reversed = false;
            self.selected_range = 0..self.content.len();
            cx.notify();
            return;
        }
        if event.click_count == 2 {
            self.is_selecting = false;
            self.selection_reversed = false;
            let idx = self.index_for_mouse_position(event.position);
            self.selected_range = word_range_at(&self.content, idx);
            cx.notify();
            return;
        }
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left() + self.scroll_offset)
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        offset_from_utf16_str(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        offset_to_utf16_str(&self.content, offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Whitespace,
    Word,
    Punctuation,
}

fn char_class(ch: char) -> CharClass {
    if ch.is_whitespace() {
        CharClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        CharClass::Word
    } else {
        CharClass::Punctuation
    }
}

fn clamp_char_boundary(text: &str, offset: usize) -> usize {
    let mut idx = offset.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn offset_from_utf16_str(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for ch in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }
    utf8_offset
}

fn offset_to_utf16_str(text: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;
    for ch in text.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

fn previous_word_boundary(text: &str, offset: usize) -> usize {
    let end = clamp_char_boundary(text, offset);
    let mut iter = text[..end].char_indices().rev().peekable();
    while matches!(iter.peek(), Some((_, ch)) if ch.is_whitespace()) {
        iter.next();
    }
    let Some((mut boundary, ch)) = iter.next() else {
        return 0;
    };
    let class = char_class(ch);
    for (idx, ch) in iter {
        if char_class(ch) != class {
            break;
        }
        boundary = idx;
    }
    boundary
}

fn next_word_boundary(text: &str, offset: usize) -> usize {
    let start = clamp_char_boundary(text, offset);
    let mut iter = text[start..].char_indices().peekable();
    while matches!(iter.peek(), Some((_, ch)) if ch.is_whitespace()) {
        iter.next();
    }
    let Some((_, ch)) = iter.next() else {
        return text.len();
    };
    let class = char_class(ch);
    for (idx, ch) in iter {
        if char_class(ch) != class {
            return start + idx;
        }
    }
    text.len()
}

fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let pos = clamp_char_boundary(text, offset);
    let (pivot_start, pivot_ch) = if pos < text.len() {
        let ch = text[pos..].chars().next().unwrap();
        (pos, ch)
    } else {
        let (idx, ch) = text.char_indices().next_back().unwrap();
        (idx, ch)
    };
    let class = char_class(pivot_ch);

    let mut start = pivot_start;
    for (idx, ch) in text[..pivot_start].char_indices().rev() {
        if char_class(ch) != class {
            break;
        }
        start = idx;
    }

    let tail_start = pivot_start + pivot_ch.len_utf8();
    for (rel_idx, ch) in text[tail_start..].char_indices() {
        if char_class(ch) != class {
            return start..tail_start + rel_idx;
        }
    }
    start..text.len()
}

fn ime_selected_range(
    content: &str,
    replaced_start: usize,
    new_text: &str,
    new_selected_range_utf16: Option<&Range<usize>>,
) -> Range<usize> {
    match new_selected_range_utf16 {
        Some(range_utf16) => {
            let rel_start = offset_from_utf16_str(new_text, range_utf16.start);
            let rel_end = offset_from_utf16_str(new_text, range_utf16.end);
            let start = clamp_char_boundary(content, replaced_start.saturating_add(rel_start));
            let end = clamp_char_boundary(content, replaced_start.saturating_add(rel_end));
            if start <= end {
                start..end
            } else {
                end..start
            }
        }
        None => {
            let end = clamp_char_boundary(content, replaced_start.saturating_add(new_text.len()));
            end..end
        }
    }
}

fn compute_scroll_offset(
    current_offset: Pixels,
    cursor_x: Pixels,
    line_width: Pixels,
    viewport_width: Pixels,
) -> Pixels {
    let current = f32::from(current_offset);
    let cursor = f32::from(cursor_x);
    let line_w = f32::from(line_width);
    let view_w = f32::from(viewport_width);
    let content_w = line_w + CURSOR_PADDING;
    if view_w <= 0. || content_w <= view_w {
        return px(0.);
    }
    let max_scroll = (content_w - view_w).max(0.);
    let mut offset = current.clamp(0., max_scroll);
    if cursor < offset {
        offset = cursor;
    } else if cursor + CURSOR_PADDING > offset + view_w {
        offset = cursor + CURSOR_PADDING - view_w;
    }
    px(offset.clamp(0., max_scroll))
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range.take();
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.selected_range = ime_selected_range(
            &self.content,
            range.start,
            new_text,
            new_selected_range_utf16.as_ref(),
        );
        self.selection_reversed = false;
        cx.emit(TextInputEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start) - self.scroll_offset,
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end) - self.scroll_offset,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_for_x(line_point.x + self.scroll_offset)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_offset: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = Theme::get(cx);
        let (placeholder_color, cursor_color, selection_color) =
            (theme.text_faint, theme.accent, theme.accent_soft);
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let current_scroll = input.scroll_offset;
        let style = window.text_style();

        let is_empty = content.is_empty();
        let content_len = content.len();
        let (display_text, text_color) = if is_empty {
            (input.placeholder.clone(), placeholder_color)
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = if is_empty {
            px(0.)
        } else {
            line.x_for_index(cursor)
        };
        let scroll_offset = if is_empty {
            px(0.)
        } else {
            compute_scroll_offset(
                current_scroll,
                cursor_pos,
                line.x_for_index(content_len),
                bounds.size.width,
            )
        };
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(
                            bounds.left() + cursor_pos - scroll_offset,
                            bounds.top() + px(1.),
                        ),
                        size(px(1.5), bounds.bottom() - bounds.top() - px(2.)),
                    ),
                    cursor_color,
                )),
            )
        } else {
            let left = f32::from(bounds.left());
            let right = f32::from(bounds.right());
            let start_x = px(
                f32::from(bounds.left() + line.x_for_index(selected_range.start) - scroll_offset)
                    .clamp(left, right),
            );
            let end_x = px(
                f32::from(bounds.left() + line.x_for_index(selected_range.end) - scroll_offset)
                    .clamp(left, right),
            );
            (
                Some(fill(
                    Bounds::from_corners(
                        point(start_x, bounds.top()),
                        point(end_x, bounds.bottom()),
                    ),
                    selection_color,
                )),
                None,
            )
        };
        PrepaintState {
            line: Some(line),
            cursor,
            selection,
            scroll_offset,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let scroll_offset = prepaint.scroll_offset;
        let line = prepaint.line.take().unwrap();
        line.paint(
            point(bounds.origin.x - scroll_offset, bounds.origin.y),
            window.line_height(),
            window,
            cx,
        )
        .ok();

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll_offset = scroll_offset;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_to_start))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_to_start))
            .on_action(cx.listener(Self::select_to_end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::cancel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_boundaries_ascii_and_punctuation() {
        let s = "hello   world-test!";
        assert_eq!(previous_word_boundary(s, s.len()), 18); // before '!'
        assert_eq!(previous_word_boundary(s, 18), 14); // "test"
        assert_eq!(previous_word_boundary(s, 14), 13); // '-'
        assert_eq!(previous_word_boundary(s, 13), 8); // "world"
        assert_eq!(previous_word_boundary(s, 8), 0); // skips spaces, lands on "hello"
        assert_eq!(previous_word_boundary(s, 0), 0);

        assert_eq!(next_word_boundary(s, 0), 5); // end of "hello"
        assert_eq!(next_word_boundary(s, 5), 13); // skips spaces, end of "world"
        assert_eq!(next_word_boundary(s, 13), 14); // past '-'
        assert_eq!(next_word_boundary(s, 14), 18); // end of "test"
        assert_eq!(next_word_boundary(s, 18), 19); // past '!'
        assert_eq!(next_word_boundary(s, 19), 19);
    }

    #[test]
    fn word_boundaries_unicode_and_whitespace() {
        let s = "  café ação  ";
        let cafe_start = 2;
        let cafe_end = cafe_start + "café".len();
        let acao_start = cafe_end + 1;
        let acao_end = acao_start + "ação".len();

        assert_eq!(next_word_boundary(s, 0), cafe_end);
        assert_eq!(next_word_boundary(s, cafe_end), acao_end);
        assert_eq!(next_word_boundary(s, acao_end), s.len());

        assert_eq!(previous_word_boundary(s, s.len()), acao_start);
        assert_eq!(previous_word_boundary(s, acao_start), cafe_start);
        assert_eq!(previous_word_boundary(s, cafe_start), 0);
        assert_eq!(previous_word_boundary("   ", 3), 0);
        assert_eq!(next_word_boundary("   ", 0), 3);
    }

    #[test]
    fn word_range_double_click() {
        let s = "revisar código-fonte";
        let codigo_start = "revisar ".len();
        let codigo_end = codigo_start + "código".len();
        let dash = codigo_end;
        let fonte_start = dash + 1;

        assert_eq!(word_range_at(s, 0), 0.."revisar".len());
        assert_eq!(word_range_at(s, 3), 0.."revisar".len());
        assert_eq!(word_range_at(s, 7), 7..8);
        assert_eq!(word_range_at(s, codigo_start + 1), codigo_start..codigo_end);
        assert_eq!(word_range_at(s, dash), dash..dash + 1);
        assert_eq!(word_range_at(s, s.len()), fonte_start..s.len());
        assert_eq!(word_range_at("", 0), 0..0);
    }

    #[test]
    fn utf16_and_clamp_helpers() {
        let s = "a😀b"; // a: 1B/1u16, 😀: 4B/2u16, b: 1B/1u16
        assert_eq!(offset_to_utf16_str(s, 0), 0);
        assert_eq!(offset_to_utf16_str(s, 1), 1);
        assert_eq!(offset_to_utf16_str(s, 5), 3);
        assert_eq!(offset_to_utf16_str(s, 6), 4);

        assert_eq!(offset_from_utf16_str(s, 0), 0);
        assert_eq!(offset_from_utf16_str(s, 1), 1);
        assert_eq!(offset_from_utf16_str(s, 3), 5);
        assert_eq!(offset_from_utf16_str(s, 4), 6);

        assert_eq!(clamp_char_boundary(s, 0), 0);
        assert_eq!(clamp_char_boundary(s, 3), 1); // inside 😀 -> start of 😀
        assert_eq!(clamp_char_boundary(s, 100), s.len());
    }

    #[test]
    fn ime_selected_range_uses_range_start_and_clamps_safely() {
        // Replacing a non-empty range at start=2..end=10 with "é" -> content is "hié!"
        let content = "hié!";
        let new_text = "é";
        assert_eq!(ime_selected_range(content, 2, new_text, Some(&(1..1))), 4..4);
        assert_eq!(ime_selected_range(content, 2, new_text, Some(&(0..1))), 2..4);
        // Out-of-bounds UTF-16 range still clamps safely to content boundary.
        assert_eq!(
            ime_selected_range(content, 2, new_text, Some(&(0..99))),
            2..4
        );
        // Emoji before replacement does not skew relative UTF-16 offsets.
        let emoji_content = "😀abc";
        let start = "😀".len();
        assert_eq!(
            ime_selected_range(emoji_content, start, "ab", Some(&(1..2))),
            start + 1..start + 2
        );
        assert_eq!(ime_selected_range(emoji_content, start, "ab", None), start + 2..start + 2);
    }

    #[test]
    fn scroll_offset_keeps_cursor_visible() {
        // Fits inside viewport -> no scroll.
        assert_eq!(compute_scroll_offset(px(20.), px(50.), px(80.), px(100.)), px(0.));

        // Cursor at end of a 200px line inside a 100px viewport.
        assert_eq!(
            compute_scroll_offset(px(0.), px(200.), px(200.), px(100.)),
            px(100. + CURSOR_PADDING)
        );

        // Moving cursor slightly left while still visible keeps current offset.
        assert_eq!(
            compute_scroll_offset(px(102.), px(150.), px(200.), px(100.)),
            px(102.)
        );

        // Moving cursor left past offset scrolls left to follow cursor.
        assert_eq!(
            compute_scroll_offset(px(102.), px(40.), px(200.), px(100.)),
            px(40.)
        );

        // Shrinking text clamps stale scroll offset.
        assert_eq!(
            compute_scroll_offset(px(150.), px(120.), px(120.), px(100.)),
            px(20. + CURSOR_PADDING)
        );
    }
}
