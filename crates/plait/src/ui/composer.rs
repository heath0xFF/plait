use super::theme::{self, BLUE, ButtonAction, EDGE, MUTED, RAIL, SUBTLE, TEXT};
use gpui::{prelude::*, *};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

const LINE_HEIGHT: f32 = 21.;
// Four lines: room to edit a prompt without crowding the conversation.
const INPUT_HEIGHT: f32 = 84.;
// Focus edge for the whole box, quieter than the keyboard ring on small controls.
const FOCUS_EDGE: u32 = 0x6f8fc4;
// shortcut: keep 100 full drafts; use edit deltas if large drafts become common.
const HISTORY_LIMIT: usize = 100;

actions!(
    composer,
    [
        Submit,
        Newline,
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        Home,
        End,
        SelectHome,
        SelectEnd,
        DocumentStart,
        DocumentEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let context = Some("PlaitComposer");
    cx.bind_keys([
        KeyBinding::new("enter", Submit, context),
        KeyBinding::new("shift-enter", Newline, context),
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("cmd-left", Home, context),
        KeyBinding::new("cmd-right", End, context),
        KeyBinding::new("cmd-shift-left", SelectHome, context),
        KeyBinding::new("cmd-shift-right", SelectEnd, context),
        KeyBinding::new("home", DocumentStart, context),
        KeyBinding::new("end", DocumentEnd, context),
        KeyBinding::new("cmd-up", DocumentStart, context),
        KeyBinding::new("cmd-down", DocumentEnd, context),
        KeyBinding::new("alt-left", WordLeft, context),
        KeyBinding::new("alt-right", WordRight, context),
        KeyBinding::new("alt-shift-left", SelectWordLeft, context),
        KeyBinding::new("alt-shift-right", SelectWordRight, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-z", Undo, context),
        KeyBinding::new("cmd-shift-z", Redo, context),
    ]);
}

#[derive(Clone, Debug)]
pub enum ComposerEvent {
    Submit(String),
    Stop,
}

#[derive(Clone, Default)]
struct Snapshot {
    text: String,
    anchor: usize,
    head: usize,
}

#[derive(Default)]
struct Draft {
    value: Snapshot,
    marked: Option<Range<usize>>,
    composition_start: Option<Snapshot>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    revision: u64,
    submitted: Option<(String, u64)>,
}

impl Draft {
    fn selection(&self) -> Range<usize> {
        self.value.anchor.min(self.value.head)..self.value.anchor.max(self.value.head)
    }

    fn boundary(&self, offset: usize, forward: bool) -> usize {
        let mut indices = self.value.text.grapheme_indices(true).map(|(i, _)| i);
        if forward {
            indices
                .find(|i| *i > offset)
                .unwrap_or(self.value.text.len())
        } else {
            indices.take_while(|i| *i < offset).last().unwrap_or(0)
        }
    }

    fn snap(&self, offset: usize) -> usize {
        if offset >= self.value.text.len() {
            self.value.text.len()
        } else {
            self.value
                .text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .take_while(|i| *i <= offset)
                .last()
                .unwrap_or(0)
        }
    }

    fn snap_end(&self, offset: usize) -> usize {
        self.value
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= offset)
            .unwrap_or(self.value.text.len())
    }

    fn normalize_selection(&mut self) {
        let range = self.selection();
        let start = if range.is_empty() {
            self.snap_end(range.start)
        } else {
            self.snap(range.start)
        };
        let end = self.snap_end(range.end);
        if self.value.head < self.value.anchor {
            self.value.head = start;
            self.value.anchor = end;
        } else {
            self.value.anchor = start;
            self.value.head = end;
        }
    }

    fn select(&mut self, offset: usize, extend: bool) {
        self.value.head = self.snap(offset);
        if !extend {
            self.value.anchor = self.value.head;
        }
    }

    fn remember(&mut self, before: Snapshot) {
        if before.text == self.value.text {
            return;
        }
        if self.undo.len() == HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }

    fn finish_composition(&mut self) {
        self.marked = None;
        self.normalize_selection();
        if let Some(before) = self.composition_start.take() {
            self.remember(before);
        }
    }

    fn replace(&mut self, range: Range<usize>, text: &str, composing: bool) {
        let before = self.value.clone();
        if composing && self.composition_start.is_none() {
            self.composition_start = Some(before.clone());
        }
        self.value.text.replace_range(range.clone(), text);
        self.value.head = range.start + text.len();
        self.value.anchor = self.value.head;
        if before.text != self.value.text {
            self.revision += 1;
        }
        if composing {
            self.marked = Some(range.start..self.value.head);
        } else if self.composition_start.is_some() {
            self.finish_composition();
        } else {
            self.marked = None;
            self.normalize_selection();
            self.remember(before);
        }
    }

    fn mark(&mut self, range: Range<usize>, text: &str, selected: Option<Range<usize>>) {
        let start = range.start;
        self.replace(range, text, true);
        if let Some(selected) = selected {
            self.value.anchor = start + byte_offset(text, selected.start);
            self.value.head = start + byte_offset(text, selected.end);
        }
    }

    fn set_external_value(&mut self, text: &str, enabled: bool) -> bool {
        if !enabled {
            return false;
        }
        // End the IME transaction before a whole-field accessibility edit.
        self.finish_composition();
        self.replace(0..self.value.text.len(), text, false);
        true
    }

    fn insert(&mut self, text: &str) {
        let range = self.marked.clone().unwrap_or_else(|| self.selection());
        self.replace(range, text, false);
    }

    fn delete(&mut self, forward: bool) {
        self.finish_composition();
        let mut range = self.selection();
        if range.is_empty() {
            if forward {
                range.end = self.boundary(range.end, true);
            } else {
                range.start = self.boundary(range.start, false);
            }
        }
        self.replace(range, "", false);
    }

    fn undo(&mut self, redo: bool) {
        self.finish_composition();
        let history = if redo { &mut self.redo } else { &mut self.undo };
        if let Some(value) = history.pop() {
            let previous = std::mem::replace(&mut self.value, value);
            if previous.text != self.value.text {
                self.revision += 1;
            }
            if redo {
                self.undo.push(previous);
            } else {
                self.redo.push(previous);
            }
        }
    }

    fn can_submit(&self, enabled: bool, busy: bool) -> bool {
        enabled && !busy && self.marked.is_none() && !self.value.text.trim().is_empty()
    }

    fn submit(&mut self, enabled: bool, busy: bool) -> Option<String> {
        if !self.can_submit(enabled, busy) {
            return None;
        }
        let text = self.value.text.clone();
        self.submitted = Some((text.clone(), self.revision));
        Some(text)
    }

    fn clear_if_unchanged(&mut self, submitted: &str) -> bool {
        let Some((text, revision)) = self.submitted.take() else {
            return false;
        };
        if text != submitted
            || revision != self.revision
            || self.value.text != submitted
            || self.marked.is_some()
        {
            return false;
        }
        self.replace(0..self.value.text.len(), "", false);
        true
    }

    fn utf16_range(&self, range: Range<usize>) -> Range<usize> {
        self.value.text[..range.start].encode_utf16().count()
            ..self.value.text[..range.end].encode_utf16().count()
    }

    fn byte_range(&self, range: Range<usize>) -> Range<usize> {
        let start = byte_offset(&self.value.text, range.start);
        start..byte_offset(&self.value.text, range.end).max(start)
    }
}

// Native text services use UTF-16; editor state uses UTF-8 byte boundaries.
fn byte_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (offset, ch) in text.char_indices() {
        if units + ch.len_utf16() > utf16 {
            return offset;
        }
        units += ch.len_utf16();
    }
    text.len()
}

struct VisualRow {
    range: Range<usize>,
    line: ShapedLine,
}

pub struct Composer {
    focus: FocusHandle,
    draft: Draft,
    enabled: bool,
    reason: Option<String>,
    busy: bool,
    selecting: bool,
    rows: Vec<VisualRow>,
    bounds: Option<Bounds<Pixels>>,
    scroll: Pixels,
    reveal_cursor: bool,
    input_changed: bool,
    layout_revision: u64,
    paint_error: Option<String>,
}

impl Composer {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            draft: Draft::default(),
            enabled: false,
            reason: Some("Connect a local thread to send messages.".into()),
            busy: false,
            selecting: false,
            rows: Vec::new(),
            bounds: None,
            scroll: px(0.),
            reveal_cursor: true,
            input_changed: false,
            layout_revision: 0,
            paint_error: None,
        }
    }

    pub fn saved_draft(&self) -> (&str, u64, &Option<(String, u64)>) {
        (
            &self.draft.value.text,
            self.draft.revision,
            &self.draft.submitted,
        )
    }

    pub fn restore_draft(
        &mut self,
        text: String,
        revision: u64,
        submitted: Option<(String, u64)>,
        cx: &mut Context<Self>,
    ) {
        self.draft = Draft {
            value: Snapshot {
                anchor: text.len(),
                head: text.len(),
                text,
            },
            revision,
            submitted,
            ..Draft::default()
        };
        self.input_changed = true;
        self.reveal_cursor = true;
        cx.notify();
    }

    pub fn set_enabled(&mut self, enabled: bool, reason: Option<String>, cx: &mut Context<Self>) {
        if self.enabled == enabled && self.reason == reason {
            return;
        }
        self.enabled = enabled;
        self.reason = reason;
        if !enabled {
            self.draft.finish_composition();
        }
        self.input_changed = true;
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        if self.busy != busy {
            self.busy = busy;
            cx.notify();
        }
    }

    pub fn clear_if_unchanged(&mut self, submitted: &str, cx: &mut Context<Self>) -> bool {
        let cleared = self.draft.clear_if_unchanged(submitted);
        if cleared {
            self.reveal_cursor = true;
            self.input_changed = true;
            cx.notify();
        }
        cleared
    }

    fn changed(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.reveal_cursor = true;
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.draft.submit(self.enabled, self.busy) {
            cx.emit(ComposerEvent::Submit(text));
        }
    }

    fn edit(
        &mut self,
        operation: impl FnOnce(&mut Draft),
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.enabled {
            operation(&mut self.draft);
            self.changed(window, cx);
        }
    }

    fn move_to(&mut self, offset: usize, extend: bool, window: &Window, cx: &mut Context<Self>) {
        self.draft.finish_composition();
        self.draft.select(offset, extend);
        self.reveal_cursor = true;
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn horizontal(&mut self, forward: bool, extend: bool, window: &Window, cx: &mut Context<Self>) {
        let selection = self.draft.selection();
        let offset = if !extend && !selection.is_empty() {
            if forward {
                selection.end
            } else {
                selection.start
            }
        } else {
            self.draft.boundary(self.draft.value.head, forward)
        };
        self.move_to(offset, extend, window, cx);
    }

    fn word(&mut self, forward: bool, extend: bool, window: &Window, cx: &mut Context<Self>) {
        let head = self.draft.value.head;
        let words = self.draft.value.text.unicode_word_indices();
        let offset = if forward {
            words
                .map(|(i, word)| i + word.len())
                .find(|i| *i > head)
                .unwrap_or(self.draft.value.text.len())
        } else {
            words
                .map(|(i, _)| i)
                .take_while(|i| *i < head)
                .last()
                .unwrap_or(0)
        };
        self.move_to(offset, extend, window, cx);
    }

    fn cursor_position(&self) -> Point<Pixels> {
        self.position_for_offset(self.draft.value.head)
    }

    fn position_for_offset(&self, offset: usize) -> Point<Pixels> {
        let row = self
            .rows
            .iter()
            .rposition(|row| row.range.start <= offset)
            .unwrap_or(0);
        let x = self
            .rows
            .get(row)
            .map(|row| {
                row.line
                    .x_for_index(offset.min(row.range.end) - row.range.start)
            })
            .unwrap_or(px(0.));
        point(x, px(row as f32 * LINE_HEIGHT))
    }

    fn index_at(&self, position: Point<Pixels>) -> usize {
        let row = (position.y / px(LINE_HEIGHT)).floor().max(0.) as usize;
        self.rows
            .get(row)
            .map(|row| {
                self.draft
                    .snap(row.range.start + row.line.closest_index_for_x(position.x))
            })
            .unwrap_or(self.draft.value.text.len())
    }

    fn mouse_index(&self, position: Point<Pixels>) -> usize {
        self.bounds
            .map(|bounds| self.index_at(position - bounds.origin + point(px(0.), self.scroll)))
            .unwrap_or(0)
    }

    fn vertical(&mut self, down: bool, extend: bool, window: &Window, cx: &mut Context<Self>) {
        let mut position = self.cursor_position();
        position.y += px(if down { LINE_HEIGHT } else { -LINE_HEIGHT });
        let offset = if position.y < px(0.) {
            0
        } else {
            self.index_at(position)
        };
        self.move_to(offset, extend, window, cx);
    }

    fn line_edge(&mut self, end: bool, extend: bool, window: &Window, cx: &mut Context<Self>) {
        let row = self
            .rows
            .iter()
            .rfind(|row| row.range.start <= self.draft.value.head);
        let offset = row
            .map(|row| if end { row.range.end } else { row.range.start })
            .unwrap_or(0);
        self.move_to(offset, extend, window, cx);
    }

    fn copy(&self, cx: &mut App) {
        let selection = self.draft.selection();
        if !selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.draft.value.text[selection].into(),
            ));
        }
    }

    fn paste_clipboard(&mut self, window: &Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.edit(
                |draft| draft.insert(&text.replace("\r\n", "\n").replace('\r', "\n")),
                window,
                cx,
            );
        }
    }

    fn rectangles(&self, range: Range<usize>) -> Vec<Bounds<Pixels>> {
        let Some(bounds) = self.bounds else {
            return Vec::new();
        };
        if self.layout_revision != self.draft.revision {
            return Vec::new();
        }
        if range.is_empty() {
            return vec![Bounds::new(
                bounds.origin + self.position_for_offset(range.start) - point(px(0.), self.scroll),
                size(px(1.), px(LINE_HEIGHT)),
            )];
        }
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                if range.start > row.range.end || range.end <= row.range.start {
                    return None;
                }
                let start = range.start.max(row.range.start) - row.range.start;
                let end = range.end.min(row.range.end) - row.range.start;
                let x = row.line.x_for_index(start);
                let width = (row.line.x_for_index(end) - x).max(px(2.));
                Some(Bounds::new(
                    bounds.origin + point(x, px(index as f32 * LINE_HEIGHT) - self.scroll),
                    size(width, px(LINE_HEIGHT)),
                ))
            })
            .collect()
    }
}

impl Focusable for Composer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl EventEmitter<ComposerEvent> for Composer {}

impl EntityInputHandler for Composer {
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.enabled
    }

    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.draft.byte_range(range);
        *actual = Some(self.draft.utf16_range(range.clone()));
        Some(self.draft.value.text[range].into())
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        (self.enabled || ignore_disabled).then(|| UTF16Selection {
            range: self.draft.utf16_range(self.draft.selection()),
            reversed: self.draft.value.head < self.draft.value.anchor,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.draft.byte_range(range);
        self.draft.value.anchor = range.start;
        self.draft.value.head = range.end;
        if self.draft.marked.is_none() {
            self.draft.normalize_selection();
        }
        self.reveal_cursor = true;
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.draft.value.text.encode_utf16().count())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.draft
            .marked
            .clone()
            .map(|range| self.draft.utf16_range(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.finish_composition();
        self.changed(window, cx);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            |draft| {
                let range = range
                    .map(|range| draft.byte_range(range))
                    .or_else(|| draft.marked.clone())
                    .unwrap_or_else(|| draft.selection());
                draft.replace(range, text, false);
            },
            window,
            cx,
        );
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit(
            |draft| {
                let range = range
                    .map(|range| draft.byte_range(range))
                    .or_else(|| draft.marked.clone())
                    .unwrap_or_else(|| draft.selection());
                draft.mark(range, text, selected);
            },
            window,
            cx,
        );
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.rectangles(self.draft.byte_range(range))
            .into_iter()
            .next()
    }

    fn selection_bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.rectangles(self.draft.byte_range(range))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        if self.layout_revision != self.draft.revision {
            return None;
        }
        self.bounds?;
        let offset = self.mouse_index(point);
        Some(self.draft.utf16_range(0..offset).end)
    }
}

struct InputElement(Entity<Composer>);

impl Element for InputElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<String>;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = px(INPUT_HEIGHT).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        self.0.update(cx, |input, _| {
            let mut failure = None;
            input.bounds = Some(bounds);
            let content: SharedString = input.draft.value.text.clone().into();
            let run = TextRun {
                len: content.len(),
                font: style.font(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window.text_system().shape_text(
                content,
                font_size,
                std::slice::from_ref(&run),
                Some(bounds.size.width.max(px(1.))),
                None,
            );
            input.rows.clear();
            match shaped {
                Ok(lines) => {
                    let mut offset = 0;
                    for line in lines {
                        let mut start = 0;
                        let ends = line
                            .wrap_boundaries()
                            .iter()
                            .map(|boundary| {
                                line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index
                            })
                            .chain([line.len()]);
                        for end in ends {
                            let text: SharedString = line.text[start..end].to_owned().into();
                            let row_run = TextRun {
                                len: text.len(),
                                ..run.clone()
                            };
                            let shaped =
                                window
                                    .text_system()
                                    .shape_line(text, font_size, &[row_run], None);
                            input.rows.push(VisualRow {
                                range: offset + start..offset + end,
                                line: shaped,
                            });
                            start = end;
                        }
                        offset += line.len() + 1;
                    }
                }
                Err(error) => failure = Some(format!("Cannot display draft: {error}")),
            }
            input.layout_revision = input.draft.revision;
            if input.reveal_cursor {
                let y = input.cursor_position().y;
                if y < input.scroll {
                    input.scroll = y;
                }
                if y + px(LINE_HEIGHT) > input.scroll + bounds.size.height {
                    input.scroll = y + px(LINE_HEIGHT) - bounds.size.height;
                }
                input.reveal_cursor = false;
            }
            let max = (px(input.rows.len() as f32 * LINE_HEIGHT) - bounds.size.height).max(px(0.));
            input.scroll = input.scroll.clamp(px(0.), max);
            failure
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        failure: &mut Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.0.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.0.clone()), cx);
        self.0.update(cx, |input, cx| {
            if std::mem::take(&mut input.input_changed) {
                window.notify_text_input_changed(&input.focus);
            }
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                if input.draft.value.text.is_empty() {
                    let style = window.text_style();
                    let text: SharedString = "Write a message…".into();
                    let run = TextRun {
                        len: text.len(),
                        font: style.font(),
                        color: rgb(SUBTLE).into(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let placeholder = window.text_system().shape_line(
                        text,
                        style.font_size.to_pixels(window.rem_size()),
                        &[run],
                        None,
                    );
                    if let Err(error) = placeholder.paint(
                        bounds.origin,
                        px(LINE_HEIGHT),
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    ) {
                        *failure = Some(format!("Cannot display draft: {error}"));
                    }
                }
                for rect in input.rectangles(input.draft.selection()) {
                    if !input.draft.selection().is_empty() {
                        window.paint_quad(fill(rect, rgba(0x93b8f54d)));
                    }
                }
                for (index, row) in input.rows.iter().enumerate() {
                    let origin = bounds.origin
                        + point(px(0.), px(index as f32 * LINE_HEIGHT) - input.scroll);
                    if origin.y + px(LINE_HEIGHT) < bounds.top() || origin.y > bounds.bottom() {
                        continue;
                    }
                    if let Err(error) =
                        row.line
                            .paint(origin, px(LINE_HEIGHT), TextAlign::Left, None, window, cx)
                    {
                        *failure = Some(format!("Cannot display draft: {error}"));
                    }
                }
                if let Some(marked) = input.draft.marked.clone() {
                    for rect in input.rectangles(marked) {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(rect.left(), rect.bottom() - px(1.)),
                                size(rect.size.width, px(1.)),
                            ),
                            rgb(TEXT),
                        ));
                    }
                }
                if input.enabled
                    && input.focus.is_focused(window)
                    && input.draft.selection().is_empty()
                {
                    for rect in input.rectangles(input.draft.selection()) {
                        window.paint_quad(fill(rect, rgb(BLUE)));
                    }
                }
            });
            if input.paint_error != *failure {
                input.paint_error = failure.take();
                cx.notify();
            }
        });
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hint = self
            .paint_error
            .clone()
            .or_else(|| {
                if self.enabled {
                    Some(
                        if self.busy {
                            "Agent is working. You can edit the next message."
                        } else {
                            "Enter to send · Shift-Enter for a new line"
                        }
                        .into(),
                    )
                } else {
                    self.reason.clone()
                }
            })
            .unwrap_or_else(|| "Composer unavailable".into());
        let input = cx.entity().downgrade();
        let enabled = self.enabled;
        let can_send = self.draft.can_submit(enabled, self.busy);
        let can_stop = enabled && self.busy;
        let focused = self.focus.is_focused(window);
        // One raised box holds the draft and its controls; the box shows focus.
        theme::column()
            .w_full()
            .min_w_0()
            .rounded_xl()
            .border_1()
            .border_color(rgb(if focused { FOCUS_EDGE } else { EDGE }))
            .bg(rgb(RAIL))
            .child(
                div()
                    .id("composer-input")
                    .role(Role::MultilineTextInput)
                    .aria_label("Message to agent")
                    .aria_description(hint.clone())
                    .aria_value(self.draft.value.text.clone())
                    .aria_placeholder("Write a message…")
                    .on_a11y_action(AccessibleAction::SetValue, move |data, window, cx| {
                        if let Some(gpui::accesskit::ActionData::Value(text)) = data {
                            let _ = input.update(cx, |this, cx| {
                                if this.draft.set_external_value(text, this.enabled) {
                                    this.changed(window, cx);
                                }
                            });
                        }
                    })
                    .a11y_synthetic_children(move |builder| {
                        if !enabled {
                            builder.parent_node().set_disabled();
                            builder.parent_node().set_read_only();
                        }
                    })
                    .track_focus(&self.focus)
                    .tab_stop(true)
                    .key_context("PlaitComposer")
                    .cursor(CursorStyle::IBeam)
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_color(rgb(if self.enabled { TEXT } else { MUTED }))
                    .text_size(px(13.))
                    .line_height(px(LINE_HEIGHT))
                    .on_action(cx.listener(|this, _: &Submit, _, cx| this.submit(cx)))
                    .on_action(cx.listener(|this, _: &Newline, window, cx| {
                        if this.draft.marked.is_none() {
                            this.edit(|draft| draft.insert("\n"), window, cx);
                        }
                    }))
                    .on_action(cx.listener(|this, _: &Backspace, w, cx| {
                        this.edit(|d| d.delete(false), w, cx)
                    }))
                    .on_action(
                        cx.listener(|this, _: &Delete, w, cx| this.edit(|d| d.delete(true), w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &Left, w, cx| this.horizontal(false, false, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &Right, w, cx| this.horizontal(true, false, w, cx)),
                    )
                    .on_action(cx.listener(|this, _: &SelectLeft, w, cx| {
                        this.horizontal(false, true, w, cx)
                    }))
                    .on_action(cx.listener(|this, _: &SelectRight, w, cx| {
                        this.horizontal(true, true, w, cx)
                    }))
                    .on_action(
                        cx.listener(|this, _: &Up, w, cx| this.vertical(false, false, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &Down, w, cx| this.vertical(true, false, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectUp, w, cx| this.vertical(false, true, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectDown, w, cx| this.vertical(true, true, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &Home, w, cx| this.line_edge(false, false, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &End, w, cx| this.line_edge(true, false, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectHome, w, cx| {
                            this.line_edge(false, true, w, cx)
                        }),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectEnd, w, cx| this.line_edge(true, true, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &DocumentStart, w, cx| this.move_to(0, false, w, cx)),
                    )
                    .on_action(cx.listener(|this, _: &DocumentEnd, w, cx| {
                        this.move_to(this.draft.value.text.len(), false, w, cx)
                    }))
                    .on_action(
                        cx.listener(|this, _: &WordLeft, w, cx| this.word(false, false, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &WordRight, w, cx| this.word(true, false, w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectWordLeft, w, cx| {
                            this.word(false, true, w, cx)
                        }),
                    )
                    .on_action(
                        cx.listener(|this, _: &SelectWordRight, w, cx| {
                            this.word(true, true, w, cx)
                        }),
                    )
                    .on_action(cx.listener(|this, _: &SelectAll, w, cx| {
                        this.move_to(0, false, w, cx);
                        this.move_to(this.draft.value.text.len(), true, w, cx);
                    }))
                    .on_action(cx.listener(|this, _: &Copy, _, cx| this.copy(cx)))
                    .on_action(cx.listener(|this, _: &Cut, w, cx| {
                        if this.enabled {
                            this.copy(cx);
                            this.edit(|d| d.insert(""), w, cx);
                        }
                    }))
                    .on_action(cx.listener(|this, _: &Paste, w, cx| this.paste_clipboard(w, cx)))
                    .on_action(
                        cx.listener(|this, _: &Undo, w, cx| this.edit(|d| d.undo(false), w, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &Redo, w, cx| this.edit(|d| d.undo(true), w, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            window.focus(&this.focus, cx);
                            window.request_virtual_keyboard();
                            this.selecting = true;
                            let offset = this.mouse_index(event.position);
                            if event.click_count >= 3 {
                                this.move_to(0, false, window, cx);
                                this.move_to(this.draft.value.text.len(), true, window, cx);
                            } else if event.click_count == 2 {
                                let word = this
                                    .draft
                                    .value
                                    .text
                                    .split_word_bound_indices()
                                    .find(|(i, word)| *i <= offset && offset < *i + word.len())
                                    .map(|(i, word)| i..i + word.len())
                                    .unwrap_or(offset..offset);
                                this.move_to(word.start, false, window, cx);
                                this.move_to(word.end, true, window, cx);
                            } else {
                                this.move_to(offset, event.modifiers.shift, window, cx);
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                        if this.selecting {
                            this.move_to(this.mouse_index(event.position), true, window, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.selecting = false),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.selecting = false),
                    )
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                        this.scroll -= event.delta.pixel_delta(px(LINE_HEIGHT)).y;
                        this.reveal_cursor = false;
                        cx.stop_propagation();
                        cx.notify();
                    }))
                    .child(InputElement(cx.entity())),
            )
            .child(
                theme::row()
                    .justify_between()
                    .gap_3()
                    .pl_3()
                    .pr_2()
                    .pb_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(rgb(SUBTLE))
                            .child(hint),
                    )
                    .child(
                        // Send is the one affirmative control; Stop stays neutral.
                        theme::toned_icon_button(
                            "composer-send",
                            if self.busy { "stop" } else { "send" },
                            if self.busy { "Stop" } else { "Send" },
                            if can_send {
                                theme::Tone::Primary
                            } else {
                                theme::Tone::Filled
                            },
                        )
                        .rounded_lg()
                        .tab_stop(can_send || can_stop)
                        .a11y_synthetic_children(move |builder| {
                            if !can_send && !can_stop {
                                builder.parent_node().set_disabled();
                            }
                        })
                        .when(!can_send && !can_stop, |button| {
                            button.opacity(0.5).cursor_default()
                        })
                        .on_activate(cx, |this, _, cx| {
                            if this.enabled && this.busy {
                                cx.emit(ComposerEvent::Stop);
                            } else {
                                this.submit(cx);
                            }
                        }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Draft, Snapshot};

    #[test]
    fn multiline_editing_selection_and_history() {
        let mut draft = Draft::default();
        draft.insert("one\ntwo");
        draft.select(0, false);
        draft.select(3, true);
        draft.insert("first");
        assert_eq!(draft.value.text, "first\ntwo");
        draft.undo(false);
        assert_eq!(draft.value.text, "one\ntwo");
        assert_eq!(draft.selection(), 0..3);
        draft.undo(true);
        assert_eq!(draft.value.text, "first\ntwo");
        draft.insert("\n");
        assert_eq!(draft.value.text, "first\n\ntwo");
        draft.undo(false);
        draft.insert("!");
        draft.undo(true);
        assert_eq!(draft.value.text, "first!\ntwo");
    }

    #[test]
    fn deletion_and_movement_preserve_graphemes() {
        for grapheme in ["e\u{301}", "👩🏽‍💻", "🇺🇦", "क्‍ष"] {
            let mut draft = Draft::default();
            draft.insert(&format!("A{grapheme}Z"));
            assert_eq!(draft.boundary(1, true), 1 + grapheme.len());
            draft.select(1 + grapheme.len(), false);
            draft.delete(false);
            assert_eq!(draft.value.text, "AZ");
            draft.undo(false);
            draft.select(1, false);
            draft.delete(true);
            assert_eq!(draft.value.text, "AZ");
        }
    }

    #[test]
    fn insertion_that_joins_a_grapheme_keeps_cursor_at_its_end() {
        let mut draft = Draft::default();
        draft.insert("\u{301}Z");
        draft.select(0, false);
        draft.insert("e");
        assert_eq!(draft.value.head, "e\u{301}".len());
        draft.delete(false);
        assert_eq!(draft.value.text, "Z");
    }

    #[test]
    fn utf16_offsets_clamp_without_splitting_scalars() {
        let mut draft = Draft::default();
        draft.insert("A🙂é\n漢");
        assert_eq!(draft.byte_range(1..3), 1..5);
        assert_eq!(draft.byte_range(2..2), 1..1);
        assert_eq!(draft.byte_range(99..usize::MAX), 11..11);
        assert_eq!(draft.byte_range(std::ops::Range { start: 3, end: 1 }), 5..5);
        assert_eq!(draft.utf16_range(1..5), 1..3);
    }

    #[test]
    fn composition_is_one_undo_step_and_blocks_submit() {
        let mut draft = Draft::default();
        draft.insert("A");
        draft.replace(1..1, "n", true);
        assert!(draft.submit(true, false).is_none());
        draft.replace(1..2, "に", true);
        draft.replace(1..4, "日本", false);
        assert_eq!(draft.submit(true, false).as_deref(), Some("A日本"));
        draft.undo(false);
        assert_eq!(draft.value.text, "A");
        draft.undo(true);
        assert_eq!(draft.value.text, "A日本");
    }

    #[test]
    fn marked_selection_is_relative_utf16_and_updates_replace_the_mark() {
        let mut draft = Draft::default();
        draft.insert("A🙂Z");
        draft.mark(draft.byte_range(1..3), "é🙂", Some(1..3));
        assert_eq!(draft.value.text, "Aé🙂Z");
        assert_eq!(draft.marked, Some(1..7));
        assert_eq!(draft.selection(), 3..7);
        draft.mark(draft.marked.clone().unwrap(), "漢", Some(1..99));
        assert_eq!(draft.value.text, "A漢Z");
        assert_eq!(draft.selection(), 4..4);
        draft.finish_composition();
        draft.undo(false);
        assert_eq!(draft.value.text, "A🙂Z");
    }

    #[test]
    fn cancelled_composition_does_not_add_an_undo_step() {
        let mut draft = Draft::default();
        draft.insert("A");
        draft.submit(true, false);
        draft.mark(1..1, "に", None);
        draft.mark(1..4, "", None);
        draft.finish_composition();
        assert!(!draft.clear_if_unchanged("A"));
        draft.undo(false);
        assert!(draft.value.text.is_empty());
    }

    #[test]
    fn empty_composition_still_blocks_enter() {
        let mut draft = Draft::default();
        draft.insert("draft");
        draft.replace(5..5, "", true);
        assert!(draft.submit(true, false).is_none());
        draft.finish_composition();
        assert!(draft.submit(true, false).is_some());
    }

    #[test]
    fn submission_never_clears_and_rejects_blank_disabled_busy() {
        let mut draft = Draft::default();
        for text in ["", " \n\t", "\u{2003}"] {
            draft.replace(0..draft.value.text.len(), text, false);
            assert!(draft.submit(true, false).is_none());
        }
        draft.insert("hello");
        assert!(draft.submit(false, false).is_none());
        assert!(draft.submit(true, true).is_none());
        let submitted = draft.submit(true, false).unwrap();
        assert_eq!(draft.value.text, submitted);
        assert!(draft.clear_if_unchanged(&submitted));
        assert!(draft.value.text.is_empty());
        assert!(!draft.clear_if_unchanged(&submitted));
    }

    #[test]
    fn acceptance_checks_revision_not_just_text() {
        let mut draft = Draft::default();
        draft.insert("hello");
        let submitted = draft.submit(true, false).unwrap();
        draft.insert("!");
        draft.undo(false);
        assert_eq!(draft.value.text, submitted);
        assert!(!draft.clear_if_unchanged(&submitted));
        assert_eq!(draft.value.text, "hello");
        draft.submit(true, false);
        draft.select(0, false);
        assert!(draft.clear_if_unchanged("hello"));
    }

    #[test]
    fn restored_submission_keeps_the_edit_revision_guard() {
        let restore = |revision| Draft {
            value: Snapshot {
                text: "hello".into(),
                anchor: 5,
                head: 5,
            },
            revision,
            submitted: Some(("hello".into(), 3)),
            ..Draft::default()
        };
        let mut unchanged = restore(3);
        assert!(unchanged.clear_if_unchanged("hello"));
        let mut edited_then_restored = restore(5);
        assert!(!edited_then_restored.clear_if_unchanged("hello"));
        assert_eq!(edited_then_restored.value.text, "hello");
    }

    #[test]
    fn external_replacement_preserves_history_and_acceptance_guard() {
        let mut draft = Draft::default();
        draft.insert("submitted");
        draft.submit(true, false);
        assert!(draft.set_external_value("next\n🙂", true));
        assert_eq!(draft.value.text, "next\n🙂");
        draft.undo(false);
        assert_eq!(draft.value.text, "submitted");
        assert!(!draft.clear_if_unchanged("submitted"));
        draft.undo(true);
        assert_eq!(draft.value.text, "next\n🙂");
    }

    #[test]
    fn external_replacement_finishes_composition_as_a_separate_undo_step() {
        let mut draft = Draft::default();
        draft.insert("A");
        draft.mark(1..1, "日本", Some(0..2));
        draft.set_external_value("replacement", true);
        assert!(draft.marked.is_none());
        draft.undo(false);
        assert_eq!(draft.value.text, "A日本");
        draft.undo(false);
        assert_eq!(draft.value.text, "A");
    }

    #[test]
    fn disabled_external_replacement_preserves_draft_and_marked_text() {
        let mut draft = Draft::default();
        draft.insert("draft");
        draft.mark(5..5, "に", None);
        let revision = draft.revision;
        assert!(!draft.set_external_value("replacement", false));
        assert_eq!(draft.value.text, "draftに");
        assert_eq!(draft.marked, Some(5..8));
        assert_eq!(draft.revision, revision);
    }

    #[test]
    fn unrelated_acceptance_cannot_clear_draft() {
        let mut draft = Draft::default();
        draft.insert("hello");
        assert!(!draft.clear_if_unchanged("hello"));
        draft.submit(true, false);
        assert!(!draft.clear_if_unchanged("another message"));
        assert_eq!(draft.value.text, "hello");
    }
}
