//! A compact list of suggestions for a text field: files, commands, and later
//! models.
//!
//! `Completion` is plain state plus a render function. It never takes focus:
//! the owning field keeps the keyboard, calls `step` for Up and Down, and reads
//! `selected_value` when the user accepts. Rows report clicks and wheel
//! scrolling through one callback. The owner positions the returned element.

use crate::ui::theme::{self, ROW_HEIGHT, colors};
use gpui::{prelude::*, *};
use std::{ops::Range, rc::Rc};

/// Rows shown at once; longer lists scroll with the selection.
pub const VISIBLE_ROWS: usize = 8;

pub struct CompletionItem<T> {
    /// Primary text, such as a file name or command.
    pub label: SharedString,
    /// Quieter secondary text, such as a directory or description. May be empty.
    pub detail: SharedString,
    /// Byte offsets of matched characters in `label` and `detail`, emphasized when drawn.
    pub label_matches: Vec<usize>,
    pub detail_matches: Vec<usize>,
    /// What assistive technology reads for the row.
    pub name: SharedString,
    /// What the owner receives when the row is accepted.
    pub value: T,
}

/// What a pointer did to the list.
pub enum CompletionEvent {
    /// The row at this index was clicked.
    Pick(usize),
    /// The wheel moved this far; positive scrolls toward the end of the list.
    Scroll(Pixels),
}

type OnEvent = Rc<dyn Fn(CompletionEvent, &mut Window, &mut App)>;

pub struct Completion<T> {
    /// Accessible name of the list, such as "File suggestions".
    name: SharedString,
    items: Vec<CompletionItem<T>>,
    selected: usize,
    /// First visible row.
    first: usize,
    /// A one-line status under the rows: loading, no matches, a limit.
    note: Option<SharedString>,
    /// Wheel movement not yet large enough to scroll a whole row.
    wheel: Pixels,
}

impl<T> Completion<T> {
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            items: Vec::new(),
            selected: 0,
            first: 0,
            note: None,
            wheel: px(0.),
        }
    }

    /// Replace the suggestions and select the first, which is the best match.
    pub fn set_items(&mut self, items: Vec<CompletionItem<T>>) {
        self.items = items;
        self.selected = 0;
        self.first = 0;
        self.wheel = px(0.);
    }

    pub fn set_note(&mut self, note: Option<SharedString>) {
        self.note = note;
    }

    pub fn has_items(&self) -> bool {
        !self.items.is_empty()
    }

    /// Whether there is anything to draw.
    pub fn is_visible(&self) -> bool {
        self.has_items() || self.note.is_some()
    }

    pub fn item(&self, index: usize) -> Option<&CompletionItem<T>> {
        self.items.get(index)
    }

    pub fn selected_item(&self) -> Option<&CompletionItem<T>> {
        self.items.get(self.selected)
    }

    /// One-based position of the selection and the number of suggestions.
    pub fn position(&self) -> Option<(usize, usize)> {
        self.has_items()
            .then_some((self.selected + 1, self.items.len()))
    }

    /// Move the selection, wrapping at both ends, and keep it in view.
    pub fn step(&mut self, forward: bool) {
        let count = self.items.len();
        if count == 0 {
            return;
        }
        self.selected = if forward {
            (self.selected + 1) % count
        } else {
            (self.selected + count - 1) % count
        };
        self.first = self.first.clamp(
            self.selected.saturating_sub(VISIBLE_ROWS - 1),
            self.selected,
        );
    }

    /// Apply wheel or trackpad movement, a whole row at a time.
    pub fn scroll_by(&mut self, distance: Pixels) {
        self.wheel += distance;
        let rows = (self.wheel / px(ROW_HEIGHT)).trunc();
        self.wheel -= px(rows * ROW_HEIGHT);
        self.scroll(rows as isize);
    }

    /// Scroll the rows without moving the selection.
    pub fn scroll(&mut self, rows: isize) {
        let last_first = self.items.len().saturating_sub(VISIBLE_ROWS);
        self.first = self.first.saturating_add_signed(rows).min(last_first);
    }

    fn visible(&self) -> Range<usize> {
        self.first..self.items.len().min(self.first + VISIBLE_ROWS)
    }

    /// The list, or `None` when there are no rows and no note.
    pub fn render(
        &self,
        id: &'static str,
        on_event: impl Fn(CompletionEvent, &mut Window, &mut App) + 'static,
    ) -> Option<Stateful<Div>> {
        if !self.is_visible() {
            return None;
        }
        let on_event: OnEvent = Rc::new(on_event);
        let scroll = on_event.clone();
        let list = theme::column()
            .id(id)
            .role(Role::ListBox)
            .aria_label(self.name.clone())
            .w_full()
            .p_1()
            .rounded_lg()
            .border_1()
            .border_color(rgb(colors().edge))
            .bg(rgb(colors().rail))
            // The list floats over other content; clicks must not reach it.
            .occlude()
            .on_scroll(move |event: &ScrollEvent, window, cx| {
                let distance = -event.delta.pixel_delta(px(ROW_HEIGHT)).y;
                scroll(CompletionEvent::Scroll(distance), window, cx);
                cx.stop_propagation();
            })
            .children(
                self.visible()
                    .map(|index| self.row(id, index, on_event.clone())),
            )
            .when_some(self.note.clone(), |list, note| {
                list.child(
                    div()
                        .px_2()
                        .py_1()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(rgb(colors().subtle))
                        .child(text!(id = (id, usize::MAX), note)),
                )
            });
        Some(list)
    }

    fn row(&self, id: &'static str, index: usize, on_event: OnEvent) -> Stateful<Div> {
        let item = &self.items[index];
        let selected = index == self.selected;
        let click = on_event.clone();
        theme::row()
            .id((id, index))
            .role(Role::ListBoxOption)
            .aria_label(item.name.clone())
            .aria_selected(selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(self.items.len())
            .h(px(ROW_HEIGHT))
            .flex_shrink_0()
            .min_w_0()
            .px_2()
            .gap_2()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(13.))
            // Selection is neutral, like every other list in the app.
            .map(|row| {
                if selected {
                    row.bg(rgb(colors().selected))
                } else {
                    row.hover(|style| style.bg(rgba(colors().hover)))
                }
            })
            .child(
                div()
                    .flex_shrink_0()
                    .max_w(relative(0.7))
                    .truncate()
                    .text_color(rgb(theme::colors().muted))
                    .child(emphasized(&item.label, &item.label_matches)),
            )
            .when(!item.detail.is_empty(), |row| {
                row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(rgb(colors().subtle))
                        .child(emphasized(&item.detail, &item.detail_matches)),
                )
            })
            .on_click(move |_, window, cx| click(CompletionEvent::Pick(index), window, cx))
            .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                on_event(CompletionEvent::Pick(index), window, cx)
            })
    }
}

/// Text with its matched characters brighter and heavier. Emphasis is neutral:
/// accents are reserved for state.
fn emphasized(text: &SharedString, matches: &[usize]) -> StyledText {
    let style = HighlightStyle {
        color: Some(rgb(colors().text).into()),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..HighlightStyle::default()
    };
    StyledText::new(text.clone()).with_highlights(
        match_ranges(text, matches)
            .into_iter()
            .map(|range| (range, style)),
    )
}

/// Turns matched character offsets into merged byte ranges. Offsets that do
/// not start a character in `text` are ignored rather than trusted.
pub(super) fn match_ranges(text: &str, matches: &[usize]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for &offset in matches {
        let Some(ch) = text.get(offset..).and_then(|rest| rest.chars().next()) else {
            continue;
        };
        let end = offset + ch.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == offset => last.end = end,
            Some(last) if last.end > offset => {}
            _ => ranges.push(offset..end),
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    // Not a glob: GPUI exports its own `test` attribute.
    use super::{Completion, CompletionItem, VISIBLE_ROWS, match_ranges};
    use crate::ui::theme::ROW_HEIGHT;
    use gpui::{SharedString, px};

    fn list(count: usize) -> Completion<usize> {
        let mut completion = Completion::new("Test suggestions");
        completion.set_items(
            (0..count)
                .map(|value| CompletionItem {
                    label: format!("item {value}").into(),
                    detail: SharedString::default(),
                    label_matches: Vec::new(),
                    detail_matches: Vec::new(),
                    name: format!("item {value}").into(),
                    value,
                })
                .collect(),
        );
        completion
    }

    #[test]
    fn an_empty_list_draws_only_when_it_has_a_note() {
        let mut completion = list(0);
        assert!(!completion.is_visible() && !completion.has_items());
        assert!(completion.selected_item().is_none());
        assert_eq!(completion.position(), None);
        completion.step(true);
        completion.scroll(3);
        assert!(completion.visible().is_empty());
        completion.set_note(Some("Loading files…".into()));
        assert!(completion.is_visible() && !completion.has_items());
    }

    #[test]
    fn selection_wraps_and_stays_inside_the_visible_rows() {
        let mut completion = list(VISIBLE_ROWS + 4);
        assert_eq!(completion.selected_item().unwrap().value, 0);
        assert_eq!(completion.visible(), 0..VISIBLE_ROWS);
        for expected in 1..VISIBLE_ROWS + 4 {
            completion.step(true);
            assert_eq!(completion.selected_item().unwrap().value, expected);
            assert!(completion.visible().contains(&expected));
            assert_eq!(completion.visible().len(), VISIBLE_ROWS);
        }
        assert_eq!(completion.visible(), 4..VISIBLE_ROWS + 4);
        completion.step(true);
        assert_eq!(completion.position(), Some((1, VISIBLE_ROWS + 4)));
        assert_eq!(completion.visible(), 0..VISIBLE_ROWS);
        completion.step(false);
        assert_eq!(completion.selected_item().unwrap().value, VISIBLE_ROWS + 3);
        assert_eq!(completion.visible(), 4..VISIBLE_ROWS + 4);
        completion.step(false);
        assert_eq!(completion.visible(), 4..VISIBLE_ROWS + 4);
    }

    #[test]
    fn scrolling_is_clamped_and_new_items_reset_the_selection() {
        let mut completion = list(VISIBLE_ROWS + 2);
        completion.scroll(1);
        assert_eq!(completion.visible(), 1..VISIBLE_ROWS + 1);
        completion.scroll(99);
        assert_eq!(completion.visible(), 2..VISIBLE_ROWS + 2);
        completion.scroll(-99);
        assert_eq!(completion.visible(), 0..VISIBLE_ROWS);
        // Stepping brings a scrolled-away selection back into view.
        completion.scroll(2);
        completion.step(true);
        assert!(completion.visible().contains(&1));
        completion.step(true);
        completion.set_items(list(3).items);
        assert_eq!(completion.position(), Some((1, 3)));
        assert_eq!(completion.visible(), 0..3);
        assert_eq!(completion.item(2).unwrap().value, 2);
        assert!(completion.item(3).is_none());
        let mut short = list(2);
        short.scroll(5);
        assert_eq!(short.visible(), 0..2);
    }

    #[test]
    fn small_wheel_movements_accumulate_into_whole_rows() {
        let mut completion = list(VISIBLE_ROWS + 3);
        completion.scroll_by(px(ROW_HEIGHT * 0.6));
        assert_eq!(completion.visible().start, 0);
        completion.scroll_by(px(ROW_HEIGHT * 0.6));
        assert_eq!(completion.visible().start, 1);
        completion.scroll_by(px(ROW_HEIGHT * 1.9));
        assert_eq!(completion.visible().start, 3);
        completion.scroll_by(px(-ROW_HEIGHT * 2.2));
        assert_eq!(completion.visible().start, 1);
    }

    #[test]
    fn matched_offsets_become_merged_character_ranges() {
        assert_eq!(match_ranges("main.rs", &[0, 1, 2, 5]), [0..3, 5..6]);
        assert_eq!(match_ranges("日本語.md", &[0, 3, 9]), [0..6, 9..10]);
        // Out-of-range, mid-character, and repeated offsets are dropped.
        assert_eq!(match_ranges("é-x", &[1, 0, 0, 3, 7]), [0..2, 3..4]);
        assert!(match_ranges("", &[0]).is_empty());
    }
}
