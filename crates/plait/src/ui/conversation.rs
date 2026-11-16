use super::{Plait, theme::*};
use gpui::{prelude::*, *};

impl Plait {
    pub(super) fn toggle_tool(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.expanded_tools.remove(&index) {
            self.expanded_tools.insert(index);
        }
        // Invalidate the changed row's height without resetting the conversation's scroll position.
        self.conversation_scroll.splice(index..index + 1, 1);
        cx.notify();
    }

    fn conversation_event(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let s = self.state.selection;
        let event =
            &self.state.projects[s.project].machines[s.machine].threads[s.thread].events[index];
        let expanded = self.expanded_tools.contains(&index);
        let user = event.author == "You";
        let label = if event.tool && !expanded {
            format!("{} · mock tool event", event.author)
        } else {
            format!(
                "{} · {} · mock {}. {}",
                event.author,
                event.time,
                if event.tool { "tool event" } else { "message" },
                event.body
            )
        };
        let content = if event.tool {
            column()
                .gap_1()
                .child(
                    row().child(
                        button(("tool-toggle", index), "")
                            .px_1()
                            .ml(px(-TOOL_INSET))
                            .aria_label(format!("{} · mock tool event", event.author))
                            .aria_expanded(expanded)
                            .child(icon(if expanded { "down" } else { "right" }))
                            .child(div().text_size(px(13.)).child(event.author))
                            .child(tag("mock", VIOLET))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.toggle_tool(index, cx)),
                            ),
                    ),
                )
                .when(expanded, |el| el.child(code_block().child(event.body)))
                .into_any_element()
        } else if user {
            row()
                .justify_end()
                .child(bubble().child(event.body))
                .into_any_element()
        } else {
            prose().child(event.body).into_any_element()
        };
        column()
            .id(("event", index))
            .role(Role::Group)
            .aria_label(label)
            .w_full()
            .px_6()
            .py(if event.tool { px(2.) } else { px(10.) })
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_WIDTH))
                    .mx_auto()
                    .child(content),
            )
            .into_any_element()
    }

    /// Git for the selected machine beside the conversation on wide windows.
    pub(super) fn inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let local = self.state.local();
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
            .child(if local {
                self.file_panel(true, cx).into_any_element()
            } else {
                empty(
                    "Remote sample",
                    "No remote Git data is available. Select a thread on Zaphod to inspect the local repository.",
                )
                .into_any_element()
            })
    }

    /// The composer's place for an example thread: visibly unavailable, no send control.
    fn composer_preview(&self) -> impl IntoElement {
        column()
            .flex_shrink_0()
            .px_6()
            .pt_2()
            .pb_3()
            .gap_2()
            .child(
                column()
                    .id("composer-preview")
                    .role(Role::Group)
                    .aria_label(
                        "Input disabled for fixture threads. Use New thread for real execution.",
                    )
                    .w_full()
                    .max_w(px(CONTENT_WIDTH))
                    .mx_auto()
                    .p_3()
                    .gap_5()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_color(rgb(SUBTLE))
                            .child("Fixture thread · input disabled"),
                    )
                    .child(
                        row()
                            .justify_between()
                            .child(tag("Fixture entries never run an agent", SUBTLE))
                            .child(tag("Example · input unavailable", VIOLET)),
                    ),
            )
            .child(self.environment(None, None))
    }

    pub(super) fn conversation(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.state.selection;
        let machine = &self.state.projects[s.project].machines[s.machine];
        let empty_thread = machine.threads[s.thread].events.is_empty();
        row()
            .gap_0()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(
                column()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        div().px_6().pt_3().pb_1().flex_shrink_0().child(
                            row()
                                .w_full()
                                .max_w(px(CONTENT_WIDTH))
                                .mx_auto()
                                .child(tag("Example thread", VIOLET))
                                .child(tag(
                                    "No agent is running. Tool events are mock data.",
                                    SUBTLE,
                                )),
                        ),
                    )
                    .child(if empty_thread {
                        empty(
                            "A quiet place to start",
                            "This fixture has no events. Use New thread to create a real conversation.",
                        )
                        .into_any_element()
                    } else {
                        list(
                            self.conversation_scroll.clone(),
                            cx.processor(|this, index, _, cx| this.conversation_event(index, cx)),
                        )
                        .flex_1()
                        .min_h_0()
                        .into_any_element()
                    })
                    .child(self.composer_preview()),
            )
            .when(wide, |el| el.child(self.inspector(cx)))
    }
}
