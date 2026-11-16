use super::{Plait, theme::*};
use gpui::{prelude::*, *};

impl Plait {
    /// Git for the checkout beside the conversation on wide windows.
    pub(super) fn inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .w(px(280.))
            .flex_shrink_0()
            .border_l_1()
            .border_color(rgb(BORDER))
            .bg(rgb(RAIL))
            .child(
                row()
                    .h(px(36.))
                    .px_3()
                    .flex_shrink_0()
                    .justify_between()
                    .child(section_label("Working tree"))
                    .child(tag("Git · read-only", SUBTLE)),
            )
            .child(self.file_panel(true, cx))
    }

    /// What to say, and offer, when no thread is selected.
    fn idle_prompt(&self) -> (&'static str, String, Option<&'static str>) {
        let machine = &self.state.projects[0].machines[0].name;
        if self.host.connecting {
            (
                "Connecting…",
                format!("Looking for the Plait host on {machine}."),
                None,
            )
        } else if !self.host.connected {
            (
                "Not connected",
                format!(
                    "Threads run in the Plait host on {machine}. Start it with plait-host start, then Reconnect. Closing Plait never stops running work."
                ),
                Some("Reconnect"),
            )
        } else if self.host.threads.is_empty() {
            (
                "No threads yet",
                "Start a thread in this checkout. Every edit and command waits for your approval."
                    .into(),
                Some("New thread"),
            )
        } else {
            (
                "Select a thread",
                "Choose one in the sidebar, or press ⌘1.".into(),
                Some("New thread"),
            )
        }
    }

    /// The workspace before a thread is chosen. Nothing here runs an agent.
    pub(super) fn idle(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, detail, action) = self.idle_prompt();
        // The action sits directly under its message, inside the centered group.
        let mut body = empty(title, detail);
        if let Some(action) = action {
            let reconnect = action == "Reconnect";
            let tone = if reconnect {
                Tone::Filled
            } else {
                Tone::Primary
            };
            body = body.child(
                toned_button("idle-action", action, tone)
                    .mt_2()
                    .on_activate(cx, move |this, window, cx| {
                        if reconnect {
                            this.reconnect_host(window, cx);
                        } else {
                            window.focus(&this.focus, cx);
                            this.open_new_thread(cx);
                        }
                    }),
            );
        }
        row()
            .gap_0()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(
                column().flex_1().min_w_0().min_h_0().child(body).child(
                    div()
                        .flex_shrink_0()
                        .px_6()
                        .pb_3()
                        .child(self.environment(None, None)),
                ),
            )
            .when(wide, |el| el.child(self.inspector(cx)))
    }
}
