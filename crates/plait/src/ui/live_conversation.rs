use super::{
    Plait,
    selection::{Owner, Site},
    theme::*,
};
use gpui::{prelude::*, *};
use std::time::Instant;

/// Smooth reveal of the streaming reply and reasoning. Advanced once per
/// frame by `advance_reveals`; rows read how much to show.
#[derive(Default)]
pub(super) struct Reveals {
    text: Reveal,
    reasoning: Reveal,
    text_shown: usize,
    reasoning_shown: usize,
}

#[cfg(test)]
impl Reveals {
    pub fn pending_text(&self, total: usize) -> bool {
        self.text.pending(total)
    }
}
use plait_core::{
    history::{self, Key},
    host::{Approval, ApprovalMode, HistoryField, Message, MessageRole, Snapshot},
    markdown::Block,
    reveal::Reveal,
    transcript::{self, Row},
};

/// Full-width line above the conversation for host and submission state.
fn banner(tint: u32) -> Div {
    row()
        .w_full()
        .max_w(px(CONTENT_WIDTH + 48.))
        .mx_auto()
        .flex_shrink_0()
        .px_6()
        .py_2()
        .gap_3()
        .text_size(px(12.))
        .text_color(rgb(tint))
        .border_b_1()
        .border_color(rgb(colors().border))
}

/// How far the copy control of an agent reply sits above its first line. It
/// stays inside the row's top padding, so reaching it keeps the row hovered.
const COPY_LIFT: f32 = 18.;

impl Plait {
    /// The desktop default from Settings → Approvals.
    pub(super) fn default_approval(&self) -> ApprovalMode {
        if self.preferences.value.ask_before_actions {
            ApprovalMode::Ask
        } else {
            ApprovalMode::Auto
        }
    }

    /// The mode the next submission requests: the open thread's own choice,
    /// else the default. Version 1 hosts always ask. A harness thread asks for
    /// none: the harness's own mode governs it.
    pub(super) fn next_approval_mode(&self) -> Option<ApprovalMode> {
        if self.thread_harness() != plait_core::host::harness::HarnessId::Pi {
            return None;
        }
        let chosen = self
            .host
            .selected
            .as_deref()
            .and_then(|id| self.thread_approval(id));
        (self.host.protocol() >= 2).then(|| chosen.unwrap_or_else(|| self.default_approval()))
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

    fn live_message(&self, message: &Message, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let user = message.role == MessageRole::User;
        let site = Site {
            owner: Owner::Message(message.id.clone().into()),
            row: index,
            block: 0,
        };
        let body = match self.formatted.messages.get(&message.id) {
            Some(parsed) => self.markdown(&site, &parsed.blocks, &message.id, cx),
            // The projection arrives with the snapshot; this is a one-frame fallback.
            None => prose().child(message.text.clone()).into_any_element(),
        };
        let body = column()
            .gap_2()
            .children(self.history_parts(
                Key::Message(message.id.clone()),
                Some(HistoryField::Text),
                cx,
            ))
            .child(body)
            .when_some(
                self.formatted.turn_summaries.get(&message.id),
                |el, summary| {
                    el.child(
                        div()
                            .pt_1()
                            .text_size(px(11.))
                            .text_color(rgb(colors().muted))
                            .child(summary.clone()),
                    )
                },
            );
        let part = self
            .history
            .cache
            .get(&Key::Message(message.id.clone()))
            .is_some_and(|entry| {
                entry
                    .parts
                    .iter()
                    .any(|part| part.field == HistoryField::Text)
            });
        let copy = quiet_button(
            SharedString::from(format!("copy-message-{}", message.id)),
            if part {
                "Copy this part"
            } else {
                "Copy message"
            },
            row_group(index),
        )
        .aria_label(if part {
            "Copy this message part"
        } else {
            "Copy message source"
        })
        .on_activate(cx, {
            let text = message.text.clone();
            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
        });
        // Position says who is speaking; the label is for assistive technology.
        let id = (
            ElementId::from("live-message"),
            SharedString::from(message.id.clone()),
        );
        let label = format!(
            "{} message: {}",
            if user { "Your" } else { "Assistant" },
            message.text
        );
        if user {
            div()
                .flex()
                .justify_end()
                .items_start()
                .gap_1()
                .child(copy.mt(px(6.)))
                .child(
                    bubble()
                        .id(id)
                        .role(Role::Group)
                        .aria_label(label)
                        .min_w_0()
                        .child(body),
                )
                .into_any_element()
        } else {
            // A block, not a flex row, so long replies wrap at the column width.
            div()
                .id(id)
                .role(Role::Group)
                .aria_label(label)
                .relative()
                .child(body)
                .child(
                    div()
                        .absolute()
                        .top(px(-COPY_LIFT))
                        .right_0()
                        .child(copy.h(px(COPY_LIFT)).bg(rgb(colors().bg))),
                )
                .into_any_element()
        }
    }

    /// The answer being written, with a still mark that it is not finished.
    /// Move the streaming reveals one frame on, and ask for the next frame
    /// while either trails what has arrived. Once per frame, from `render`.
    pub(super) fn advance_reveals(&mut self, window: &mut Window) {
        let key = self.host.selected.as_deref().unwrap_or_default();
        let now = Instant::now();
        let total = |parsed: Option<&std::sync::Arc<plait_core::markdown::Message>>| {
            parsed.map_or(0, |parsed| plait_core::reveal::total(&parsed.blocks))
        };
        let text = total(self.formatted.live.as_ref());
        let reasoning = total(self.formatted.live_reasoning.as_ref());
        let reveal = &mut self.reveal;
        reveal.text_shown = reveal.text.advance(key, text, now);
        reveal.reasoning_shown = reveal.reasoning.advance(key, reasoning, now);
        if reveal.text.pending(text) || reveal.reasoning.pending(reasoning) {
            window.request_animation_frame();
        }
    }

    /// The part of live `blocks` on screen this frame: whole blocks and a cut last one.
    fn revealed<'a>(&self, shown: usize, blocks: &'a [Block]) -> (&'a [Block], Option<Block>) {
        let (whole, tail) = plait_core::reveal::visible(blocks, shown);
        (&blocks[..whole], tail)
    }

    fn live_stream(&self, text: &str, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let site = Site {
            owner: Owner::Stream,
            row: index,
            block: 0,
        };
        let body = match self.formatted.live.as_ref() {
            Some(parsed) => {
                let (whole, tail) = self.revealed(self.reveal.text_shown, &parsed.blocks);
                self.markdown_with(&site, whole, tail.as_ref(), "stream", cx)
            }
            None => prose().child(text.to_owned()).into_any_element(),
        };
        column()
            .id("live-stream")
            .role(Role::Group)
            .aria_label(format!("Assistant is writing: {text}"))
            .gap_2()
            .child(body)
            .child(
                row()
                    .gap(px(6.))
                    .child(dot(colors().blue))
                    .child(tag("Writing…", colors().muted)),
            )
            .into_any_element()
    }

    fn reasoning(
        &self,
        id: &str,
        text: &str,
        index: usize,
        live: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = live || self.history.expanded.contains(id);
        let site = Site {
            owner: if live {
                Owner::LiveReasoning
            } else {
                Owner::Reasoning(id.to_owned().into())
            },
            row: index,
            block: 0,
        };
        let parsed = if live {
            self.formatted.live_reasoning.as_ref()
        } else {
            self.formatted.reasoning.get(id)
        };
        let key = id.to_owned();
        let caption = if live {
            "Thinking…"
        } else if open {
            "Hide reasoning"
        } else {
            "Show reasoning"
        };
        let heading = row()
            .justify_between()
            .gap_2()
            .child(
                button(SharedString::from(format!("reasoning-{id}")), caption)
                    .aria_label(caption)
                    .child(icon(if open { "down" } else { "right" }))
                    .disabled(live)
                    .when(!live, |el| {
                        el.on_activate(cx, move |this, _, cx| this.toggle_reasoning(&key, cx))
                    }),
            )
            .when(open, |el| {
                el.child(
                    quiet_button(
                        SharedString::from(format!("copy-reasoning-{id}")),
                        "Copy reasoning",
                        row_group(index),
                    )
                    .on_activate(cx, {
                        let text = text.to_owned();
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                        }
                    }),
                )
            });
        column()
            .gap_1()
            .text_color(rgb(colors().muted))
            .child(heading)
            .when(open, |el| {
                el.child(
                    div()
                        .id(SharedString::from(format!("reasoning-body-{id}")))
                        .pl_3()
                        .border_l_1()
                        .border_color(rgb(colors().border))
                        .when(live, |el| el.max_h(px(180.)).overflow_y_scroll())
                        .when_some(parsed, |el, parsed| {
                            let (whole, tail) = if live {
                                self.revealed(self.reveal.reasoning_shown, &parsed.blocks)
                            } else {
                                (&parsed.blocks[..], None)
                            };
                            el.child(self.markdown_with(
                                &site,
                                whole,
                                tail.as_ref(),
                                &format!("reasoning-{id}"),
                                cx,
                            ))
                        })
                        .when(!live, |el| {
                            el.children(self.history_parts(
                                Key::Message(id.to_owned()),
                                Some(HistoryField::Reasoning),
                                cx,
                            ))
                        }),
                )
            })
            .into_any_element()
    }

    fn live_event(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(snapshot) = self.conversation_snapshot() else {
            return div().into_any_element();
        };
        let Some(row) = self
            .live_layout
            .get(index)
            .and_then(|slot| transcript::resolve(snapshot, *slot))
        else {
            return div().into_any_element();
        };
        // Top padding leaves room for a reply's copy control; a reply sits
        // closer to the tool calls that follow it than to what came before.
        let (content, top, bottom) = match row {
            Row::Message(message) if message.role == MessageRole::User => {
                (self.live_message(message, index, cx), 10., 4.)
            }
            Row::Message(message) => (self.live_message(message, index, cx), COPY_LIFT, 6.),
            Row::Streaming(text) => (self.live_stream(text, index, cx), 14., 8.),
            Row::Reasoning(message) => (
                self.reasoning(&message.id, &message.reasoning, index, false, cx),
                6.,
                3.,
            ),
            Row::LiveReasoning(text) => (self.reasoning("live", text, index, true, cx), 6., 3.),
            Row::Tool(tool) => match self.tool_row(tool, index, cx) {
                Some(content) => (content, 1., 1.),
                // Folded into a closed group: the row stays, with no height.
                None => return div().into_any_element(),
            },
        };
        div()
            .group(row_group(index))
            .w_full()
            .px_6()
            .pt(px(top))
            .pb(px(bottom))
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_WIDTH))
                    .mx_auto()
                    .child(content),
            )
            .into_any_element()
    }

    /// The list of rows with what floats over it: pointer handling for text
    /// selection, the way back to the newest row, and a hovered link's address.
    fn live_rows(&self, snapshot: &Snapshot, cx: &mut Context<Self>) -> AnyElement {
        self.reconcile_tool_rows();
        self.resolve_selection();
        let away = self.live_scroll.logical_scroll_top().item_ix < self.live_count;
        let size = (
            snapshot.messages.len() + snapshot.tools.len(),
            snapshot.live_text.len() + snapshot.live_reasoning.len(),
        );
        if !away {
            self.reading.seen.set(size);
        }
        let unseen = self.reading.seen.get() != size;
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(self.selection_surface(cx))
            .child(
                list(
                    self.live_scroll.clone(),
                    cx.processor(|this, index, _, cx| this.live_event(index, cx)),
                )
                .size_full()
                .py_2(),
            )
            .when(away, |el| {
                el.child(
                    // Laid out like a row, so the control ends at the text's right
                    // edge and stays clear of a hovered link's address on the left.
                    div()
                        .absolute()
                        .bottom(px(10.))
                        .left_0()
                        .right_0()
                        .px_6()
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .w_full()
                                .max_w(px(CONTENT_WIDTH))
                                .mx_auto()
                                .child(
                                    toned_button("jump-to-latest", "", Tone::Filled)
                                        .occlude()
                                        .gap(px(6.))
                                        .aria_label(if unseen {
                                            "New activity. Jump to latest"
                                        } else {
                                            "Jump to latest"
                                        })
                                        .when(unseen, |el| el.child(dot(colors().blue)))
                                        .child(if unseen {
                                            "New activity · Jump to latest"
                                        } else {
                                            "Jump to latest"
                                        })
                                        .child(icon("down").text_color(rgb(colors().text)))
                                        .on_activate(cx, |this, window, cx| {
                                            this.jump_latest(window, cx)
                                        }),
                                ),
                        ),
                )
            })
            .when_some(self.reading.hovered_link.clone(), |el, url| {
                el.child(
                    div()
                        .absolute()
                        .bottom(px(6.))
                        .left(px(8.))
                        .max_w(relative(0.6))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(rgb(colors().panel))
                        .border_1()
                        .border_color(rgb(colors().edge))
                        .text_size(px(11.))
                        .text_color(rgb(colors().muted))
                        .truncate()
                        .child(text!(id = "link-destination", format!("Opens {url}"))),
                )
            })
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
                .border_color(rgb(colors().warn_edge))
                .bg(rgb(colors().rail))
                .text_size(px(12.))
                .child(
                    row().child(dot(colors().amber)).child(
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
                        .bg(rgb(colors().bg))
                        .border_1()
                        .border_color(rgb(colors().border))
                        .font_family(MONO)
                        .line_height(px(19.))
                        .child(approval.details.clone()),
                )
                .child(
                    div()
                        .text_color(rgb(colors().amber))
                        .child(approval.risk.clone()),
                )
                .when(!approval.dirty_paths.is_empty(), |el| {
                    el.child(
                        div()
                            .text_color(rgb(colors().muted))
                            .child(format!("Protected dirty paths: {dirty}")),
                    )
                })
                .when(!exact, |el| {
                    el.child(div().text_color(rgb(colors().red)).child(
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
                                .text_color(rgb(colors().amber))
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
            banners.push(banner(colors().red).child(text!(error)));
        }
        if !self.host.connected {
            let connecting = self.host.connecting;
            banners.push(
                banner(colors().amber)
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
        if pending && self.host.protocol() < 3 {
            let ready = self.host.connected && !self.host.submitting && self.session.ready();
            banners.push(
                banner(colors().muted)
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
            .child(self.history_heading(cx))
            .child(if snapshot.is_none() {
                empty(
                    "Loading thread",
                    "The conversation comes from the host. Reconnect if the host is unavailable.",
                )
                .into_any_element()
            } else if self.live_count == 0 {
                empty(
                    "Start this conversation",
                    "Send a message below. Change the model or thinking level between runs.",
                )
                .into_any_element()
            } else if let Some(snapshot) = &snapshot {
                self.live_rows(snapshot, cx)
            } else {
                div().into_any_element()
            });
        if let Some(snapshot) = &snapshot {
            if snapshot.truncated && !self.history.enabled {
                main = main.child(div().px_6().py_1().child(tag(
                    "Bounded preview · older messages or tool output have been clipped by the host.",
                    colors().amber,
                )));
            }
            if let Some((state, failed)) = history::run_state(snapshot, self.history.now) {
                main = main.child(
                    div().px_6().py_2().child(
                        row()
                            .w_full()
                            .max_w(px(CONTENT_WIDTH))
                            .mx_auto()
                            .text_size(px(12.))
                            .text_color(rgb(if failed { colors().red } else { colors().muted }))
                            .child(dot(if failed {
                                colors().red
                            } else if snapshot.status.busy() {
                                colors().blue
                            } else {
                                colors().muted
                            }))
                            .child(state),
                    ),
                );
            }
            if self.history.enabled
                && snapshot.status.busy()
                && (snapshot.live_text.encode_utf16().count() >= 16_383
                    || snapshot.live_reasoning.encode_utf16().count() >= 16_383)
            {
                main = main.child(
                    div().px_6().child(
                        div()
                            .w_full()
                            .max_w(px(CONTENT_WIDTH))
                            .mx_auto()
                            .text_size(px(11.))
                            .text_color(rgb(colors().amber))
                            .child(
                                "Live preview · complete text loads when this response is saved.",
                            ),
                    ),
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
                .children(self.queued_inputs(cx))
                .children(self.thread_files_card(cx))
                .when_some(composer.cloned(), |el, composer| {
                    el.child(
                        div()
                            .w_full()
                            .max_w(px(CONTENT_WIDTH))
                            .mx_auto()
                            .child(composer),
                    )
                })
                .child(if self.host.protocol() >= 3 {
                    self.thread_environment(cx)
                } else {
                    self.environment(
                        Some(model_label),
                        Some((self.approval_note(snapshot.as_deref()), colors().muted)),
                    )
                    .into_any_element()
                }),
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

/// Hover group for one conversation row, so its quiet controls appear together.
fn row_group(row: usize) -> SharedString {
    format!("conversation-row-{row}").into()
}
