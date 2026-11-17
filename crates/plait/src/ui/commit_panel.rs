//! Staging and committing from Changes.
//!
//! plait-core runs each write on the background executor and refreshes status
//! afterwards; this file asks for writes and shows how they went. A write is
//! refused while another is queued or running, so a double click is one write.

use super::{Plait, theme::*};
use gpui::{prelude::*, *};
use plait_core::{git::WriteOp, model::Change};
use std::path::PathBuf;

/// A change's paths for staging. A rename also names where it came from, so
/// both sides of it move together.
pub(super) fn change_paths(change: &Change) -> Vec<PathBuf> {
    let mut paths = vec![change.path.clone()];
    paths.extend(change.original.clone());
    paths
}

impl Plait {
    /// Ask for a write and start it if nothing else is running.
    pub(super) fn git_write(&mut self, op: WriteOp, cx: &mut Context<Self>) {
        if self.state.repository.request_write(op) {
            self.pump(cx);
        }
        cx.notify();
    }

    /// Stage or unstage one changed file.
    pub(super) fn stage_change(&mut self, index: usize, stage: bool, cx: &mut Context<Self>) {
        let Some(change) = self
            .state
            .repository
            .snapshot
            .ready()
            .and_then(|snapshot| snapshot.changes.get(index))
        else {
            return;
        };
        let paths = change_paths(change);
        self.git_write(
            if stage {
                WriteOp::Stage(paths)
            } else {
                WriteOp::Unstage(paths)
            },
            cx,
        );
    }

    /// Counted once per status read, not on every frame.
    fn staged_count(&self) -> usize {
        self.state.repository.staged()
    }

    fn commit_message_text(&self, cx: &App) -> String {
        self.commit_message.read(cx).text().trim_end().to_owned()
    }

    /// Why Commit is unavailable, in words, or `None` when it can run.
    fn commit_blocker(&self, cx: &App) -> Option<&'static str> {
        if self.state.repository.write.busy() {
            Some("Git is still working.")
        } else if self.state.repository.conflicts() > 0 {
            Some("Resolve the conflicts, then mark each file resolved.")
        } else if self.staged_count() == 0 {
            Some("Stage a change to commit it.")
        } else if self.commit_message.read(cx).text().trim().is_empty() {
            Some("Write a commit message.")
        } else {
            None
        }
    }

    pub(super) fn commit_staged(&mut self, cx: &mut Context<Self>) {
        if self.commit_blocker(cx).is_some() {
            return;
        }
        let message = self.commit_message_text(cx);
        self.commit_sent = Some(message.clone());
        self.git_write(WriteOp::Commit { message }, cx);
    }

    /// Clear the message once its commit lands, never on a failure, and not
    /// if it was edited while the commit ran.
    pub(super) fn after_git_write(&mut self, cx: &mut Context<Self>) {
        if self.state.repository.write.busy() {
            return;
        }
        let sent = self.commit_sent.take();
        if self.state.repository.write.committed.take().is_some()
            && sent.is_some_and(|sent| sent == self.commit_message_text(cx))
        {
            self.commit_message
                .update(cx, |field, cx| field.set_text("", cx));
        }
    }

    /// Put the cursor in the commit message, on the Changes tab.
    pub(super) fn focus_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = super::Workspace::Changes;
        window.focus(&self.commit_message.focus_handle(cx), cx);
        cx.notify();
    }

    /// A thread is running in this checkout, so its edits may be half done.
    fn running_here(&self) -> bool {
        let Some(root) = self.state.repository.snapshot.ready().map(|s| &s.root) else {
            return false;
        };
        self.host
            .threads
            .iter()
            .any(|thread| thread.status.busy() && &thread.checkout == root)
    }

    /// Stage-all controls, the message field, Commit, and the last result.
    pub(super) fn commit_box(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let write = &self.state.repository.write;
        let busy = write.busy();
        let staged = self.staged_count();
        let changed = self
            .state
            .repository
            .snapshot
            .ready()
            .map_or(0, |snapshot| snapshot.changes.len());
        let blocker = self.commit_blocker(cx);
        let note = write.note.clone();
        column()
            .id("commit-box")
            .role(Role::Group)
            .aria_label("Commit")
            .flex_shrink_0()
            .px_3()
            .py_2()
            .gap_2()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                row()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .child(match staged {
                                0 => "Nothing staged".to_owned(),
                                n => format!("{n} staged"),
                            }),
                    )
                    .child(
                        button("stage-all", "Stage all")
                            .disabled(busy || changed == staged)
                            .on_activate(cx, |this, _, cx| this.git_write(WriteOp::StageAll, cx)),
                    )
                    .child(
                        button("unstage-all", "Unstage all")
                            .disabled(busy || staged == 0)
                            .on_activate(cx, |this, _, cx| this.git_write(WriteOp::UnstageAll, cx)),
                    ),
            )
            .child(
                div()
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if this.commit_message.read(cx).is_composing() {
                            return;
                        }
                        // Plain Return only; modified Return is left to the field.
                        if event.keystroke.key == "enter" && !event.keystroke.modifiers.modified() {
                            this.commit_staged(cx);
                            cx.stop_propagation();
                        }
                    }))
                    .child(self.commit_message.clone()),
            )
            .child(
                toned_button("commit", "Commit", Tone::Primary)
                    .w_full()
                    .justify_center()
                    .aria_description(blocker.unwrap_or("Commit the staged changes. Hooks run."))
                    .disabled(blocker.is_some())
                    .on_activate(cx, |this, _, cx| this.commit_staged(cx)),
            )
            .when(self.running_here(), |el| {
                el.child(tag(
                    "A thread is running in this checkout; its edits may be unfinished.",
                    AMBER,
                ))
            })
            .when(busy, |el| el.child(tag("Git is working…", SUBTLE)))
            .when_some(note.filter(|_| !busy), |el, (note, failed)| {
                el.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(if failed { RED } else { SUBTLE }))
                        .child(note),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::change_paths;
    use plait_core::model::Change;
    use std::path::PathBuf;

    #[test]
    fn a_rename_stages_both_of_its_paths() {
        let mut change = Change {
            path: PathBuf::from("new.rs"),
            original: None,
            index: b'R',
            worktree: b' ',
        };
        assert_eq!(change_paths(&change), [PathBuf::from("new.rs")]);
        change.original = Some(PathBuf::from("old.rs"));
        assert_eq!(
            change_paths(&change),
            [PathBuf::from("new.rs"), PathBuf::from("old.rs")]
        );
    }
}
