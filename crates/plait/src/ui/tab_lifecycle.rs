//! Only the selected tab owns a GPUI workspace. Parking is a save boundary.
use super::{Plait, composer::ParkedEditor, tabs::TabNavigation};
use gpui::*;
use plait_core::tabs::{
    TabId, ThreadTarget,
    bookmark::{self, BookmarkRecord, ThreadBookmark},
};
use std::sync::atomic::{AtomicU64, Ordering};

const EDITOR_LIMIT: usize = 8 * 1024 * 1024;
static RECORD: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(super) enum Intent {
    Open(ThreadTarget),
    Activate(TabId),
    Close(TabId),
    Navigate(TabNavigation),
}
#[derive(Clone)]
pub(super) enum EditorRecord {
    Memory(Box<ParkedEditor>),
    Stored(String),
}
#[derive(Default)]
pub(super) struct Lifecycle {
    pub busy: bool,
    pub editors: Vec<(ThreadTarget, EditorRecord)>,
}

impl Plait {
    pub(super) fn request_tab(
        &mut self,
        intent: Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.lifecycle.busy {
            return;
        }
        let unchanged = match &intent {
            Intent::Close(id) => self.tabs.model.active() != Some(*id),
            Intent::Activate(id) => self.tabs.model.active() == Some(*id),
            Intent::Open(target) => self
                .tabs
                .model
                .active()
                .and_then(|id| self.tabs.model.get(id))
                .is_some_and(|tab| tab.thread().target() == target),
            Intent::Navigate(_) => false,
        };
        if unchanged {
            self.perform_tab(intent, window, cx);
            return;
        }
        let Some(active) = self.tabs.model.active() else {
            self.perform_tab(intent, window, cx);
            return;
        };
        let target = self
            .tabs
            .model
            .get(active)
            .unwrap()
            .thread()
            .target()
            .clone();
        let editor = self
            .visible_composer()
            .cloned()
            .map(|c| c.update(cx, |c, _| c.park_editor()));
        self.capture_session(cx);
        let bookmark = self.capture_tab_bookmark(Some(window), cx);
        let record = match BookmarkRecord::capture(&bookmark) {
            Ok(record) => record,
            Err(bookmark::BookmarkError::NeedsStorage) if self.session.journal.is_some() => {
                BookmarkRecord::stored(bookmark.view, record_name("view")).expect("generated name")
            }
            Err(_) => {
                self.notice = Some("Tab kept open: its view could not be saved.".into());
                cx.notify();
                return;
            }
        };
        let Some(journal) = self.session.journal.clone() else {
            let memory: usize = self
                .tabs
                .lifecycle
                .editors
                .iter()
                .filter(|(old, _)| old != &target)
                .map(|(_, e)| match e {
                    EditorRecord::Memory(e) => e.retained_bytes(),
                    EditorRecord::Stored(_) => 0,
                })
                .sum();
            if memory + editor.as_ref().map_or(0, ParkedEditor::retained_bytes) > EDITOR_LIMIT {
                self.notice = Some("Tab kept open: private editor storage is unavailable.".into());
                cx.notify();
                return;
            }
            if let Some(editor) = editor {
                self.remember_editor(target, EditorRecord::Memory(Box::new(editor)), cx);
            }
            let result = self.tabs.model.save_bookmark(active, record);
            if self.tab_error(result) {
                self.perform_tab(intent, window, cx);
            }
            return;
        };
        self.tabs.lifecycle.busy = true;
        let saved = bookmark.clone();
        let saved_record = record.clone();
        let task = cx.background_executor().spawn(async move {
            journal.flush()?;
            if let BookmarkRecord::Stored { blob, .. } = &saved_record {
                let bytes =
                    bookmark::encode_extended(&saved).map_err(|_| "Tab bookmark is too large.")?;
                journal.write_record_bytes(blob, &bytes, bookmark::EXTENDED_BOOKMARK_LIMIT)?;
            }
            let editor = editor
                .map(|editor| {
                    let name = record_name("editor");
                    journal
                        .write_record(&name, &editor, EDITOR_LIMIT)
                        .map(|()| name)
                })
                .transpose()?;
            Ok::<_, String>(editor)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.tabs.lifecycle.busy = false;
                match result {
                    Err(error) => {
                        this.notice = Some(format!(
                            "Tab kept open because work could not be saved: {error}"
                        ));
                        cx.notify();
                    }
                    Ok(editor) => {
                        // Edits made while flushing belong to a newer save boundary.
                        if this.tabs.model.active() != Some(active) {
                            return;
                        }
                        if this.capture_tab_bookmark(Some(window), cx) != bookmark {
                            this.request_tab(intent, window, cx);
                            return;
                        }
                        if let Some(editor) = editor {
                            this.remember_editor(target, EditorRecord::Stored(editor), cx);
                        }
                        let result = this.tabs.model.save_bookmark(active, record);
                        if this.tab_error(result) {
                            this.perform_tab(intent, window, cx);
                        }
                    }
                }
            });
        })
        .detach();
    }

    fn remember_editor(
        &mut self,
        target: ThreadTarget,
        editor: EditorRecord,
        cx: &mut Context<Self>,
    ) {
        let retired: Vec<_> = self
            .tabs
            .lifecycle
            .editors
            .iter()
            .filter_map(|(old, editor)| match editor {
                EditorRecord::Stored(name) if old == &target => Some(name.clone()),
                _ => None,
            })
            .collect();
        self.tabs
            .lifecycle
            .editors
            .retain(|(old, _)| old != &target);
        self.tabs.lifecycle.editors.push((target, editor));
        if let Some(journal) = self.session.journal.clone() {
            cx.background_executor()
                .spawn(async move {
                    for name in retired {
                        let _ = journal.remove_record(&name);
                    }
                })
                .detach();
        }
    }

    fn perform_tab(&mut self, intent: Intent, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.tabs.model.active();
        let connection = self
            .host
            .connection
            .connected
            .then_some(self.host.connection.epoch);
        let result = match intent {
            Intent::Open(target) => self.tabs.model.open(target, connection).map(|_| ()),
            Intent::Activate(id) => self.tabs.model.activate(id),
            Intent::Close(id) => {
                let nonempty = self.tabs.model.get(id).is_some_and(|tab| {
                    let ThreadTarget::Draft { checkout, .. } = tab.thread().target() else {
                        return false;
                    };
                    if previous == Some(id) {
                        self.draft
                            .composer
                            .as_ref()
                            .is_some_and(|c| !c.read(cx).is_empty())
                    } else {
                        self.session
                            .data
                            .as_ref()
                            .and_then(|s| s.new_draft_for(&checkout.root))
                            .is_some_and(|d| !d.text.is_empty() || !d.images.is_empty())
                    }
                });
                self.tabs.model.close(id, nonempty)
            }
            Intent::Navigate(action) => match action {
                TabNavigation::Next => self.tabs.model.next_tab(),
                TabNavigation::Previous => self.tabs.model.previous_tab(),
                TabNavigation::Jump(n) => self.tabs.model.jump(n),
                TabNavigation::Reopen => self.tabs.model.reopen(|target| match target {
                    ThreadTarget::Existing { id, checkout } => self
                        .host
                        .connection
                        .threads
                        .iter()
                        .any(|t| &t.id == id && t.checkout == checkout.root)
                        .then_some(connection),
                    ThreadTarget::Draft { checkout, .. } => self
                        .session
                        .data
                        .as_ref()
                        .and_then(|s| s.new_draft_for(&checkout.root))
                        .map(|_| connection),
                }),
            }
            .map(|_| ()),
        };
        if self.tab_error(result) {
            if previous != self.tabs.model.active() {
                self.park_workspace(cx);
                self.present_tab(window, cx);
            }
            self.tabs.list_open = false;
            self.refresh_tab_rows();
            self.retire_editor_cache(cx);
            cx.notify();
        }
    }

    fn park_workspace(&mut self, cx: &mut Context<Self>) {
        self.workspace.cancelled.store(true, Ordering::Release);
        self.workspace.host_watch = None;
        self.host.select(None);
        let field = self.workspace.commit_message.clone();
        field.update(cx, |f, cx| f.set_text("", cx));
        // Configuration acknowledgements are keyed operations, not view payload.
        let controls = std::mem::take(&mut self.workspace.thread_controls);
        let review_generation = self.workspace.turn_review.generation + 1;
        self.workspace = super::thread_workspace::ThreadWorkspace::new(field);
        self.workspace.turn_review.generation = review_generation;
        self.workspace.thread_controls = controls;
        self.workspace.thread_controls.usage_open = false;
        self.draft.composer = None;
        self.draft.open = false;
        self.conversation_focus = None;
        self.conversation_find_focus = None;
        self.state.repository.park_view();
    }

    pub(super) fn hydrate_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .model
            .active()
            .and_then(|id| self.tabs.model.get(id))
        else {
            return;
        };
        let lifetime = tab.lifetime();
        let owner = self.host.read_stamp();
        let target = tab.thread().target().clone();
        let record = tab.thread().bookmark().clone();
        let editor = self
            .tabs
            .lifecycle
            .editors
            .iter()
            .find(|(t, _)| t == &target)
            .map(|(_, e)| e.clone());
        if matches!(record, BookmarkRecord::Inline { .. })
            && !matches!(editor, Some(EditorRecord::Stored(_)))
        {
            if let Some(EditorRecord::Memory(editor)) = editor {
                self.install_editor(*editor, cx);
            }
            self.restore_tab_bookmark(record.inline().unwrap_or_default(), window, cx);
            return;
        }
        let Some(journal) = self.session.journal.clone() else {
            return;
        };
        let task = cx.background_executor().spawn(async move {
            let bookmark = match record {
                BookmarkRecord::Inline { .. } => record.inline(),
                BookmarkRecord::Stored { blob, .. } => journal
                    .read_record_bytes(&blob, bookmark::EXTENDED_BOOKMARK_LIMIT)?
                    .and_then(|bytes| bookmark::decode_extended(&bytes)),
            }
            .ok_or("Saved tab view is unavailable; showing its default view.")?;
            let editor = match editor {
                Some(EditorRecord::Stored(name)) => journal.read_record(&name, EDITOR_LIMIT),
                Some(EditorRecord::Memory(editor)) => Ok(Some(*editor)),
                None => Ok(None),
            };
            Ok::<_, String>((bookmark, editor))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.tabs.model.active() != Some(lifetime.tab)
                    || !this.host.accepts_read(&owner)
                    || this
                        .tabs
                        .model
                        .get(lifetime.tab)
                        .is_none_or(|tab| tab.lifetime() != lifetime)
                {
                    return;
                }
                match result {
                    Ok((bookmark, editor)) => {
                        match editor {
                            Ok(Some(editor)) => this.install_editor(editor, cx),
                            Ok(None) => {}
                            Err(_) => this.notice = Some(
                                "The editor history was unavailable. Your saved draft is intact."
                                    .into(),
                            ),
                        }
                        this.restore_tab_bookmark(bookmark, window, cx);
                    }
                    Err(error) => {
                        this.notice = Some(error);
                        this.restore_tab_bookmark(ThreadBookmark::default(), window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn install_editor(&mut self, editor: ParkedEditor, cx: &mut Context<Self>) {
        if editor.retained_bytes() > EDITOR_LIMIT {
            self.notice =
                Some("The editor history exceeded its limit. Your saved draft is intact.".into());
            return;
        }
        if let Some(composer) = self.visible_composer() {
            composer.update(cx, |c, cx| c.restore_editor(editor, cx));
        }
    }

    fn retire_editor_cache(&mut self, cx: &mut Context<Self>) {
        let mut removed = Vec::new();
        self.tabs.lifecycle.editors.retain(|(target, editor)| {
            if self.tabs.model.retains_target(target) {
                return true;
            }
            if let EditorRecord::Stored(name) = editor {
                removed.push(name.clone());
            }
            false
        });
        if let Some(journal) = self.session.journal.clone() {
            cx.background_executor()
                .spawn(async move {
                    for name in removed {
                        let _ = journal.remove_record(&name);
                    }
                })
                .detach();
        }
    }
}

fn record_name(kind: &str) -> String {
    format!(
        "{kind}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        RECORD.fetch_add(1, Ordering::Relaxed)
    )
}
