//! Desktop recovery and save scheduling. Host execution remains independent.

use super::{Plait, composer::Composer};
use gpui::*;
use plait_core::{
    host::{MessageRole, ThreadSummary},
    host_state::HostState,
    session::{self, Draft, Journal, Session, Thread},
    state::project_name,
};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Default)]
pub(super) struct SessionState {
    pub required: bool,
    pub loading: bool,
    pub closing: bool,
    pub journal: Option<Arc<Journal>>,
    pub data: Option<Session>,
    pub note: Option<String>,
    restore_thread: Option<String>,
    save: Option<Task<()>>,
}

impl SessionState {
    pub fn ready(&self) -> bool {
        !self.closing && (!self.required || (!self.loading && self.journal.is_some()))
    }
}

impl Plait {
    pub fn restore_desktop(
        &mut self,
        requested: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                this.close_desktop(false, window, cx);
                false
            })
            .unwrap_or(true)
        });
        self.session.required = true;
        self.session.loading = true;
        self.session.note = Some("Restoring saved workspace…".into());
        self.sync_composer(cx);
        let epoch = self.host.epoch;
        let task = cx.background_executor().spawn(async move {
            let (directory, host) = session::locations()?;
            let (journal, data) = Journal::open(&directory, host)?;
            let path = requested
                .or_else(|| data.checkout.clone())
                .unwrap_or_else(|| PathBuf::from("."));
            let path = std::fs::canonicalize(&path)
                .or_else(|_| {
                    if path.is_absolute() {
                        Ok(path.clone())
                    } else {
                        std::env::current_dir().map(|root| root.join(&path))
                    }
                })
                .map_err(|error| format!("Cannot restore checkout: {error}"))?;
            Ok::<_, String>((journal, data, path))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.session.loading = false;
                match result {
                    Ok((journal, data, path)) => {
                        // This observer outlives the window. Closing it must flush the latest draft.
                        let quit_journal = journal.clone();
                        let app: &mut App = cx;
                        app.on_app_quit(move |cx| {
                            let journal = quit_journal.clone();
                            cx.background_executor().spawn(async move {
                                if let Err(error) = journal.flush() { eprintln!("Plait workspace: {error}"); }
                            })
                        }).detach();
                        for (id, thread) in &data.threads {
                            if let Some(pending) = &thread.pending {
                                this.host.submissions.restore(id.clone(), pending.clone());
                            }
                        }
                        let selected = data.selected.clone();
                        this.session.journal = Some(journal);
                        this.session.data = Some(data);
                        this.session.note = None;
                        for (id, composer) in &this.composers {
                            this.restore_composer(id, composer, cx);
                        }
                        // A folder chosen while disk state loaded wins over the saved folder.
                        if this.host.epoch == epoch {
                            this.change_checkout(path, window, cx);
                            this.session.restore_thread = selected;
                        }
                        this.capture_session(cx);
                    }
                    Err(error) => this.session.note = Some(format!("Workspace recovery unavailable: {error} Sending is disabled to protect saved work.")),
                }
                this.sync_composer(cx);
                cx.notify();
            });
        }).detach();
    }

    pub(super) fn close_desktop(
        &mut self,
        quit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session.closing {
            return;
        }
        self.capture_session(cx);
        let Some(journal) = self.session.journal.clone() else {
            if quit {
                cx.quit();
            } else {
                window.remove_window();
            }
            return;
        };
        self.session.closing = true;
        self.sync_composer(cx);
        let task = cx
            .background_executor()
            .spawn(async move { journal.flush() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    if quit {
                        cx.quit();
                    } else {
                        window.remove_window();
                    }
                }
                Err(error) => {
                    this.session.closing = false;
                    this.session.note = Some(format!(
                        "Window kept open because work could not be saved: {error}"
                    ));
                    this.sync_composer(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn change_checkout(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_session(cx);
        self.state.projects[0].name = project_name(&path);
        let epoch = self.host.epoch + 1;
        let submissions = std::mem::take(&mut self.host.submissions);
        self.host = HostState::new(path.clone());
        self.host.epoch = epoch;
        self.host.submissions = submissions;
        self.host_watch = None;
        self.session.restore_thread = None;
        self.settings_open = false;
        self.new_thread_open = false;
        self.file_scroll = UniformListScrollHandle::new();
        self.notice = None;
        self.state.repository.open(path);
        self.reconnect_host(window, cx);
        self.pump(cx);
        self.capture_session(cx);
    }

    pub(super) fn restore_thread_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.session.restore_thread.take()
            && self.host.selected.is_none()
            && let Some(summary) = self.host.threads.iter().find(|thread| thread.id == id)
            && self.saved_thread(summary).is_some()
        {
            self.select_host_thread(id, window, cx);
        }
    }

    pub(super) fn saved_thread(&self, summary: &ThreadSummary) -> Option<&Thread> {
        self.session
            .data
            .as_ref()?
            .threads
            .get(&summary.id)
            .filter(|saved| saved.checkout == summary.checkout && saved.model == summary.model)
    }

    pub(super) fn restore_composer(
        &self,
        id: &str,
        composer: &Entity<Composer>,
        cx: &mut Context<Self>,
    ) {
        if let Some(saved) = self
            .host
            .threads
            .iter()
            .find(|thread| thread.id == id)
            .and_then(|summary| self.saved_thread(summary))
        {
            let draft = saved.draft.clone();
            composer.update(cx, |composer, cx| {
                composer.restore_draft(draft.text, draft.revision, draft.submitted, cx)
            });
        }
    }

    pub(super) fn capture_session(&mut self, cx: &mut Context<Self>) {
        let Some(data) = &self.session.data else {
            return;
        };
        let mut next = data.clone();
        if self.host.checkout.is_absolute() {
            next.checkout = Some(self.host.checkout.clone());
        }
        next.selected = self
            .host
            .selected
            .clone()
            .or_else(|| self.session.restore_thread.clone());
        for summary in &self.host.threads {
            let Some(composer) = self.composers.get(&summary.id) else {
                continue;
            };
            let composer = composer.read(cx);
            let (text, revision, submitted) = composer.saved_draft();
            let title = next
                .threads
                .get(&summary.id)
                .map(|thread| thread.title.clone())
                .filter(|title| !title.is_empty())
                .or_else(|| {
                    self.host
                        .snapshot
                        .as_ref()
                        .filter(|snapshot| snapshot.id == summary.id)
                        .and_then(|snapshot| {
                            snapshot
                                .messages
                                .iter()
                                .find(|message| message.role == MessageRole::User)
                        })
                        .map(|message| session::title(&message.text))
                })
                .unwrap_or_default();
            next.threads.insert(
                summary.id.clone(),
                Thread {
                    checkout: summary.checkout.clone(),
                    model: summary.model.clone(),
                    title,
                    draft: Draft {
                        text: text.to_owned(),
                        revision,
                        submitted: submitted.clone(),
                    },
                    pending: self.host.submissions.pending(&summary.id).cloned(),
                },
            );
        }
        if *data == next {
            return;
        }
        if let Some(journal) = &self.session.journal {
            journal.queue(next.clone());
        }
        self.session.data = Some(next);
        self.schedule_session_save(cx);
    }

    fn schedule_session_save(&mut self, cx: &mut Context<Self>) {
        let Some(journal) = self.session.journal.clone() else {
            return;
        };
        let timer = cx.background_executor().timer(Duration::from_millis(250));
        self.session.save = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let result = cx
                .background_executor()
                .spawn(async move { journal.flush() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.session.note = result
                    .err()
                    .map(|error| format!("Workspace not saved: {error}"));
                cx.notify();
            });
        }));
    }

    pub(super) fn thread_title(&self, id: &str) -> String {
        self.session
            .data
            .as_ref()
            .and_then(|data| data.threads.get(id))
            .map(|thread| thread.title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| format!("Thread {}", id.chars().take(8).collect::<String>()))
    }

    pub(super) fn record_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.host.snapshot.as_ref() else {
            return;
        };
        let id = snapshot.id.clone();
        if let Some(pending) = self.host.submissions.pending(&id).cloned()
            && snapshot
                .messages
                .iter()
                .any(|message| message.id == format!("request-{}", pending.request_id))
        {
            self.host.submissions.finish(&id, &pending.request_id);
            if let Some(composer) = self.composers.get(&id) {
                composer.update(cx, |composer, cx| {
                    composer.clear_if_unchanged(&pending.text, cx);
                });
            }
        }
        let title_missing = self
            .session
            .data
            .as_ref()
            .and_then(|data| data.threads.get(&id))
            .is_none_or(|thread| thread.title.is_empty());
        let pending_changed = self
            .session
            .data
            .as_ref()
            .and_then(|data| data.threads.get(&id))
            .is_some_and(|thread| thread.pending.as_ref() != self.host.submissions.pending(&id));
        if title_missing || pending_changed {
            self.capture_session(cx);
        }
    }
}
