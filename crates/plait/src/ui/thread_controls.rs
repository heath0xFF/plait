//! Controls for the next message and host-owned inputs waiting behind a run.
use super::{
    Plait,
    commands::Command,
    model_picker::ModelPurpose,
    palette::{Item, Options, Palette},
    theme::*,
};
use gpui::{prelude::*, *};
use plait_core::{
    compose::{ContextPressure, ContextUsage, thinking_levels},
    host::{self, ApprovalMode, Model, ModelChoice, QueueMode, Request, ThinkingLevel},
};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct ThreadControls {
    serial: u64,
    pending: Option<(String, u64)>,
    pending_epoch: u64,
    pub picker_thread: Option<(String, u64)>,
    note: Option<(String, String, bool)>,
    cancelling: HashMap<String, u64>,
    pub(super) usage_open: bool,
    /// Approval choices made this session, by thread. `None` is an explicit
    /// return to the default, so it shadows an older saved choice.
    approvals: HashMap<String, Option<ApprovalMode>>,
}

/// Plain text shown on hover, for controls whose label is a summary.
struct TextTooltip(String);
impl Render for TextTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(360.))
            .p_3()
            .bg(rgb(PANEL))
            .border_1()
            .border_color(rgb(BORDER))
            .rounded_md()
            .text_size(px(12.))
            .text_color(rgb(TEXT))
            .child(self.0.clone())
    }
}

impl Plait {
    pub(super) fn set_thread_notice(&mut self, id: String, message: String, failed: bool) {
        self.thread_controls.note = Some((id, message, failed));
    }

    fn thread_model(&self) -> Option<&Model> {
        let snapshot = self.host.snapshot.as_ref()?;
        self.host
            .status
            .as_ref()?
            .models
            .iter()
            .find(|model| model.choice() == snapshot.model)
    }

    pub(super) fn thread_configuration_pending(&self) -> bool {
        self.thread_controls.pending_epoch == self.host.epoch
            && self
                .thread_controls
                .pending
                .as_ref()
                .is_some_and(|(id, _)| Some(id) == self.host.selected.as_ref())
    }

    fn configure_reason(&self) -> Option<&'static str> {
        if self.host.protocol() < 3 {
            Some("Update the host to change a thread's model.")
        } else if !self.host.connected {
            Some("Reconnect to change this thread.")
        } else if self.host.snapshot.is_none() {
            Some("Loading thread…")
        } else if self.host.snapshot.as_ref().is_some_and(|s| s.status.busy()) {
            Some("Available after the current run ends.")
        } else if self.thread_configuration_pending() {
            Some("Saving thread settings…")
        } else {
            None
        }
    }

    pub(super) fn open_thread_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(reason) = self.configure_reason() {
            if let Some(id) = &self.host.selected {
                self.thread_controls.note = Some((id.clone(), reason.into(), false));
            }
            cx.notify();
            return;
        }
        self.thread_controls.picker_thread =
            self.host.selected.clone().map(|id| (id, self.host.epoch));
        self.open_model_picker(ModelPurpose::ThreadModel, window, cx);
    }

    pub(super) fn choose_thread_model(
        &mut self,
        choice: ModelChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((id, epoch)) = self.thread_controls.picker_thread.take() else {
            return;
        };
        if epoch != self.host.epoch || self.host.selected.as_ref() != Some(&id) {
            return;
        }
        self.configure_selected(Some(choice), None, window, cx);
    }

    pub(super) fn configure_selected(
        &mut self,
        model: Option<ModelChoice>,
        thinking: Option<ThinkingLevel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.configure_reason().is_some() {
            return;
        }
        let Some(snapshot) = &self.host.snapshot else {
            return;
        };
        if let Some(level) = thinking
            && !thinking_levels(self.thread_model()).contains(&level)
        {
            return;
        }
        let id = snapshot.id.clone();
        let checkout = snapshot.checkout.clone();
        let watch_epoch = self.host.watch_epoch;
        self.thread_controls.serial += 1;
        let serial = self.thread_controls.serial;
        self.thread_controls.pending = Some((id.clone(), serial));
        self.thread_controls.pending_epoch = self.host.epoch;
        // The controls show the new choice; only a failure needs words.
        self.thread_controls.note = None;
        self.sync_composer(cx);
        self.host_request::<host::ThreadReply>(
            Request::configure_thread(id.clone(), model.clone(), thinking),
            window,
            cx,
            move |this, result, _, cx| {
                if this.thread_controls.pending.as_ref() != Some(&(id.clone(), serial)) {
                    return;
                }
                this.thread_controls.pending = None;
                if !this.host.accepts_watch(this.host.epoch, watch_epoch, &id) {
                    return;
                }
                let result = match result {
                    Ok(reply)
                        if reply.thread.id == id
                            && reply.thread.checkout == checkout
                            && model.as_ref().is_none_or(|m| *m == reply.thread.model)
                            && thinking.is_none_or(|t| Some(t) == reply.thread.thinking_level) =>
                    {
                        Ok(())
                    }
                    Ok(_) => {
                        Err("Host returned different settings. Reconnect to check them.".into())
                    }
                    Err(error) => Err(error.to_string()),
                };
                this.thread_controls.note = result
                    .err()
                    .map(|error| (id, format!("Not saved: {error}"), true));
                this.capture_session(cx);
            },
        );
    }

    pub(super) fn open_thinking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.configure_reason().is_some() {
            return;
        }
        let items = thinking_levels(self.thread_model())
            .iter()
            .map(|level| {
                Item::new(
                    format!("thinking-{level:?}"),
                    level.label(),
                    Command::Thinking {
                        thread_id: self.host.selected.clone().unwrap_or_default(),
                        epoch: self.host.epoch,
                        level: *level,
                    },
                )
            })
            .collect();
        let palette = cx.new(|cx| {
            Palette::new(
                Options {
                    name: "Thinking level".into(),
                    placeholder: "Choose thinking level…".into(),
                    empty: "This model has no thinking levels.".into(),
                    initial: None,
                    mark: None,
                    aside: None,
                    note: Some("Applies from the next run.".into()),
                },
                items,
                window,
                cx,
            )
        });
        self.present(palette, None, window, cx);
    }

    pub(super) fn thread_environment(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(snapshot) = &self.host.snapshot else {
            return div().into_any_element();
        };
        let model = self.thread_model();
        let model_name = self.model_name(&snapshot.model);
        let reason = self.configure_reason();
        let levels = thinking_levels(model);
        let level = snapshot.thinking_level.unwrap_or(ThinkingLevel::Off);
        let usage = snapshot
            .usage
            .as_ref()
            .and_then(|u| ContextUsage::new(u, model));
        let mut controls = row()
            .flex_wrap()
            .gap_x_2()
            .gap_y_1()
            .w_full()
            .min_w_0()
            .text_size(px(11.))
            .text_color(rgb(MUTED))
            .child(
                button("thread-model", "")
                    .max_w(px(240.))
                    .flex_shrink_1()
                    .aria_label(format!("Change thread model: {model_name}"))
                    .aria_description(reason.unwrap_or("Choose a model for the next run."))
                    .disabled(reason.is_some())
                    .child(div().min_w_0().truncate().child(text!(model_name)))
                    .on_activate(cx, |this, window, cx| this.open_thread_models(window, cx)),
            );
        if !levels.is_empty() {
            controls = controls.child(
                // Just the level; a bare "Off" beside the model would not say what is off.
                button(
                    "thread-thinking",
                    if level == ThinkingLevel::Off {
                        "No thinking"
                    } else {
                        level.label()
                    },
                )
                .aria_label(format!("Thinking level: {}", level.label()))
                .aria_description(reason.unwrap_or("Choose how much this model thinks."))
                .disabled(reason.is_some())
                .on_activate(cx, |this, window, cx| this.open_thinking(window, cx)),
            );
        }
        if let Some(usage) = &usage {
            let detail = usage.detail.clone();
            let tint = match usage.pressure {
                ContextPressure::Normal => MUTED,
                ContextPressure::High => AMBER,
                ContextPressure::NearlyFull => RED,
            };
            controls = controls.child(
                button("thread-usage", usage.label.clone())
                    .text_color(rgb(tint))
                    .aria_description(detail.clone())
                    .tooltip(move |_, cx| cx.new(|_| TextTooltip(detail.clone())).into())
                    .on_activate(cx, |this, _, cx| {
                        this.thread_controls.usage_open = !this.thread_controls.usage_open;
                        cx.notify();
                    }),
            );
        }
        controls = controls.child(self.run_settings_button(snapshot.id.clone(), cx));
        let mut content = column()
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .mx_auto()
            .gap_1()
            .child(controls);
        if snapshot.status.busy() {
            let approval = if snapshot.approval_mode == Some(host::ApprovalMode::Auto) {
                "automatic approval"
            } else {
                "ask before actions"
            };
            content = content.child(tag(
                format!("Current run: {approval} · Model locked during run"),
                SUBTLE,
            ));
        }
        if self.thread_controls.usage_open
            && let Some(usage) = usage
        {
            content = content.child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(usage.detail),
            );
        }
        if let Some((id, note, failed)) = &self.thread_controls.note
            && id == &snapshot.id
        {
            content = content.child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(if *failed { AMBER } else { MUTED }))
                    .child(note.clone()),
            );
        }
        content.into_any_element()
    }

    fn queued_text(&self, request: &host::Queued) -> Option<String> {
        let id = self.host.selected.as_deref()?;
        self.host
            .submissions
            .accepted(id)
            .iter()
            .find(|p| p.request_id == request.request_id)
            .map(|p| p.text.clone())
    }

    fn cancel_input(
        &mut self,
        request_id: String,
        restore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = &self.host.snapshot else {
            return;
        };
        if !self.host.connected
            || self.thread_controls.cancelling.get(&request_id) == Some(&self.host.epoch)
        {
            return;
        }
        let Some(queued) = snapshot.queued.iter().find(|q| q.request_id == request_id) else {
            return;
        };
        let text = restore.then(|| self.queued_text(queued)).flatten();
        if restore && text.is_none() {
            return;
        }
        let id = snapshot.id.clone();
        let checkout = snapshot.checkout.clone();
        let composer = self.composers.get(&id).cloned();
        let revision = composer.as_ref().map(|c| c.read(cx).saved_draft().1);
        if restore && composer.as_ref().is_none_or(|c| !c.read(cx).is_empty()) {
            return;
        }
        self.thread_controls
            .cancelling
            .insert(request_id.clone(), self.host.epoch);
        self.host_request::<host::ThreadReply>(Request::cancel_queued(id.clone(), request_id.clone()), window, cx, move |this, result, window, cx| {
            this.thread_controls.cancelling.remove(&request_id);
            match result {
                Ok(reply) if reply.thread.id == id && reply.thread.checkout == checkout && !reply.thread.queued.iter().any(|q| q.request_id == request_id) => {
                    this.host.submissions.forget_accepted(&id, &request_id);
                    if let (Some(text), Some(composer), Some(revision)) = (text, composer, revision) {
                        let restored = composer.update(cx, |composer, cx| composer.restore_cancelled(&text, revision, cx));
                        if restored && this.host.selected.as_ref() == Some(&id) { this.focus_composer(window, cx); }
                        else if !restored { cx.write_to_clipboard(ClipboardItem::new_string(text)); this.set_thread_notice(id.clone(), "Message cancelled. The draft changed, so its text was copied to the clipboard.".into(), false); }
                    }
                    this.capture_session(cx);
                }
                Ok(_) => this.set_thread_notice(id.clone(), "Cancellation was not confirmed. Reconnect to check the queue.".into(), true),
                Err(error) => this.set_thread_notice(id.clone(), format!("Message not cancelled: {error}"), true),
            }
        });
    }

    pub(super) fn queued_inputs(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.host.protocol() < 3 {
            return None;
        }
        let snapshot = self.host.snapshot.as_ref()?;
        let pending = self.host.submissions.all(&snapshot.id);
        if pending.is_empty() && snapshot.queued.is_empty() {
            return None;
        }
        let mut stack = column()
            .id("queued-messages")
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .mx_auto()
            .max_h(px(144.))
            .overflow_y_scroll()
            .gap_1()
            .py_1()
            .text_size(px(12.));
        if !snapshot.status.busy() && !snapshot.queued.is_empty() {
            stack = stack.child(tag(
                "Queue held · next send resumes from the first message",
                AMBER,
            ));
        }
        let draft_empty = self
            .composers
            .get(&snapshot.id)
            .is_some_and(|c| c.read(cx).is_empty());
        for queued in &snapshot.queued {
            let request_id = queued.request_id.clone();
            let cancel_id = request_id.clone();
            let edit_id = request_id.clone();
            let cancelling =
                self.thread_controls.cancelling.get(&request_id) == Some(&self.host.epoch);
            let mode = match queued.mode {
                QueueMode::FollowUp => "Follow-up",
                QueueMode::Steer => "Steering",
            };
            let edit = draft_empty && self.queued_text(queued).is_some();
            stack = stack.child(
                row()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(tag(mode, SUBTLE))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(queued.text.clone()),
                    )
                    .when(edit, |el| {
                        el.child(
                            button(
                                SharedString::from(format!("edit-queued-{request_id}")),
                                "Edit",
                            )
                            .aria_label("Cancel queued message into draft")
                            .disabled(cancelling || !self.host.connected)
                            .on_activate(
                                cx,
                                move |this, window, cx| {
                                    this.cancel_input(edit_id.clone(), true, window, cx)
                                },
                            ),
                        )
                    })
                    .child(
                        button(
                            SharedString::from(format!("cancel-queued-{request_id}")),
                            if cancelling {
                                "Cancelling…"
                            } else {
                                "Cancel"
                            },
                        )
                        .aria_label("Cancel queued message")
                        .disabled(cancelling || !self.host.connected)
                        .on_activate(cx, move |this, window, cx| {
                            this.cancel_input(cancel_id.clone(), false, window, cx)
                        }),
                    ),
            );
        }
        for pending in pending {
            let id = snapshot.id.clone();
            let request_id = pending.request_id.clone();
            let sending = self.host.submissions.is_sending(&request_id);
            let detail = self
                .host
                .submissions
                .error(&request_id)
                .unwrap_or("Retry uses the same request ID.");
            stack = stack.child(
                row()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(tag(
                        if sending { "Sending…" } else { "Unconfirmed" },
                        if sending { BLUE } else { AMBER },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(pending.text.clone()),
                    )
                    .child(
                        button(
                            SharedString::from(format!("retry-{request_id}")),
                            "Retry safely",
                        )
                        .aria_description(detail.to_owned())
                        .disabled(sending || !self.host.connected || !self.session.ready())
                        .on_activate(cx, move |this, window, cx| {
                            this.retry_submission_id(&id, &request_id, window, cx)
                        }),
                    ),
            );
        }
        Some(stack.into_any_element())
    }
}

/// What the run-settings button and menu call an approval mode.
fn approval_label(mode: ApprovalMode) -> &'static str {
    match mode {
        ApprovalMode::Ask => "Ask before actions",
        ApprovalMode::Auto => "Auto-approve actions",
    }
}

impl Plait {
    /// Opens the run-settings menu. The label is this thread's approval mode,
    /// and names trusted commands, since those reach beyond the sandbox.
    fn run_settings_button(&self, id: String, cx: &mut Context<Self>) -> Stateful<Div> {
        let own = self.thread_approval(&id);
        let mode = own.unwrap_or_else(|| self.default_approval());
        let policy = self.command_policy();
        let mut label = approval_label(mode).to_owned();
        if policy == Some(host::policy::CommandPolicy::Trusted) {
            label.push_str(" · Trusted commands");
        }
        let source = if own.is_some() {
            "Chosen for this thread."
        } else {
            "The default from Settings."
        };
        let tip = format!(
            "{source} Commands: {}. Click to change how this thread runs; applies from your next message.",
            plait_core::host::policy::context_label(policy).to_lowercase()
        );
        button("thread-run-settings", "")
            .min_w_0()
            .flex_shrink_1()
            .aria_label(format!("Run settings: {label}"))
            .aria_description(tip.clone())
            .tooltip(move |_, cx| cx.new(|_| TextTooltip(tip.clone())).into())
            .child(div().min_w_0().truncate().child(label))
            .child(icon("down").size(px(12.)))
            .on_activate(cx, |this, window, cx| this.open_run_settings(window, cx))
    }

    /// This thread's own approval choice, if it has one.
    pub(super) fn thread_approval(&self, id: &str) -> Option<ApprovalMode> {
        if let Some(chosen) = self.thread_controls.approvals.get(id) {
            return *chosen;
        }
        let summary = self.host.threads.iter().find(|thread| thread.id == id)?;
        self.saved_thread(summary)?.approval
    }

    /// Give a thread its own approval mode, or `None` to follow the default.
    pub(super) fn set_thread_approval(
        &mut self,
        id: String,
        mode: Option<ApprovalMode>,
        cx: &mut Context<Self>,
    ) {
        self.thread_controls.approvals.insert(id, mode);
        self.capture_session(cx);
        cx.notify();
    }

    /// The menu under the message box for how this thread runs: its approval
    /// mode, and the project's command policy, which is changed in Settings.
    pub(super) fn open_run_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread_id) = self.host.selected.clone() else {
            return;
        };
        let chosen = self.thread_approval(&thread_id);
        let default = self.default_approval();
        let protocol = self.host.protocol();
        let approvals = "Approvals for this thread";
        let choice = |key: &str, title: String, detail: &str, mode: Option<ApprovalMode>| {
            let item = Item::new(
                key.to_owned(),
                title,
                Command::Approval {
                    thread_id: thread_id.clone(),
                    mode,
                },
            )
            .section(approvals)
            .detail(detail.to_owned())
            .enabled(protocol >= 2);
            if chosen == mode {
                item.badge("Current", BLUE)
            } else {
                item
            }
        };
        let current = match chosen {
            None => "approval-default",
            Some(ApprovalMode::Ask) => "approval-ask",
            Some(ApprovalMode::Auto) => "approval-auto",
        };
        let policy = self.command_policy();
        let mut items = vec![
            choice(
                "approval-default",
                format!("Use default ({})", approval_label(default)),
                "Follow Settings → Approvals.",
                None,
            ),
            choice(
                "approval-ask",
                approval_label(ApprovalMode::Ask).into(),
                "Each edit and command waits for Allow once.",
                Some(ApprovalMode::Ask),
            ),
            choice(
                "approval-auto",
                approval_label(ApprovalMode::Auto).into(),
                "Edits and commands run without asking.",
                Some(ApprovalMode::Auto),
            ),
        ];
        items.push(
            Item::new(
                "command-policy",
                plait_core::host::policy::context_label(policy),
                Command::PolicySettings,
            )
            .section("Commands in this project")
            .detail(
                policy.map_or("Shared by every thread in this checkout.", |policy| {
                    policy.meaning()
                }),
            )
            .hint("Change in Settings"),
        );
        let palette = cx.new(|cx| {
            Palette::new(
                Options {
                    name: "Thread run settings".into(),
                    placeholder: "Choose how this thread runs…".into(),
                    empty: "No run settings.".into(),
                    initial: Some(current.into()),
                    mark: None,
                    aside: None,
                    note: Some(
                        if protocol < 2 {
                            "This host asks before every edit and command. Update the host to choose."
                        } else {
                            "Applies from your next message. A running task keeps its settings."
                        }
                        .into(),
                    ),
                },
                items,
                window,
                cx,
            )
        });
        self.present(palette, None, window, cx);
    }
}
