use super::{OpenRepository, Plait, theme::*};
use gpui::{prelude::*, *};
use plait_core::model::Selection;

/// One-line title that gives way to the trailing state.
fn title(text: impl Into<SharedString>) -> Div {
    div().flex_1().min_w_0().truncate().child(text.into())
}

impl Plait {
    fn project_row(&self, pi: usize, expanded: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let project = &self.state.projects[pi];
        nav_row(("project", pi), format!("{} project", project.name), false)
            .role(Role::TreeItem)
            .aria_level(1)
            .aria_expanded(expanded)
            .child(icon(if expanded { "down" } else { "right" }))
            .child(icon("folder").text_color(rgb(MINT)))
            .child(
                title(project.name.clone())
                    .text_color(rgb(TEXT))
                    .font_weight(FontWeight::MEDIUM),
            )
            .child(tag(project.machines.len().to_string(), SUBTLE))
            .on_activate(cx, move |this, _, cx| {
                if !this.state.collapsed_projects.remove(&pi) {
                    this.state.collapsed_projects.insert(pi);
                }
                cx.notify();
            })
    }

    fn machine_row(
        &self,
        pi: usize,
        mi: usize,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let machine = &self.state.projects[pi].machines[mi];
        nav_row(
            ("machine", pi * 100 + mi),
            format!(
                "{}, {}",
                machine.name,
                if machine.local {
                    "local Git"
                } else {
                    "remote fixture, not connected"
                }
            ),
            false,
        )
        .role(Role::TreeItem)
        .aria_level(2)
        .aria_expanded(open)
        .pl(px(16.))
        .child(icon(if open { "down" } else { "right" }))
        .child(icon("machine").text_color(rgb(BLUE)))
        .child(title(machine.name).text_color(rgb(TEXT)))
        .child(tag(
            if machine.local { "local" } else { "fixture" },
            if machine.local { MINT } else { SUBTLE },
        ))
        .on_activate(cx, move |this, _, cx| {
            if !this.state.collapsed_machines.remove(&(pi, mi)) {
                this.state.collapsed_machines.insert((pi, mi));
            }
            cx.notify();
        })
    }

    fn host_thread_row(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let thread = &self.host.threads[index];
        let id = thread.id.clone();
        let selected = self.host.selected.as_ref() == Some(&id);
        nav_row(
            ("host-thread", index),
            format!(
                "Local thread {} · {} · {} / {}",
                id,
                thread.status.label(),
                thread.model.provider,
                thread.model.model_id
            ),
            selected,
        )
        .role(Role::TreeItem)
        .aria_level(3)
        .aria_selected(selected)
        .pl(px(36.))
        .child(icon("thread").text_color(rgb(MINT)))
        .child(title(id.clone()).text_color(rgb(TEXT)))
        .child(tag(thread.status.label(), SUBTLE))
        .child(dot(host_status_color(thread.status)))
        .on_activate(cx, move |this, window, cx| {
            this.select_host_thread(id.clone(), window, cx)
        })
    }

    fn fixture_thread_row(&self, selection: Selection, cx: &mut Context<Self>) -> Stateful<Div> {
        let Selection {
            project: pi,
            machine: mi,
            thread: ti,
        } = selection;
        let thread = &self.state.projects[pi].machines[mi].threads[ti];
        let selected = self.host.selected.is_none() && selection == self.state.selection;
        nav_row(
            ("thread", pi * 10000 + mi * 100 + ti),
            format!("{}, {} (mock)", thread.title, thread.status.label()),
            selected,
        )
        .role(Role::TreeItem)
        .aria_level(3)
        .aria_selected(selected)
        .pl(px(36.))
        .child(icon("thread").text_color(rgb(VIOLET)))
        .child(title(thread.title).text_color(rgb(TEXT)))
        // Sample state is a dot here; the header and accessible name spell it out.
        .child(tag("mock", SUBTLE))
        .child(dot(status_color(thread.status)))
        .on_activate(cx, move |this, window, cx| {
            window.focus(&this.focus, cx);
            this.select_thread(selection, cx)
        })
    }

    fn tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut tree = column()
            .id("project-tree")
            .role(Role::Tree)
            .aria_label("Projects, machines, host threads and labeled fixtures")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_2()
            .py_2()
            .gap_2();
        for (pi, project) in self.state.projects.iter().enumerate() {
            let expanded = !self.state.collapsed_projects.contains(&pi);
            let mut group = column()
                .gap(px(1.))
                .child(self.project_row(pi, expanded, cx));
            if expanded {
                for (mi, machine) in project.machines.iter().enumerate() {
                    let open = !self.state.collapsed_machines.contains(&(pi, mi));
                    group = group.child(self.machine_row(pi, mi, open, cx));
                    if !open {
                        continue;
                    }
                    if machine.local {
                        for index in 0..self.host.threads.len() {
                            group = group.child(self.host_thread_row(index, cx));
                        }
                    }
                    for ti in 0..machine.threads.len() {
                        let selection = Selection {
                            project: pi,
                            machine: mi,
                            thread: ti,
                        };
                        group = group.child(self.fixture_thread_row(selection, cx));
                    }
                }
            }
            tree = tree.child(group);
        }
        tree
    }

    fn actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .flex_shrink_0()
            .px_2()
            .pb_1()
            .gap(px(1.))
            .child(
                nav_row("new-local-thread", "New thread on the local host…", false)
                    .child(icon("plus").text_color(rgb(MINT)))
                    .child(title("New thread").text_color(rgb(TEXT)))
                    .on_activate(cx, |this, window, cx| {
                        window.focus(&this.focus, cx);
                        this.open_new_thread(cx);
                    }),
            )
            .child(
                nav_row("open-repository", "Open repository…", false)
                    .aria_keyshortcuts("Meta+O")
                    .child(icon("folder"))
                    .child(title("Open repository…").text_color(rgb(TEXT)))
                    .child(tag("⌘O", SUBTLE))
                    .on_activate(cx, |this, window, cx| {
                        this.open_repository(&OpenRepository, window, cx)
                    }),
            )
    }

    /// Host state stays visible; remote entries are labeled where they appear.
    fn host_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, tint) = if self.host.connecting {
            ("Connecting to local host…", MUTED)
        } else if self.host.connected {
            ("Local host connected", MINT)
        } else {
            ("Local host disconnected", AMBER)
        };
        row()
            .h(px(40.))
            .pl_4()
            .pr_2()
            .flex_shrink_0()
            .justify_between()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                row()
                    .min_w_0()
                    .child(dot(tint))
                    .child(tag(label, MUTED).truncate()),
            )
            .child(
                icon_button("settings", "sliders", "Settings…")
                    .aria_keyshortcuts("Meta+,")
                    .on_activate(cx, |this, window, cx| this.show_settings(true, window, cx)),
            )
    }

    pub(super) fn navigation(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        column()
            .w(px(if wide { 272. } else { 240. }))
            .flex_shrink_0()
            .bg(rgb(RAIL))
            .border_r_1()
            .border_color(rgb(BORDER))
            .child(
                row().h(px(CHROME_HEIGHT)).px_4().flex_shrink_0().child(
                    row()
                        .id("plait-brand")
                        .role(Role::Image)
                        .aria_label("Plait")
                        .gap(px(10.))
                        .child(img("plait-mark.svg").w(px(18.)).h(px(22.)).flex_shrink_0())
                        .child(
                            img("plait-wordmark.svg")
                                .w(px(50.))
                                .h(px(24.))
                                .flex_shrink_0(),
                        ),
                ),
            )
            .child(self.actions(cx))
            .child(self.tree(cx))
            .child(self.host_footer(cx))
    }
}
