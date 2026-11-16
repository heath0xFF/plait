use super::{Plait, theme::*};
use gpui::{prelude::*, *};
use plait_core::{
    host::{Approval, ApprovalMode, Message, MessageRole, Snapshot, Tool, ToolStatus},
    transcript::{self, Row, Slot},
};

/// Full-width line above the conversation for host and submission state.
fn banner(tint: u32) -> Div {
    row()
        .flex_shrink_0()
        .px_6()
        .py_2()
        .gap_3()
        .text_size(px(12.))
        .text_color(rgb(tint))
        .border_b_1()
        .border_color(rgb(BORDER))
}

fn tool_tint(status: ToolStatus) -> u32 {
    match status {
        ToolStatus::Running => BLUE,
        ToolStatus::Done => SUBTLE,
        ToolStatus::Error => RED,
        ToolStatus::Interrupted => AMBER,
    }
}

impl Plait {
    /// The mode the next submission requests. Version 1 hosts always ask.
    pub(super) fn next_approval_mode(&self) -> Option<ApprovalMode> {
        (self.host.protocol() >= 2).then_some(if self.preferences.value.ask_before_actions {
            ApprovalMode::Ask
        } else {
            ApprovalMode::Auto
        })
    }

    /// A running thread reports its own mode; otherwise show what the next message uses.
    fn approval_note(&self, snapshot: Option<&Snapshot>) -> &'static str {
        let mode = match snapshot {
            Some(snapshot) if snapshot.status.busy() => snapshot.approval_mode,
            _ => self.next_approval_mode(),
        };
        if mode == Some(ApprovalMode::Auto) {
            "Automatic approval"
        } else {
            "Approve each action"
        }
    }

    fn live_message(&self, message: &Message, cx: &mut Context<Self>) -> AnyElement {
        let user = message.role == MessageRole::User;
        let body = if user { bubble() } else { prose() }
            .id((
                ElementId::from("live-message"),
                SharedString::from(message.id.clone()),
            ))
            .role(Role::Group)
            .aria_label(format!(
                "{} message: {}",
                if user { "Your" } else { "Assistant" },
                message.text
            ))
            .child(
                column()
                    .gap_1()
                    .child(
                        row()
                            .justify_between()
                            .child(tag(if user { "You" } else { "Plait" }, SUBTLE))
                            .child(
                                button(
                                    SharedString::from(format!("copy-message-{}", message.id)),
                                    "Copy",
                                )
                                .aria_label("Copy message source")
                                .on_activate(cx, {
                                    let text = message.text.clone();
                                    move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            text.clone(),
                                        ))
                                    }
                                }),
                            ),
                    )
                    .child(if !user {
                        self.formatted
                            .messages
                            .get(&message.id)
                            .map(|body| self.markdown(&message.id, body, cx))
                            .unwrap_or_else(|| {
                                prose().child(message.text.clone()).into_any_element()
                            })
                    } else {
                        prose().child(message.text.clone()).into_any_element()
                    }),
            );
        if user {
            row().justify_end().child(body).into_any_element()
        } else {
            // A block, not a flex row, so long replies wrap at the column width.
            body.into_any_element()
        }
    }

    pub(super) fn toggle_live_tool(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(snapshot) = self.host.snapshot.as_ref() else {
            return;
        };
        let Some(offset) = snapshot.tools.iter().position(|tool| tool.id == id) else {
            return;
        };
        let Some(index) = self
            .live_layout
            .iter()
            .position(|slot| *slot == Slot::Tool(offset))
        else {
            return;
        };
        if !self.expanded_live_tools.remove(id) {
            self.expanded_live_tools.insert(id.to_owned());
        }
        self.live_scroll.remeasure_items(index..index + 1);
        cx.notify();
    }

    fn live_tool(&self, tool: &Tool, cx: &mut Context<Self>) -> AnyElement {
        let expanded = self.expanded_live_tools.contains(&tool.id);
        let id = tool.id.clone();
        let subject = transcript::tool_subject(tool);
        column()
            .gap_1()
            .child(
                row().min_w_0().child(
                    button(
                        (
                            ElementId::from("live-tool"),
                            SharedString::from(tool.id.clone()),
                        ),
                        "",
                    )
                    .px_1()
                    .ml(px(-TOOL_INSET))
                    .min_w_0()
                    .aria_label(format!(
                        "{} tool{} · {}",
                        tool.name,
                        subject
                            .as_ref()
                            .map(|subject| format!(" {subject}"))
                            .unwrap_or_default(),
                        tool.status.label()
                    ))
                    .aria_expanded(expanded)
                    .child(icon(if expanded { "down" } else { "right" }))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(13.))
                            .child(tool.name.clone()),
                    )
                    .when_some(subject.clone(), |el, subject| {
                        el.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(MONO)
                                .text_size(px(12.))
                                .text_color(rgb(MUTED))
                                .child(subject),
                        )
                    })
                    // Completion is the normal case; only call out the others.
                    .when(tool.status != ToolStatus::Done, |el| {
                        el.child(tag(tool.status.label(), tool_tint(tool.status)).flex_shrink_0())
                    })
                    .on_activate(cx, move |this, _, cx| this.toggle_live_tool(&id, cx)),
                ),
            )
            .when(expanded, |el| {
                el.child(
                    code_block()
                        .child(
                            button(
                                SharedString::from(format!("copy-tool-{}", tool.id)),
                                "Copy tool details",
                            )
                            .on_activate(cx, {
                                let text = format!("{}\n{}", tool.input, tool.output);
                                move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                                }
                            }),
                        )
                        .child(div().child(text!(tool.input.clone())))
                        .child(
                            div()
                                .text_color(rgb(TEXT))
                                .child(text!(tool.output.clone())),
                        ),
                )
            })
            .into_any_element()
    }

    fn live_event(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(snapshot) = self.host.snapshot.clone() else {
            return div().into_any_element();
        };
        let Some(row) = self
            .live_layout
            .get(index)
            .and_then(|slot| transcript::resolve(&snapshot, *slot))
        else {
            return div().into_any_element();
        };
        let tool = matches!(row, Row::Tool(_));
        let content = match row {
            Row::Message(message) => self.live_message(message, cx),
            Row::Streaming(text) => column()
                .gap_1()
                .child(tag("Assistant · streaming", BLUE))
                .child(
                    self.formatted
                        .live
                        .as_ref()
                        .map(|body| self.markdown("stream", body, cx))
                        .unwrap_or_else(|| {
                            prose().child(text!(text.to_owned())).into_any_element()
                        }),
                )
                .into_any_element(),
            Row::Tool(tool) => self.live_tool(tool, cx),
        };
        div()
            .w_full()
            .px_6()
            .py(if tool { px(2.) } else { px(10.) })
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_WIDTH))
                    .mx_auto()
                    .child(content),
            )
            .into_any_element()
    }

    /// Exact arguments and a one-call decision. Nothing runs without it.
    fn approval(&self, approval: &Approval, cx: &mut Context<Self>) -> impl IntoElement {
        let exact = approval.details.len() <= 16384 && !approval.details.is_empty();
        let enabled = exact
            && self.host.connected
            && !self.host.controlling
            && !self.host.decided_approvals.contains(&approval.id);
        let allow_id = approval.id.clone();
        let deny_id = approval.id.clone();
        let dirty = approval.dirty_paths.join(", ");
        div().flex_shrink_0().px_6().pt_2().child(
            column()
                .w_full()
                .max_w(px(CONTENT_WIDTH))
                .mx_auto()
                .p_3()
                .gap_2()
                .rounded_xl()
                .border_1()
                .border_color(rgb(0x5c4f38))
                .bg(rgb(RAIL))
                .text_size(px(12.))
                .child(
                    row().child(dot(AMBER)).child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("Approval required: {}", approval.summary)),
                    ),
                )
                .child(
                    div()
                        .id("approval-details")
                        .role(Role::Group)
                        .aria_label(format!(
                            "Exact tool arguments: {}. Risk: {}. Protected dirty paths: {}",
                            approval.details, approval.risk, dirty
                        ))
                        .max_h(px(140.))
                        .overflow_y_scroll()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(BG))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .font_family(MONO)
                        .line_height(px(19.))
                        .child(approval.details.clone()),
                )
                .child(div().text_color(rgb(AMBER)).child(approval.risk.clone()))
                .when(!approval.dirty_paths.is_empty(), |el| {
                    el.child(
                        div()
                            .text_color(rgb(MUTED))
                            .child(format!("Protected dirty paths: {dirty}")),
                    )
                })
                .when(!exact, |el| {
                    el.child(div().text_color(rgb(RED)).child(
                        "Exact arguments are unavailable. Approval is disabled; Stop this run.",
                    ))
                })
                .child(
                    row()
                        .justify_end()
                        .child(
                            toned_button("deny-tool", "Deny once", Tone::Filled)
                                .disabled(!enabled)
                                .when(enabled, |el| {
                                    el.on_activate(cx, move |this, window, cx| {
                                        this.control_thread(
                                            Some((deny_id.clone(), false)),
                                            window,
                                            cx,
                                        )
                                    })
                                }),
                        )
                        .child(
                            toned_button("allow-tool", "Allow once", Tone::Filled)
                                .text_color(rgb(AMBER))
                                .disabled(!enabled)
                                .when(enabled, |el| {
                                    el.on_activate(cx, move |this, window, cx| {
                                        this.control_thread(
                                            Some((allow_id.clone(), true)),
                                            window,
                                            cx,
                                        )
                                    })
                                }),
                        ),
                ),
        )
    }

    fn live_banners(&self, pending: bool, cx: &mut Context<Self>) -> Vec<Div> {
        let mut banners = Vec::new();
        if let Some(error) = self.host.error.clone() {
            banners.push(banner(RED).child(text!(error)));
        }
        if !self.host.connected {
            let connecting = self.host.connecting;
            banners.push(
                banner(AMBER)
                    .child(div().flex_1().min_w_0().child(
                        "Disconnected · displayed content may be stale. The host may still be running this thread.",
                    ))
                    .child(
                        toned_button(
                            "reconnect-conversation",
                            if connecting { "Connecting…" } else { "Reconnect" },
Tone::Filled,
)
                        .disabled(connecting)
                        .when(!connecting, |el| {
                            el.on_activate(cx, |this, window, cx| this.reconnect_host(window, cx))
                        }),
                    ),
            );
        }
        if pending {
            let ready = self.host.connected && !self.host.submitting && self.session.ready();
            banners.push(
                banner(MUTED)
                    .child(div().flex_1().min_w_0().child(if self.host.submitting {
                        "Waiting for host acceptance…"
                    } else {
                        "Submission unconfirmed. Retry uses the same request ID and cannot start duplicate work."
                    }))
                    .child(
                        toned_button("retry-submit", "Retry safely", Tone::Filled)
                            .disabled(!ready)
                            .when(ready, |el| {
                                el.on_activate(cx, |this, window, cx| {
                                    this.retry_submission(window, cx)
                                })
                            }),
                    ),
            );
        }
        banners
    }

    pub(super) fn live_conversation(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.host.snapshot.clone();
        let model = snapshot
            .as_ref()
            .map(|snapshot| &snapshot.model)
            .or_else(|| {
                self.host
                    .threads
                    .iter()
                    .find(|thread| Some(&thread.id) == self.host.selected.as_ref())
                    .map(|thread| &thread.model)
            });
        let model_label = model.map_or_else(
            || "Loading saved model…".into(),
            |model| format!("{} / {}", model.provider, model.model_id),
        );
        let pending = self
            .host
            .selected
            .as_ref()
            .is_some_and(|id| self.host.submissions.pending(id).is_some());
        let mut main = column()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .children(self.live_banners(pending, cx))
            .child(if snapshot.is_none() {
                empty(
                    "Loading thread",
                    "The conversation comes from the host. Reconnect if the host is unavailable.",
                )
                .into_any_element()
            } else if self.live_count == 0 {
                empty(
                    "Start this conversation",
                    "Send a message below. The saved provider and model will be used for every turn.",
                )
                .into_any_element()
            } else {
                list(
                    self.live_scroll.clone(),
                    cx.processor(|this, index, _, cx| this.live_event(index, cx)),
                )
                .flex_1()
                .min_h_0()
                .into_any_element()
            });
        if let Some(snapshot) = &snapshot {
            if snapshot.truncated {
                main = main.child(div().px_6().py_1().child(tag(
                    "Bounded preview · older messages or tool output have been clipped by the host.",
                    AMBER,
                )));
            }
            if let Some(error) = &snapshot.error {
                main = main.child(
                    div()
                        .px_6()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(rgb(RED))
                        .child(text!(format!("{}: {}", error.code, error.message))),
                );
            }
            if let Some(approval) = &snapshot.pending_approval {
                main = main.child(self.approval(approval, cx));
            }
        }
        let composer = self
            .host
            .selected
            .as_ref()
            .and_then(|id| self.composers.get(id));
        main = main.child(
            column()
                .flex_shrink_0()
                .px_6()
                .pt_2()
                .pb_3()
                .gap_2()
                .when_some(composer.cloned(), |el, composer| {
                    el.child(
                        div()
                            .w_full()
                            .max_w(px(CONTENT_WIDTH))
                            .mx_auto()
                            .child(composer),
                    )
                })
                .child(self.environment(
                    Some(model_label),
                    Some((self.approval_note(snapshot.as_deref()), MUTED)),
                )),
        );
        row()
            .gap_0()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(main)
            .when(self.shows_inspector(wide), |el| {
                el.child(self.inspector(cx))
            })
    }
}
