//! Host-owned run records, their guarded reads, and undo at the closing line.
use super::{Plait, Workspace, theme::*, thread_controls::TextTooltip};
use gpui::{prelude::*, *};
use plait_core::{
    git::record::Uncovered,
    history::records::{self, Availability, Line, Lines},
    host::{self, Client, Request, TurnRecord, UndoState, UndoTurnReply},
    host_state::request_id,
    transcript,
};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    pub(super) thread: String,
    pub(super) epoch: u64,
    pub(super) watch: u64,
    pub(super) revision: Option<u64>,
    pub(super) generation: u64,
}
#[derive(Clone, Debug)]
pub(super) struct Attempt {
    run: String,
    target: String,
    request: String,
    leave_changed: bool,
}
pub(super) struct Blocked {
    run: String,
    reply: UndoTurnReply,
    coverage: Vec<SharedString>,
    changed: Vec<SharedString>,
    omitted: usize,
    changed_omitted: usize,
    host_omitted: usize,
}
impl Blocked {
    pub(super) fn new(run: String, reply: UndoTurnReply, coverage: &[Uncovered]) -> Self {
        let host_omitted = reply
            .not_covered_total
            .saturating_sub(reply.not_covered.len());
        let coverage: Vec<_> = coverage
            .iter()
            .chain(&reply.not_covered)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let omitted = coverage.len().saturating_sub(records::PREVIEW_LIMIT);
        let coverage = coverage
            .iter()
            .take(records::PREVIEW_LIMIT)
            .map(|item| {
                format!(
                    "{} — {}",
                    records::preview_name(&item.path),
                    records::coverage_reason(item.reason)
                )
                .into()
            })
            .collect();
        let changed: Vec<SharedString> = reply
            .changed_since
            .iter()
            .take(records::PREVIEW_LIMIT)
            .map(|path| records::preview_name(&path.to_string_lossy()).into())
            .collect();
        let changed_omitted = reply.changed_since_total.saturating_sub(changed.len());
        Self {
            run,
            reply,
            coverage,
            changed,
            omitted,
            changed_omitted,
            host_omitted,
        }
    }
}
#[derive(Default)]
pub(super) struct TurnRecords {
    pub(super) stamp: Option<Stamp>,
    pub data: Arc<Vec<TurnRecord>>,
    pub lines: Arc<Lines>,
    pub(super) generation: u64,
    pub(super) data_generation: u64,
    pub(super) projected_generation: u64,
    pub(super) loaded: bool,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    pub(super) pending: Option<Attempt>,
    pub(super) retry: Option<Attempt>,
    pub(super) blocked: Option<Blocked>,
    pub(super) note: Option<(String, &'static str, String)>,
}
impl Plait {
    fn record_stamp(&self) -> Option<Stamp> {
        let snapshot = self.host.snapshot.as_ref()?;
        (self.host.selected.as_deref() == Some(snapshot.id.as_str())).then(|| Stamp {
            thread: snapshot.id.clone(),
            epoch: self.host.epoch,
            watch: self.host.watch_epoch,
            revision: snapshot.records_revision,
            generation: self.turn_records.generation,
        })
    }
    pub(super) fn records_current(&self) -> bool {
        self.host.protocol() >= 5
            && self.turn_records.loaded
            && self.turn_records.projected_generation == self.turn_records.generation
            && self.turn_records.stamp == self.record_stamp()
    }
    pub(super) fn turn_records_snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stamp) = self.record_stamp() else {
            return;
        };
        if self.turn_records.stamp.as_ref() == Some(&stamp) {
            return;
        }
        let same_thread = self.turn_records.stamp.as_ref().is_some_and(|old| {
            old.thread == stamp.thread && old.epoch == stamp.epoch && old.watch == stamp.watch
        });
        if !same_thread {
            let generation = self.turn_records.generation + 1;
            self.turn_records = TurnRecords {
                generation,
                ..Default::default()
            };
        }
        if self.host.protocol() < 5 {
            return;
        }
        self.fetch_turn_records(window, cx);
    }
    pub(super) fn fetch_turn_records(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.host.connected || self.host.protocol() < 5 {
            return;
        }
        self.turn_records.generation += 1;
        let Some(stamp) = self.record_stamp() else {
            return;
        };
        self.turn_records.stamp = Some(stamp.clone());
        self.turn_records.loading = true;
        self.turn_records.loaded = false;
        self.turn_records.error = None;
        let thread = stamp.thread.clone();
        let task = cx.background_executor().spawn(async move {
            records::read(&Client::from_environment()?, &thread).map(Arc::new)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.record_stamp().as_ref() != Some(&stamp) {
                    return;
                }
                this.turn_records.loading = false;
                match result {
                    Ok(records) => {
                        this.record_thread_files(&records, cx);
                        this.turn_records.data = records;
                        this.turn_records.data_generation = stamp.generation;
                        this.turn_records.loaded = true;
                        this.conversation_changed(window, cx);
                    }
                    Err(error) => this.turn_records.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Called with the background conversation projection, never from render.
    pub(super) fn install_record_lines(&mut self, lines: Arc<Lines>, generation: u64) {
        self.turn_records.projected_generation = generation;
        let old = std::mem::replace(&mut self.turn_records.lines, lines);
        let Some(snapshot) = self.conversation_snapshot() else {
            return;
        };
        for (index, slot) in self.live_layout.iter().enumerate() {
            if let Some(key) = transcript::resolve(snapshot, *slot).and_then(records::closing_key)
                && (old.rows.get(&key) != self.turn_records.lines.rows.get(&key)
                    || old.covered.contains(&key) != self.turn_records.lines.covered.contains(&key))
            {
                self.live_scroll.remeasure_items(index..index + 1);
            }
        }
    }
    fn remeasure_undo_panel(&self, run: &str) {
        let Some(snapshot) = self.conversation_snapshot() else {
            return;
        };
        for (index, slot) in self.live_layout.iter().enumerate() {
            if let Some(key) = transcript::resolve(snapshot, *slot).and_then(records::closing_key)
                && self
                    .turn_records
                    .lines
                    .rows
                    .get(&key)
                    .is_some_and(|line| line.run == run)
            {
                self.live_scroll.remeasure_items(index..index + 1);
            }
        }
    }
    pub(super) fn undo_refusal(&self, line: &Line) -> Option<(&'static str, &'static str)> {
        if !self.host.connected {
            return Some(("Offline", "Reconnect to the host to undo this run."));
        }
        if self.turn_records.pending.is_some() {
            return Some(("Working", "An undo is waiting for the host's reply."));
        }
        if !self.records_current() {
            return Some(("Loading", "Waiting for the current run records."));
        }
        if self.state.repository.write.busy()
            || self.host.controlling
            || self.host.snapshot.as_ref().is_some_and(|s| s.status.busy())
            || self.host.snapshot.as_ref().is_some_and(|s| {
                self.host
                    .threads
                    .iter()
                    .any(|t| t.checkout == s.checkout && t.status.busy())
            })
        {
            return Some((
                "Wait",
                "A run or another write is active in this checkout. Wait until it finishes.",
            ));
        }
        match line.availability {
            Availability::Recording => {
                Some(("Recording", "The host is still taking this run's record."))
            }
            Availability::Failed => Some((
                "Unavailable",
                "The record could not be made, so this run cannot be undone.",
            )),
            Availability::Unavailable => Some((
                "Unavailable",
                "The record needed to undo this run is no longer retained.",
            )),
            Availability::Ready => None,
        }
    }
    pub(super) fn shortcut_undo(&self) -> Option<String> {
        if self.settings_open || self.tab != Workspace::Conversation || self.overlay.is_some() {
            return None;
        }
        let line = self.turn_records.lines.latest.as_ref()?;
        (self.undo_refusal(line).is_none()
            && self.turn_records.blocked.is_none()
            && self.turn_records.retry.is_none()
            && self.turn_records.note.is_none())
        .then(|| line.run.clone())
    }
    pub(super) fn undo_last_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(run) = self.shortcut_undo() {
            self.undo_run(&run, false, window, cx);
        }
    }
    pub(super) fn undo_run(
        &mut self,
        run: &str,
        leave_changed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(line) = self.turn_records.lines.runs.get(run) else {
            return;
        };
        if self.undo_refusal(line).is_some() {
            return;
        }
        let Some(target) = line.target.clone() else {
            return;
        };
        if leave_changed
            && !self
                .turn_records
                .blocked
                .as_ref()
                .is_some_and(|blocked| blocked.run == run && blocked.reply.record_id == target)
        {
            return;
        }
        let attempt = self
            .turn_records
            .retry
            .take()
            .filter(|old| {
                old.run == run && old.target == target && old.leave_changed == leave_changed
            })
            .unwrap_or_else(|| Attempt {
                run: run.into(),
                target,
                request: request_id(),
                leave_changed,
            });
        self.send_undo(attempt, window, cx);
    }
    fn send_undo(&mut self, attempt: Attempt, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stamp) = self.record_stamp() else {
            return;
        };
        let request = Request::undo_turn(
            stamp.thread.clone(),
            attempt.target.clone(),
            attempt.request.clone(),
            attempt.leave_changed,
        );
        self.turn_records.blocked = None;
        self.turn_records.note = None;
        self.turn_records.pending = Some(attempt.clone());
        self.remeasure_undo_panel(&attempt.run);
        let task = cx
            .background_executor()
            .spawn(async move { Client::from_environment()?.request::<UndoTurnReply>(&request) });
        cx.spawn_in(window, async move |this, cx| {
            let reply = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                // The operation itself changes records_revision. Guard identity,
                // not that revision, so its own watch update cannot hide success.
                if !this.host.accepts_watch(stamp.epoch, stamp.watch, &stamp.thread)
                    || this.turn_records.pending.as_ref().is_none_or(|p| p.request != attempt.request)
                { return; }
                this.turn_records.pending = None;
                match reply {
                    Ok(reply) if reply.state == UndoState::Blocked => {
                        let coverage = this.turn_records.lines.runs.get(&attempt.run)
                            .map(|line| line.not_covered.as_slice()).unwrap_or_default();
                        this.turn_records.blocked = Some(Blocked::new(attempt.run.clone(), reply, coverage));
                    }
                    Ok(_) => {
                        this.turn_records.retry = None;
                        this.fetch_turn_records(window, cx);
                        this.state.repository.refresh();
                        this.pump(cx);
                    }
                    Err(host::Error::Rejected(error)) => {
                        let label = match error.code.as_str() { "busy" => "Wait", "undo_no_effect" => "Retry", "undo_incomplete" => "Incomplete", _ => "Unavailable" };
                        let detail = if error.code == "undo_incomplete" {
                            "Undo was interrupted; some files may have changed. Inspect Changes before another undo.".into()
                        } else { error.message };
                        this.turn_records.note = Some((attempt.run.clone(), label, detail));
                        this.fetch_turn_records(window, cx);
                        this.state.repository.refresh();
                        this.pump(cx);
                    }
                    Err(error) => {
                        // An explicit Retry repeats the identical ID and flags.
                        this.turn_records.retry = Some(attempt.clone());
                        this.turn_records.note = Some((attempt.run.clone(), "Retry", format!("The reply was not received. Retry checks the same operation without applying it twice. {error}")));
                    }
                }
                this.remeasure_undo_panel(&attempt.run);
                cx.notify();
            });
        }).detach();
        cx.notify();
    }
    pub(super) fn cancel_undo_panel(&mut self, cx: &mut Context<Self>) {
        if let Some(blocked) = self.turn_records.blocked.take() {
            self.remeasure_undo_panel(&blocked.run);
        }
        cx.notify();
    }

    pub(super) fn record_line(
        &self,
        line: &Line,
        latest: bool,
        group: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let run = &line.run;
        let refusal = self.undo_refusal(line);
        let note = self
            .turn_records
            .note
            .as_ref()
            .filter(|(id, _, _)| id == run);
        let label = refusal
            .map(|(label, _)| label)
            .or_else(|| note.map(|(_, label, _)| *label))
            .unwrap_or(if line.undone { "Redo" } else { "Undo" });
        let detail = refusal
            .map(|(_, reason)| reason.to_owned())
            .or_else(|| note.map(|(_, _, reason)| reason.clone()))
            .unwrap_or_else(|| line.detail.clone());
        let action = if latest {
            button(SharedString::from(format!("undo-{run}")), label)
        } else {
            quiet_button(SharedString::from(format!("undo-{run}")), label, group)
        };
        let retryable = note.is_none_or(|(_, label, _)| matches!(*label, "Retry" | "Wait"));
        let action = action
            .h(px(22.))
            .tooltip(move |_, cx| cx.new(|_| TextTooltip(detail.clone())).into())
            .when(refusal.is_some() || !retryable, |el| {
                el.text_color(rgb(colors().subtle))
                    .cursor_default()
                    .a11y_synthetic_children(|builder| builder.parent_node().set_disabled())
            })
            .when(refusal.is_none() && retryable, |el| {
                el.on_activate(cx, {
                    let run = run.clone();
                    move |this, window, cx| {
                        if let Some(attempt) =
                            this.turn_records.retry.clone().filter(|p| p.run == run)
                        {
                            if this.turn_records.pending.is_none() {
                                this.send_undo(attempt, window, cx);
                            }
                        } else {
                            this.undo_run(&run, false, window, cx);
                        }
                    }
                })
            });
        let tip = line.detail.clone();
        let mut content = row()
            .id(SharedString::from(format!("run-record-{run}")))
            .w_full()
            .min_w_0()
            .h(px(22.))
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(rgb(colors().subtle))
            .tooltip(move |_, cx| cx.new(|_| TextTooltip(tip.clone())).into());
        if latest
            && !line.undone
            && let Some(snapshot) = &self.host.snapshot
        {
            let status = match snapshot.status {
                host::ThreadStatus::Stopped => Some(("Stopped", colors().muted)),
                host::ThreadStatus::Interrupted => Some(("Interrupted", colors().amber)),
                host::ThreadStatus::Error => Some(("Failed", colors().red)),
                _ => None,
            };
            if let Some((label, tint)) = status {
                let detail = plait_core::history::run_state(snapshot, self.history.now)
                    .map(|(text, _)| text)
                    .unwrap_or_default();
                content = content.child(
                    div()
                        .id("record-run-state")
                        .flex_shrink_0()
                        .text_color(rgb(tint))
                        .child(label)
                        .tooltip(move |_, cx| cx.new(|_| TextTooltip(detail.clone())).into()),
                );
            }
        }
        if line.undone {
            content = content.child(
                div()
                    .flex_shrink_0()
                    .text_color(rgb(colors().muted))
                    .child("Undone"),
            );
        } else if let Some(steps) = line.steps {
            content = content.child(div().flex_shrink_0().text_color(rgb(colors().muted)).child(
                format!("{steps} {}", if steps == 1 { "step" } else { "steps" }),
            ));
        }
        content = content
            .child(
                row()
                    .min_w_0()
                    .flex_1()
                    .gap(px(4.))
                    .children(line.files.iter().take(3).map(|file| {
                        let path = file.path.clone();
                        button(
                            SharedString::from(format!("run-file-{run}-{}", path.display())),
                            "",
                        )
                        .min_w_0()
                        .flex_shrink_1()
                        .h(px(22.))
                        .px_0()
                        .gap(px(3.))
                        .aria_label(format!(
                            "Show {} in Changes, {}",
                            path.display(),
                            file.counts
                        ))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(mono())
                                .text_color(rgb(colors().blue))
                                .underline()
                                .child(file.name.clone()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(rgb(colors().subtle))
                                .text_size(px(11.))
                                .child(file.counts.clone()),
                        )
                        .on_activate(cx, move |this, window, cx| {
                            this.open_thread_file(path.clone(), window, cx)
                        })
                    }))
                    .when(line.files.len() > 3, |el| {
                        el.child(
                            div()
                                .flex_shrink_0()
                                .child(format!("and {} more", line.files.len() - 3)),
                        )
                    }),
            )
            .when(line.after_restart, |el| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(colors().amber))
                        .child("After restart"),
                )
            })
            .when(line.head_moved, |el| {
                el.child(div().flex_shrink_0().child("HEAD moved"))
            })
            .child(action);
        column()
            .w_full()
            .min_w_0()
            .child(content)
            .children(self.undo_panel(line, cx))
            .into_any_element()
    }
    fn undo_panel(&self, line: &Line, cx: &mut Context<Self>) -> Option<AnyElement> {
        let blocked = self
            .turn_records
            .blocked
            .as_ref()
            .filter(|blocked| blocked.run == line.run)?;
        let covered = &blocked.coverage;
        let run = line.run.clone();
        let list = column()
            .id("undo-paths")
            .max_h(px(132.))
            .overflow_y_scroll()
            .gap_1()
            .children(
                blocked
                    .changed
                    .iter()
                    .map(|path| div().font_family(mono()).child(path.clone())),
            )
            .when(blocked.changed_omitted > 0, |el| {
                el.child(format!("{} more changed paths", blocked.changed_omitted))
            })
            .when(!covered.is_empty(), |el| {
                el.child(div().mt_2().child("Not covered"))
            })
            .children(covered.iter().map(|text| div().child(text.clone())))
            .when(blocked.omitted > 0, |el| {
                el.child(format!("{} more not-covered paths", blocked.omitted))
            })
            .when(blocked.host_omitted > 0, |el| {
                el.child(format!(
                    "The host preview omitted {} further coverage entries.",
                    blocked.host_omitted
                ))
            });
        Some(column().mt_2().mb_1().p_3().gap_2().border_1().border_color(rgb(colors().edge)).rounded_md()
            .text_size(px(12.)).text_color(rgb(colors().muted))
            .child(div().text_color(rgb(colors().text)).child("Changed since this turn"))
            .child("Files found changed by you or a later run are left alone; Undo the rest restores only the other covered files.")
            .child(list)
            .child(row().child(button("undo-rest", "Undo the rest").disabled(self.undo_refusal(line).is_some())
                .on_activate(cx, move |this, window, cx| this.undo_run(&run, true, window, cx)))
                .child(button("undo-cancel", "Cancel").on_activate(cx, |this, _, cx| this.cancel_undo_panel(cx))))
            .into_any_element())
    }
    pub(super) fn legacy_record_tip(&self) -> String {
        if self.host.protocol() < 5 {
            "Undo needs a newer host (version 5 or later).".into()
        } else if let Some(error) = &self.turn_records.error {
            format!("Records could not be read. {error}")
        } else if self.turn_records.loading {
            "Loading the host's run records.".into()
        } else {
            "No retained host record is available for this turn.".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn blocked_previews_bound_both_lists_before_rendering() {
        let coverage: Vec<_> = (0..10_000)
            .map(|n| Uncovered {
                path: format!("excluded-{n:05}/"),
                reason: plait_core::git::record::Reason::Unreadable,
            })
            .collect();
        let reply = UndoTurnReply {
            state: UndoState::Blocked,
            changed_since: (0..500).map(|n| format!("changed-{n}").into()).collect(),
            changed_since_total: 1000,
            not_covered: vec![],
            not_covered_total: 0,
            record_id: "target".into(),
            after_restart: false,
        };
        let blocked = Blocked::new("run".into(), reply, &coverage);
        assert_eq!(blocked.coverage.len(), records::PREVIEW_LIMIT);
        assert_eq!(blocked.changed.len(), records::PREVIEW_LIMIT);
        assert_eq!(blocked.omitted, 9988);
        assert_eq!(blocked.changed_omitted, 988);
    }
}
