//! Bounded presentation titles; the journal continues to own the full draft.
use super::{Plait, session::UNTITLED};
use gpui::*;
use plait_core::tabs::{
    ThreadTarget,
    row::{TabMetadata, TabState},
};

fn first_line(text: &str) -> &str {
    let text = text.trim_start();
    for (count, (end, ch)) in text.char_indices().enumerate() {
        if ch == '\n' {
            return text[..end].trim_end();
        }
        if count == 120 {
            return text[..end].trim_end();
        }
    }
    text.trim_end()
}

fn title(text: &str) -> String {
    let line = first_line(text);
    plait_core::display_text::escape(if line.is_empty() { UNTITLED } else { line })
}

fn rename(row: &mut TabMetadata, title: &str) {
    // The remainder is the already-derived project, machine and state description.
    row.hover.replace_range(..row.title.len(), title);
    row.title = title.into();
}

impl Plait {
    /// Runs at catalog/tab transitions, never from rendering or composer edits.
    pub(super) fn name_draft_rows(&mut self, rows: &mut [TabMetadata]) {
        let mut titles = std::collections::HashMap::new();
        self.tabs.draft_row = None;
        for (index, (tab, row)) in self.tabs.model.tabs().iter().zip(rows).enumerate() {
            let ThreadTarget::Draft { id, checkout } = tab.thread().target() else {
                continue;
            };
            let saved = self
                .session
                .data
                .as_ref()
                .and_then(|s| s.draft_tab(id, &checkout.root));
            let parked = self.tabs.lifecycle.drafts.get(id);
            let name = self.tabs.draft_titles.get(id).cloned().unwrap_or_else(|| {
                title(
                    parked
                        .map(|d| d.saved.text.as_str())
                        .or_else(|| saved.map(|d| d.draft.text.as_str()))
                        .unwrap_or_default(),
                )
            });
            rename(row, &name);
            titles.insert(id.clone(), name);
            if row.active {
                self.tabs.draft_row = Some((id.clone(), index));
            }
            let draft = if self.draft.id.as_ref() == Some(id) {
                Some(&self.draft)
            } else {
                parked
            };
            let (harness, model) = match draft {
                Some(draft) => (draft.harness, draft.model.as_ref()),
                None => (
                    saved.and_then(|d| d.settings.harness),
                    saved.and_then(|d| d.settings.model.as_ref()),
                ),
            };
            let harness = harness.unwrap_or_else(|| self.default_harness());
            let model_known = model.is_some() || self.default_model_for(harness).is_some();
            if row.state == TabState::Idle && model_known {
                row.word = "Ready";
            }
        }
        self.tabs.draft_titles = titles;
    }

    /// Observe only the mounted text, and change one cached row only when needed.
    pub(super) fn refresh_draft_title(&mut self, cx: &mut Context<Self>) {
        let Some(id) = &self.draft.id else {
            return;
        };
        let name = title(
            self.draft
                .composer
                .as_ref()
                .map(|c| c.read(cx).saved_draft().0)
                .unwrap_or(&self.draft.saved.text),
        );
        if self.tabs.draft_titles.get(id) == Some(&name) {
            return;
        }
        if let Some((owner, index)) = &self.tabs.draft_row
            && owner == id
            && let Some(row) = self.tabs.rows.get_mut(*index)
        {
            rename(row, &name);
        }
        self.tabs.draft_titles.insert(id.clone(), name);
        // Equal-width tabs do not need layout or overflow regrouping for a new title.
        if let Some(strip) = &self.tabs.strip {
            strip.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    pub(super) fn current_draft_title(&self) -> &str {
        self.draft
            .id
            .as_ref()
            .and_then(|id| self.tabs.draft_titles.get(id))
            .map(String::as_str)
            .unwrap_or(UNTITLED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn titles_use_the_first_nonblank_line_with_bounded_unicode_storage() {
        for (text, expected) in [
            ("", UNTITLED),
            (" \n\t\r\n", UNTITLED),
            (
                " \n\t  Name this draft  \r\nAnother line",
                "Name this draft",
            ),
            ("First\nSecond", "First"),
        ] {
            assert_eq!(title(text), expected);
        }
        let pasted = "🦀".repeat(1_000_000);
        assert_eq!(title(&pasted), "🦀".repeat(120));
        assert_eq!(title(&format!("{} end", "a".repeat(119))), "a".repeat(119));
        assert_eq!(title("Spoof\u{202e}"), "Spoof\\u202e");
    }
}
