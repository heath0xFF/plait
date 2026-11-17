//! Optional per-thread worktrees: start a thread in a fresh checkout of the
//! current project, and remove that checkout when its work is done.

use super::Plait;
use gpui::*;
use plait_core::git;
use std::path::PathBuf;

impl Plait {
    /// Create a worktree of the current project on a new branch, then start a
    /// thread there with the default model. The worktree's threads list under
    /// the project, and Changes follows them into the worktree.
    pub(super) fn new_worktree_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_overlay(true, window, cx);
        if self.worktree_busy {
            return;
        }
        // An unconfirmed creation is retried where it was asked for.
        if self.host.creating || self.host.pending_create.is_some() {
            return self.new_thread(None, window, cx);
        }
        if !self.host.connected || self.host.protocol() < 3 {
            self.notice = Some("Connect to the host to start a thread in a worktree.".into());
            return cx.notify();
        }
        let choice = self
            .host
            .status
            .as_ref()
            .and_then(|status| status.default_model.clone())
            .filter(|choice| self.host.available(Some(choice)));
        let Some(choice) = choice else {
            self.notice =
                Some("Choose a default model in Settings to start threads in a worktree.".into());
            return cx.notify();
        };
        let main = self.active_project().checkout.clone();
        self.worktree_busy = true;
        self.notice = None;
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { git::add_worktree(&main).map(|tree| (main, tree)) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.worktree_busy = false;
                match result {
                    Ok((main, tree)) => {
                        let mut value = this.preferences.value.clone();
                        value
                            .navigation
                            .remember_worktree(tree.path.clone(), main, tree.branch);
                        this.persist_navigation(value, cx);
                        this.change_checkout(tree.path, window, cx);
                        this.new_thread(Some(choice), window, cx);
                    }
                    Err(error) => {
                        this.notice = Some(format!("Worktree not created. {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Ask, then remove a Plait worktree. Git refuses one with uncommitted
    /// work; the branch stays, and so do the threads that ran there.
    pub(super) fn remove_worktree(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigation.menu = None;
        let Some(tree) = self.preferences.value.navigation.worktree(&path).cloned() else {
            return;
        };
        if self.worktree_busy {
            return;
        }
        let working = self
            .host
            .threads
            .iter()
            .any(|thread| thread.checkout == path && thread.status.busy());
        if working {
            self.navigation.error = Some("Stop the thread working in this worktree first.".into());
            return cx.notify();
        }
        let detail = format!(
            "This deletes {}. The branch {} and its commits stay. Git refuses if the worktree has uncommitted changes.",
            path.display(),
            tree.branch
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            "Remove this worktree?",
            Some(&detail),
            &[
                PromptButton::Other("Remove".into()),
                PromptButton::Cancel("Cancel".into()),
            ],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let main = tree.main.clone();
            let target = path.clone();
            let Ok(task) = this.update(cx, |this, cx| {
                this.worktree_busy = true;
                cx.notify();
                cx.background_executor()
                    .spawn(async move { git::remove_worktree(&main, &target) })
            }) else {
                return;
            };
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.worktree_busy = false;
                match result {
                    // Its folder is gone; show the project it came from.
                    Ok(()) if this.host.checkout == path => {
                        this.change_checkout(tree.main, window, cx)
                    }
                    Ok(()) => this.navigation.available.clear(),
                    Err(error) => this.navigation.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
