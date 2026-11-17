//! Staging and committing from Changes.
//!
//! plait-core runs each write on the background executor and refreshes status
//! afterwards; this file asks for writes and shows how they went. A write is
//! refused while another is queued or running, so a double click is one write.

use super::{Plait, theme::*};
use gpui::{prelude::*, *};
use plait_core::{
    git::WriteOp,
    model::{Change, Snapshot, Tracking},
};
use std::path::PathBuf;

/// What sending commits to the remote means for this branch right now.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Sync {
    pub label: String,
    /// The write the Push button runs, if the branch can push at all.
    pub op: Option<WriteOp>,
    /// Why that write is unavailable now, in words.
    pub blocker: Option<&'static str>,
    /// There is an upstream to fetch from.
    pub fetch: bool,
    /// Such as `origin/main`, for the accessible description.
    pub upstream: Option<String>,
}

/// Counts come from the status read; nothing here runs Git.
pub(super) fn sync(named: bool, tracking: Option<&Tracking>) -> Sync {
    let publish = |label: String| Sync {
        label,
        op: Some(WriteOp::Publish),
        blocker: None,
        fetch: false,
        upstream: None,
    };
    let Some(tracking) = tracking.filter(|_| named) else {
        return if named {
            publish("Not on the remote yet".into())
        } else {
            Sync {
                label: "No branch to push".into(),
                op: None,
                blocker: None,
                fetch: false,
                upstream: None,
            }
        };
    };
    let up = &tracking.upstream;
    if tracking.gone {
        return publish(format!("{up} was deleted"));
    }
    let upstream = Some(up.clone());
    let (ahead, behind) = (tracking.ahead, tracking.behind);
    let label = match (ahead, behind) {
        (0, 0) => "Up to date".to_owned(),
        (a, 0) => format!("{a} to push"),
        (0, b) => format!("{b} to pull"),
        (a, b) => format!("{a} to push, {b} to pull"),
    };
    let blocker = if ahead == 0 {
        Some("Nothing to push.")
    } else if behind > 0 {
        Some("The remote has commits this branch does not. Pull them from Terminal first.")
    } else {
        None
    };
    Sync {
        label,
        op: Some(WriteOp::Push),
        blocker,
        fetch: true,
        upstream,
    }
}

fn sync_of(snapshot: &Snapshot) -> Sync {
    sync(snapshot.named, snapshot.tracking.as_ref())
}

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

    /// Push, or publish a branch the remote does not have yet.
    pub(super) fn push(&mut self, cx: &mut Context<Self>) {
        let Some(sync) = self.state.repository.snapshot.ready().map(|s| sync_of(s)) else {
            return;
        };
        if let (Some(op), None) = (sync.op, sync.blocker) {
            self.git_write(op, cx);
        }
    }

    /// The branch against its upstream, with Fetch and Push or Publish.
    fn sync_row(&self, busy: bool, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let sync = sync_of(self.state.repository.snapshot.ready()?);
        let publish = sync.op == Some(WriteOp::Publish);
        Some(
            row()
                .gap_1()
                .child(
                    div()
                        .id("sync-state")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .when_some(sync.upstream, |el, up| {
                            el.aria_description(format!("Upstream {up}"))
                        })
                        .child(sync.label),
                )
                .when(sync.fetch, |el| {
                    el.child(
                        button("fetch", "Fetch")
                            .aria_description("Fetch from the remote. Nothing local changes.")
                            .disabled(busy)
                            .on_activate(cx, |this, _, cx| this.git_write(WriteOp::Fetch, cx)),
                    )
                })
                .when(sync.op.is_some(), |el| {
                    el.child(
                        button("push", if publish { "Publish" } else { "Push" })
                            .aria_description(sync.blocker.unwrap_or(if publish {
                                "Push this branch to its remote and track it there."
                            } else {
                                "Push commits to the remote. Hooks run."
                            }))
                            .disabled(busy || sync.blocker.is_some())
                            .on_activate(cx, |this, _, cx| this.push(cx)),
                    )
                }),
        )
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
            .children(self.sync_row(busy, cx))
            .child(
                row()
                    .gap_1()
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
                // The count lives on the button, where it fits at any width.
                toned_button(
                    "commit",
                    match staged {
                        0 => "Commit".to_owned(),
                        1 => "Commit 1 file".to_owned(),
                        n => format!("Commit {n} files"),
                    },
                    Tone::Primary,
                )
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
    use super::{change_paths, sync};
    use plait_core::{
        git::WriteOp,
        model::{Change, Tracking},
    };
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

    #[test]
    fn push_follows_the_branch_and_its_upstream() {
        let up = |ahead, behind, gone| Tracking {
            upstream: "origin/main".into(),
            ahead,
            behind,
            gone,
        };
        let none = sync(false, None);
        assert_eq!((none.op, none.fetch), (None, false));
        let local = sync(true, None);
        assert_eq!(local.op, Some(WriteOp::Publish));
        assert_eq!(sync(true, Some(&up(0, 0, true))).op, Some(WriteOp::Publish));
        let even = sync(true, Some(&up(0, 0, false)));
        assert_eq!(even.label, "Up to date");
        assert_eq!(even.upstream.as_deref(), Some("origin/main"));
        assert!(even.blocker.is_some() && even.fetch);
        let ahead = sync(true, Some(&up(2, 0, false)));
        assert_eq!(ahead.label, "2 to push");
        assert_eq!((ahead.op, ahead.blocker), (Some(WriteOp::Push), None));
        let diverged = sync(true, Some(&up(1, 3, false)));
        assert_eq!(diverged.label, "1 to push, 3 to pull");
        assert!(diverged.blocker.unwrap().contains("Pull"));
    }
}
