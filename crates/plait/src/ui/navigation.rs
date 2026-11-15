use super::{OpenRepository, Plait, theme::*};
use gpui::{prelude::*, *};
use plait_core::model::Selection;

impl Plait {
    pub(super) fn navigation(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tree = column()
            .id("project-tree")
            .role(Role::Tree)
            .aria_label("Projects, machines, host threads and labeled fixtures")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_2()
            .py_3()
            .gap_2();
        for (pi, project) in self.state.projects.iter().enumerate() {
            let expanded = !self.state.collapsed_projects.contains(&pi);
            let mut group = column().gap_1().child(
                button(("project", pi), "")
                    .h(px(32.))
                    .w_full()
                    .role(Role::TreeItem)
                    .aria_level(1)
                    .aria_label(format!("{} project", project.name))
                    .aria_expanded(expanded)
                    .child(icon(if expanded { "down" } else { "right" }))
                    .child(icon("folder").text_color(rgb(MINT)))
                    .child(
                        div()
                            .flex_1()
                            .text_color(rgb(TEXT))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(project.name.clone()),
                    )
                    .child(tag(project.machines.len().to_string(), MUTED))
                    .on_activate(cx, move |this, _, cx| {
                        if !this.state.collapsed_projects.remove(&pi) {
                            this.state.collapsed_projects.insert(pi);
                        }
                        cx.notify();
                    }),
            );
            if expanded {
                for (mi, machine) in project.machines.iter().enumerate() {
                    let open = !self.state.collapsed_machines.contains(&(pi, mi));
                    group = group.child(
                        button(("machine", pi * 100 + mi), "")
                            .ml_3()
                            .h(px(30.))
                            .role(Role::TreeItem)
                            .aria_level(2)
                            .aria_label(format!(
                                "{}, {}",
                                machine.name,
                                if machine.local {
                                    "local Git"
                                } else {
                                    "remote fixture, not connected"
                                }
                            ))
                            .aria_expanded(open)
                            .child(icon(if open { "down" } else { "right" }))
                            .child(icon("machine").text_color(rgb(BLUE)))
                            .child(div().flex_1().text_color(rgb(TEXT)).child(machine.name))
                            .child(tag(
                                if machine.local { "local" } else { "fixture" },
                                if machine.local { MINT } else { MUTED },
                            ))
                            .on_activate(cx, move |this, _, cx| {
                                if !this.state.collapsed_machines.remove(&(pi, mi)) {
                                    this.state.collapsed_machines.insert((pi, mi));
                                }
                                cx.notify();
                            }),
                    );
                    if open {
                        if machine.local {
                            for (index, thread) in self.host.threads.iter().enumerate() {
                                let id = thread.id.clone();
                                let selected = self.host.selected.as_ref() == Some(&id);
                                group = group.child(
                                    button(("host-thread", index), "")
                                        .ml(px(37.))
                                        .h(px(48.))
                                        .min_w_0()
                                        .role(Role::TreeItem)
                                        .aria_level(3)
                                        .aria_selected(selected)
                                        .aria_label(format!(
                                            "Local thread {} · {} · {} / {}",
                                            id,
                                            thread.status.label(),
                                            thread.model.provider,
                                            thread.model.model_id
                                        ))
                                        .when(selected, |el| {
                                            el.bg(rgb(PANEL)).border_color(rgb(MINT))
                                        })
                                        .child(icon("thread").text_color(rgb(MINT)))
                                        .child(
                                            column()
                                                .min_w_0()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .text_color(rgb(TEXT))
                                                        .child(id.clone()),
                                                )
                                                .child(tag(thread.status.label(), MUTED)),
                                        )
                                        .on_activate(cx, move |this, window, cx| {
                                            this.select_host_thread(id.clone(), window, cx)
                                        }),
                                );
                            }
                        }
                        for (ti, thread) in machine.threads.iter().enumerate() {
                            let selection = Selection {
                                project: pi,
                                machine: mi,
                                thread: ti,
                            };
                            let selected =
                                self.host.selected.is_none() && selection == self.state.selection;
                            group = group.child(
                                button(("thread", pi * 10000 + mi * 100 + ti), "")
                                    .ml(px(37.))
                                    .h(px(43.))
                                    .min_w_0()
                                    .role(Role::TreeItem)
                                    .aria_level(3)
                                    .aria_selected(selected)
                                    .aria_label(format!(
                                        "{}, {} (mock)",
                                        thread.title,
                                        thread.status.label()
                                    ))
                                    .when(selected, |el| {
                                        el.bg(rgb(0x292d2c)).border_color(rgb(0x383f3c))
                                    })
                                    .child(icon("thread").text_color(rgb(VIOLET)))
                                    .child(
                                        column()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_color(rgb(if selected {
                                                        MINT
                                                    } else {
                                                        TEXT
                                                    }))
                                                    .child(thread.title),
                                            )
                                            .child(
                                                row()
                                                    .gap_2()
                                                    .child(dot(status_color(thread.status)))
                                                    .child(tag(
                                                        format!("{} · mock", thread.status.label()),
                                                        MUTED,
                                                    )),
                                            ),
                                    )
                                    .on_activate(cx, move |this, window, cx| {
                                        window.focus(&this.focus, cx);
                                        this.select_thread(selection, cx)
                                    }),
                            );
                        }
                    }
                }
            }
            tree = tree.child(group);
        }
        column()
            .w(px(if wide { 248. } else { 218. }))
            .flex_shrink_0()
            .bg(rgb(RAIL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                row()
                    .h(px(54.))
                    .px_4()
                    .flex_shrink_0()
                    .justify_between()
                    .child(
                        row()
                            .id("plait-brand")
                            .role(Role::Image)
                            .aria_label("Plait")
                            .gap_3()
                            .child(img("plait-mark.svg").w(px(24.)).h(px(30.)).flex_shrink_0())
                            .child(
                                img("plait-wordmark.svg")
                                    .w(px(64.))
                                    .h(px(31.))
                                    .flex_shrink_0(),
                            ),
                    )
                    .child(tag("2", MUTED)),
            )
            .child(tree)
            .child(
                column()
                    .p_3()
                    .gap_3()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .child(
                        button("new-local-thread", "New local thread…")
                            .text_color(rgb(MINT))
                            .on_activate(cx, |this, window, cx| {
                                window.focus(&this.focus, cx);
                                this.open_new_thread(cx);
                            }),
                    )
                    .child(
                        button("settings", "Settings…")
                            .aria_keyshortcuts("Meta+,")
                            .on_activate(cx, |this, window, cx| {
                                window.focus(&this.focus, cx);
                                this.settings_open = true;
                                this.new_thread_open = false;
                                cx.notify();
                            }),
                    )
                    .child(
                        button("open-repository", "Open repository…")
                            .aria_keyshortcuts("Meta+O")
                            .h(px(34.))
                            .bg(rgb(PANEL))
                            .text_color(rgb(TEXT))
                            .on_activate(cx, |this, window, cx| {
                                this.open_repository(&OpenRepository, window, cx)
                            }),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(if self.host.connected {
                                "Local host connected.\nRemote entries remain fixtures."
                            } else {
                                "Local host disconnected.\nFixtures never run an agent."
                            }),
                    ),
            )
    }
}
