use super::{Plait, theme::*};
use gpui::{prelude::*, *};
use plait_core::host::MessageRole;

impl Plait {
    fn live_event(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(snapshot) = &self.host.snapshot else {
            return div().into_any_element();
        };
        let content = if let Some(message) = snapshot.messages.get(index) {
            let user = message.role == MessageRole::User;
            row()
                .when(user, |el| el.justify_end())
                .child(
                    div()
                        .id(("live-message", index))
                        .max_w(relative(if user { 0.85 } else { 1.0 }))
                        .when(user, |el| el.px_4().py_3().rounded_xl().bg(rgb(PANEL)))
                        .role(Role::Group)
                        .aria_label(format!(
                            "{} message: {}",
                            if user { "Your" } else { "Assistant" },
                            message.text
                        ))
                        .text_size(px(14.))
                        .line_height(relative(1.6))
                        .child(message.text.clone()),
                )
                .into_any_element()
        } else {
            let offset = index - snapshot.messages.len();
            let live = usize::from(!snapshot.live_text.is_empty());
            if live == 1 && offset == 0 {
                column()
                    .gap_2()
                    .child(tag("Assistant · streaming", BLUE))
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(relative(1.6))
                            .child(text!(snapshot.live_text.clone())),
                    )
                    .into_any_element()
            } else if let Some(tool) = snapshot.tools.get(offset - live) {
                let expanded = self.expanded_tools.contains(&index);
                column()
                    .gap_2()
                    .child(
                        button(("live-tool", index), "")
                            .aria_label(format!("{} tool · {}", tool.name, tool.status.label()))
                            .aria_expanded(expanded)
                            .child(icon(if expanded { "down" } else { "right" }))
                            .child(div().child(tool.name.clone()))
                            .child(tag(tool.status.label(), BLUE))
                            .on_activate(cx, move |this, _, cx| {
                                if !this.expanded_tools.remove(&index) {
                                    this.expanded_tools.insert(index);
                                }
                                this.live_scroll.splice(index..index + 1, 1);
                                cx.notify();
                            }),
                    )
                    .when(expanded, |el| {
                        el.child(
                            column()
                                .gap_2()
                                .pl_4()
                                .font_family("Menlo")
                                .text_size(px(12.))
                                .child(
                                    div()
                                        .text_color(rgb(MUTED))
                                        .child(text!(tool.input.clone())),
                                )
                                .child(div().child(text!(tool.output.clone()))),
                        )
                    })
                    .into_any_element()
            } else {
                div().into_any_element()
            }
        };
        div()
            .w_full()
            .px_6()
            .py_3()
            .child(div().w_full().max_w(px(820.)).mx_auto().child(content))
            .into_any_element()
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
            |model| format!("{} / {} · saved model", model.provider, model.model_id),
        );
        let pending = self
            .host
            .selected
            .as_ref()
            .is_some_and(|id| self.host.submissions.pending(id).is_some());
        let mut main = column().flex_1().min_w_0().min_h_0()
            .child(row().px_6().py_2().flex_shrink_0().justify_between()
                .child(tag(model_label, MUTED))
                .child(button("reconnect-conversation", if self.host.connecting { "Connecting…" } else { "Reconnect" })
                    .disabled(self.host.connecting)
                    .when(!self.host.connecting, |el| el.on_activate(cx, |this, window, cx| this.reconnect_host(window, cx)))))
            .when_some(self.host.error.clone(), |el, error| el.child(div().px_6().py_2().text_color(rgb(RED)).child(text!(error))))
            .when(!self.host.connected, |el| el.child(div().px_6().py_2().text_color(rgb(AMBER)).child("Disconnected · displayed content may be stale. The host may still be running this thread.")))
            .when(pending, |el| el.child(row().px_6().py_2()
                .child(div().flex_1().child(if self.host.submitting { "Waiting for host acceptance…" } else { "Submission unconfirmed. Retry uses the same request ID and cannot start duplicate work." }))
                .child(button("retry-submit", "Retry safely").disabled(!self.host.connected || self.host.submitting)
                    .when(self.host.connected && !self.host.submitting, |el| el.on_activate(cx, |this, window, cx| this.retry_submission(window, cx))))))
            .child(if snapshot.is_none() {
                empty("Loading thread", "The conversation comes from the host. Reconnect if the host is unavailable.").into_any_element()
            } else if self.live_count == 0 {
                empty("Start this conversation", "Send a message below. The saved provider and model will be used for every turn.").into_any_element()
            } else {
                list(self.live_scroll.clone(), cx.processor(|this, index, _, cx| this.live_event(index, cx)))
                    .flex_1().min_h_0().into_any_element()
            });
        if let Some(snapshot) = &snapshot {
            if snapshot.truncated {
                main = main.child(div().px_6().py_1().child(tag("Bounded preview · older messages or tool output have been clipped by the host.", AMBER)));
            }
            if let Some(error) = &snapshot.error {
                main = main.child(
                    div()
                        .px_6()
                        .py_2()
                        .text_color(rgb(RED))
                        .child(text!(format!("{}: {}", error.code, error.message))),
                );
            }
            if let Some(approval) = &snapshot.pending_approval {
                let exact = approval.details.len() <= 16384 && !approval.details.is_empty();
                let enabled = exact
                    && self.host.connected
                    && !self.host.controlling
                    && !self.host.decided_approvals.contains(&approval.id);
                let allow_id = approval.id.clone();
                let deny_id = approval.id.clone();
                main = main.child(column().flex_shrink_0().px_6().py_3().gap_2().border_t_1().border_color(rgb(BORDER))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Approval required: {}", approval.summary)))
                    .child(div().id("approval-details").role(Role::Group).aria_label(format!("Exact tool arguments: {}. Risk: {}. Protected dirty paths: {}", approval.details, approval.risk, approval.dirty_paths.join(", "))).max_h(px(140.)).overflow_y_scroll().font_family("Menlo").text_size(px(12.)).child(approval.details.clone()))
                    .child(div().text_color(rgb(AMBER)).child(approval.risk.clone()))
                    .when(!approval.dirty_paths.is_empty(), |el| el.child(div().child(format!("Protected dirty paths: {}", approval.dirty_paths.join(", ")))))
                    .when(!exact, |el| el.child(div().text_color(rgb(RED)).child("Exact arguments are unavailable. Approval is disabled; Stop this run.")))
                    .child(row()
                        .child(button("deny-tool", "Deny once").disabled(!enabled)
                            .when(enabled, |el| el.on_activate(cx, move |this, window, cx| this.control_thread(Some((deny_id.clone(), false)), window, cx))))
                        .child(button("allow-tool", "Allow once").disabled(!enabled).text_color(rgb(AMBER))
                            .when(enabled, |el| el.on_activate(cx, move |this, window, cx| this.control_thread(Some((allow_id.clone(), true)), window, cx))))));
            }
        }
        if let Some(composer) = self
            .host
            .selected
            .as_ref()
            .and_then(|id| self.composers.get(id))
        {
            main = main.child(div().flex_shrink_0().px_6().py_3().child(composer.clone()));
        }
        row()
            .gap_0()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(main)
            .when(wide, |el| {
                el.child(
                    column()
                        .w(px(280.))
                        .flex_shrink_0()
                        .border_l_1()
                        .border_color(rgb(BORDER))
                        .bg(rgb(RAIL))
                        .child(
                            row()
                                .h(px(48.))
                                .px_4()
                                .justify_between()
                                .child(div().font_weight(FontWeight::MEDIUM).child("Working tree"))
                                .child(tag("Git · read-only", MINT)),
                        )
                        .child(self.file_panel(true, cx)),
                )
            })
    }
}
