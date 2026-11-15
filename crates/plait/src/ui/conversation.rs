use super::{Plait, theme::*};
use gpui::{prelude::*, *};

impl Plait {
    fn toggle_tool(&mut self, index: usize, cx: &mut Context<Self>) {
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
                .gap_2()
                .child(
                    button(("tool-toggle", index), "")
                        .h(px(32.))
                        .px_1()
                        .aria_label(format!("{} · mock tool event", event.author))
                        .aria_expanded(expanded)
                        .child(icon(if expanded { "down" } else { "right" }))
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(rgb(MUTED))
                                .child(event.author),
                        )
                        .child(tag("mock", VIOLET))
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(index, cx))),
                )
                .when(expanded, |el| {
                    el.child(
                        div()
                            .ml_2()
                            .pl_4()
                            .py_2()
                            .border_l_1()
                            .border_color(rgb(BORDER))
                            .font_family("Menlo")
                            .text_size(px(12.))
                            .line_height(relative(1.7))
                            .text_color(rgb(MUTED))
                            .child(event.body),
                    )
                })
                .into_any_element()
        } else if user {
            row()
                .justify_end()
                .child(
                    div()
                        .max_w(relative(0.85))
                        .px_4()
                        .py_3()
                        .rounded_xl()
                        .bg(rgb(PANEL))
                        .text_size(px(14.))
                        .line_height(relative(1.6))
                        .child(event.body),
                )
                .into_any_element()
        } else {
            div()
                .text_size(px(14.))
                .line_height(relative(1.7))
                .child(event.body)
                .into_any_element()
        };
        column()
            .id(("event", index))
            .role(Role::Group)
            .aria_label(label)
            .w_full()
            .px_6()
            .py(if event.tool { px(3.) } else { px(16.) })
            .child(div().w_full().max_w(px(820.)).mx_auto().child(content))
            .into_any_element()
    }

    pub(super) fn conversation(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.state.selection;
        let machine = &self.state.projects[s.project].machines[s.machine];
        let empty_thread = machine.threads[s.thread].events.is_empty();
        row().gap_0().flex_1().min_h_0().items_stretch()
            .child(column().flex_1().min_w_0().min_h_0()
                .child(row().px_6().py_3().flex_shrink_0().gap_3()
                    .child(tag("Example thread", VIOLET))
                    .child(tag("No agent is running. Tool events are mock data.", MUTED)))
                .child(if empty_thread {
                    empty("A quiet place to start", "This sample thread has no events. Creating and running real threads is planned for M2.").into_any_element()
                } else {
                    list(self.conversation_scroll.clone(), cx.processor(|this, index, _, cx| {
                        this.conversation_event(index, cx)
                    })).flex_1().min_h_0().into_any_element()
                })
                .child(column().flex_shrink_0().px_6().pt_4().pb_5()
                    .child(column().id("composer-preview").role(Role::Group)
                        .aria_label("Message composer unavailable. Local agent execution is not connected in M1.")
                        .p_4().gap_6().rounded_xl().border_1().border_color(rgb(BORDER))
                        .child(div().text_size(px(15.)).text_color(rgb(MUTED)).child("Message this thread"))
                        .child(row().justify_between()
                            .child(tag("Local agent not connected", MUTED))
                            .child(tag("M1 preview · input unavailable", VIOLET))))))
            .when(wide, |el| el.child(column().w(px(280.)).flex_shrink_0()
                .border_l_1().border_color(rgb(BORDER)).bg(rgb(RAIL))
                .child(row().h(px(48.)).px_4().justify_between()
                    .child(div().font_weight(FontWeight::MEDIUM).child("Working tree"))
                    .child(tag("Git", MINT)))
                .child(if machine.local { self.file_panel(true, cx).into_any_element() } else {
                    empty("Remote sample", "No remote Git data is available. Select a thread on Zaphod to inspect the local repository.").into_any_element()
                })))
    }
}
