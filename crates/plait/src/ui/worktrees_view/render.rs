//! Virtualized graph and selected-checkout detail; consumes prepared state only.
use super::*;

impl Plait {
    pub(in crate::ui) fn worktrees_view(&self, wide: bool, cx: &mut Context<Self>) -> AnyElement {
        let graph = column().flex_1().min_w_0().min_h_0().child(
            uniform_list(
                "worktrees-graph",
                self.worktrees.items.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|i| this.worktree_graph_row(i, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.worktrees.scroll)
            .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
            .flex_1()
            .min_h_0()
            .w_full(),
        );
        let content = if wide {
            row()
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(graph)
                .child(
                    self.worktree_detail(cx)
                        .w(px(340.))
                        .border_l_1()
                        .border_color(rgb(colors().border)),
                )
                .into_any_element()
        } else {
            column()
                .flex_1()
                .min_h_0()
                .child(graph)
                .child(
                    self.worktree_detail(cx)
                        .h(px(245.))
                        .border_t_1()
                        .border_color(rgb(colors().border)),
                )
                .into_any_element()
        };
        column().id("worktrees-view").flex_1().min_h_0()
            .child(self.worktrees_toolbar(cx))
            .when(self.worktrees.loading, |el| {
                el.child(div().px_4().child(a11y_text("worktrees-loading", "Loading")))
            })
            .when_some(self.worktrees.error.clone(), |el, error| {
                el.child(div().p_4().text_color(rgb(colors().muted)).child(a11y_text("worktrees-error", error)))
            })
            .when(self.worktrees.rows.len() == 1, |el| {
                el.child(div().px_4().py_2().text_color(rgb(colors().muted)).child(a11y_text(
                    "worktrees-empty", "No worktrees yet. Choose Own worktree when a project is busy, or New Thread in Worktree."
                )))
            })
            .when_some(self.worktrees.removal.message.clone(), |el, result| {
                el.child(div().px_4().py_2().child(a11y_text("worktrees-result", result)))
            })
            .child(content)
            .when(self.worktrees.graph.more, |el| {
                el.child(div().px_4().child(a11y_text("worktrees-older", "Older commits not shown · 300 commit limit")))
            }).into_any_element()
    }
    fn worktrees_toolbar(&self, cx: &mut Context<Self>) -> Div {
        row()
            .h(px(44.))
            .px_4()
            .gap_3()
            .child(a11y_text("worktrees-heading", "Worktrees"))
            .child(
                div()
                    .flex_1()
                    .text_color(rgb(colors().subtle))
                    .child(a11y_text("worktrees-total", self.worktrees.total.clone())),
            )
            .child(
                button("worktrees-sort-used", "Last used")
                    .tooltip(|_, cx| {
                        hover(
                            "Order lanes by last use; the main checkout stays first.".into(),
                            cx,
                        )
                    })
                    .on_activate(cx, |this, _, cx| this.sort_worktree_lanes(false, cx)),
            )
            .child(
                button("worktrees-sort-size", "Size")
                    .tooltip(|_, cx| {
                        hover(
                            "Order lanes by measured size; the main checkout stays first.".into(),
                            cx,
                        )
                    })
                    .on_activate(cx, |this, _, cx| this.sort_worktree_lanes(true, cx)),
            )
            .child(
                button("worktrees-refresh", "Refresh")
                    .tooltip(|_, cx| {
                        hover("Read worktree state and measure storage again.".into(), cx)
                    })
                    .on_activate(cx, |this, _, cx| this.refresh_worktrees_view(true, cx)),
            )
    }
    fn worktree_graph_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let item = &self.worktrees.items[index];
        let (r, lane) = match item {
            Item::Head(r, l) | Item::Changed(r, l) => (Some(*r), *l),
            Item::Commit(r) => (Some(*r), self.worktrees.graph.rows[*r].lane),
            Item::Detached(l) => (None, *l),
        };
        let node_x = 12. + lane as f32 * 14.;
        let lane = if lane < self.worktrees.rows.len() {
            lane
        } else {
            0
        };
        let width = 24. + self.worktrees.graph.columns as f32 * 14.;
        let mut drawing = div().relative().h(px(28.)).w(px(width)).flex_shrink_0();
        if let Some(r) = r {
            let row = &self.worktrees.graph.rows[r];
            for &l in &row.lines {
                drawing = drawing.child(
                    div()
                        .absolute()
                        .left(px(12. + l as f32 * 14.))
                        .top_0()
                        .h_full()
                        .w(px(1.))
                        .bg(rgb(colors().edge)),
                );
            }
            if matches!(item, Item::Commit(_)) {
                for &(a, b) in &row.joins {
                    drawing = drawing.child(
                        div()
                            .absolute()
                            .left(px(12. + a.min(b) as f32 * 14.))
                            .top(px(14.))
                            .w(px(a.abs_diff(b) as f32 * 14.))
                            .h(px(1.))
                            .bg(rgb(colors().edge)),
                    );
                }
            }
        }
        let changed = matches!(item, Item::Changed(..));
        let node_color = if matches!(item, Item::Head(..)) {
            match self.worktrees.words[lane] {
                "Working" => colors().blue,
                "Approve" | "Interrupted" => colors().amber,
                "Failed" => colors().red,
                _ => colors().muted,
            }
        } else {
            colors().muted
        };
        drawing = drawing.child(graph_mark(
            node_x,
            changed,
            node_color,
            if self.worktrees.selected == lane {
                colors().selected
            } else {
                colors().bg
            },
        ));
        let text = match item {
            Item::Commit(r) => self.worktrees.commit_labels[*r].clone(),
            Item::Changed(..) => format!(
                "{} changed · uncommitted",
                self.worktrees.rows[lane].preservation.changed.unwrap_or(0)
            ),
            Item::Head(..) | Item::Detached(_) => self.worktrees.head_labels[lane].clone(),
        };
        let identity = self.worktrees.rows[lane].path.display();
        let id = match item {
            Item::Head(..) | Item::Detached(_) => format!("worktrees-head-{identity}"),
            Item::Changed(..) => format!("worktrees-changed-{identity}"),
            Item::Commit(r) => format!(
                "worktrees-commit-{}",
                self.worktrees.graph.rows[*r].commit.commit.id
            ),
        };
        let mut line = row()
            .id(SharedString::from(id.clone()))
            .role(Role::Button)
            .aria_label(text.clone())
            .focusable()
            .tab_stop(true)
            .h(px(28.))
            .w_full()
            .min_w(px(width + 300.))
            .focus_visible(|s| {
                s.bg(rgb(colors().selected))
                    .border_color(rgb(colors().blue))
                    .border_1()
            })
            .px_2()
            .bg(rgb(if self.worktrees.selected == lane {
                colors().selected
            } else {
                colors().bg
            }))
            .child(drawing)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .child(a11y_text(format!("{id}-text"), text)),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.select_worktree_lane(lane, cx);
                    window.prevent_default();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_activate(cx, move |this, _, cx| {
                this.select_worktree_lane(lane, cx);
            });
        if matches!(item, Item::Head(..) | Item::Detached(_)) {
            line = line.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(12.))
                    .text_color(rgb(colors().subtle))
                    .child(a11y_text(
                        format!("{id}-size"),
                        size_label(self.worktrees.sizes[lane].as_ref()),
                    )),
            );
        }
        line.audited().into_any_element()
    }
    fn worktree_detail(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let panel = column()
            .id("worktrees-detail")
            .p_4()
            .gap_2()
            .overflow_y_scroll();
        let n = self.worktrees.selected;
        let Some(tree) = self.worktrees.rows.get(n) else {
            return panel;
        };
        let path = tree.path.clone();
        let finder = path.clone();
        let terminal = path.clone();
        let drawn = (path.clone(), tree.identity.clone());
        let missing = tree.missing;
        let title = if self.worktrees.words[n].is_empty() {
            self.worktrees.titles[n].clone()
        } else {
            format!("{} · {}", self.worktrees.titles[n], self.worktrees.words[n])
        };
        let facts = &tree.preservation;
        let count = |n: Option<u64>| n.map_or("Unknown".into(), |n| n.to_string());
        let reason=facts.refusal.clone().unwrap_or_else(||"Clean, published, and no commits kept only by this worktree. The host checks again before removal.".into());
        let base = tree.base.as_deref().map(|s| &s[..12]).unwrap_or("Unknown");
        let size = self
            .worktrees
            .sizes
            .get(n)
            .and_then(|s| s.as_ref())
            .and_then(|s| s.as_ref().ok());
        let largest = self.worktrees.largest;
        let bar = row()
            .h(px(6.))
            .flex_shrink_0()
            .w_full()
            .bg(rgb(colors().panel));
        let bar = if let Some(size) = size {
            bar.child(
                div()
                    .h_full()
                    .w(relative(size.source as f32 / largest))
                    .bg(rgb(colors().muted)),
            )
            .child(
                div()
                    .h_full()
                    .w(relative(size.build as f32 / largest))
                    .bg(rgb(colors().edge)),
            )
        } else {
            bar
        };
        let thread = tree
            .thread_id
            .clone()
            .filter(|_| self.worktrees.openable[n]);
        let remove_reason = if facts.refusal.is_some() {
            reason.clone()
        } else if self.worktrees.removal.pending.is_some() {
            self.worktrees
                .removal
                .message
                .clone()
                .unwrap_or_else(|| "The removal is still in progress.".into())
        } else {
            reason.clone()
        };
        panel.child(a11y_text("worktrees-detail-title",title))
            .child(row().flex_wrap().gap_2()
                .when_some(thread,|el,id|el.child(button("worktrees-open","Open").tooltip(|_,cx|hover("Bring this worktree’s thread forward.".into(),cx)).on_activate(cx,move |this,window,cx|this.select_host_thread(id.clone(),window,cx))))
                .child(button("worktrees-finder","Finder").disabled(tree.missing).tooltip(|_,cx|hover("Show this checkout in Finder.".into(),cx)).on_activate(cx,move |this,_,cx| { if !missing { this.hand_off(plait_core::handoff::show_in_finder(&finder),cx); } }))
                .child(button("worktrees-terminal","Terminal").disabled(tree.missing).tooltip(|_,cx|hover("Open Terminal Here using Settings → Editor.".into(),cx)).on_activate(cx,move |this,_,cx| { if !missing { this.hand_off(plait_core::handoff::open_terminal(&this.preferences.value.open_terminals_with,&terminal),cx); } }))
                .when(tree.creation_id.is_some()&&!tree.main&&!tree.removed,|el|el.child(button("worktrees-remove","Remove").when(cfg!(test), |el| el.tooltip_show_delay(std::time::Duration::ZERO)).disabled(facts.refusal.is_some() || self.worktrees.removal.pending.is_some() || self.worktree_busy || !self.host.connection.connected || self.worktrees.loading || self.worktrees.error.is_some()).text_color(rgb(if facts.refusal.is_some(){colors().subtle}else{colors().text})).tooltip(move |_,cx|hover(remove_reason.clone(),cx)).on_activate(cx,move |this,window,cx|this.ask_remove_view_worktree(&drawn.0,drawn.1.as_ref(),window,cx)))))
            .child(div().text_size(px(12.)).text_color(rgb(colors().muted)).child(a11y_text("worktrees-detail-branch",format!("{} · from {base}",safe_text(tree.branch.as_deref().unwrap_or("Detached"))))))
            .child(div().text_size(px(11.)).text_color(rgb(colors().subtle)).child(a11y_text("worktrees-detail-path",safe_text(&path.display().to_string()))))
            .child(a11y_text("worktrees-detail-facts",format!("{} changed · {} not pushed · {} kept only here",count(facts.changed),count(facts.unpushed),count(facts.exclusive))))
            .child(div().text_size(px(12.)).text_color(rgb(colors().muted)).child(a11y_text("worktrees-detail-reason",safe_text(&reason))))
            .child(bar)
            .child(a11y_text("worktrees-detail-size",size.map_or_else(||size_label(self.worktrees.sizes[n].as_ref()),|s|format!("{} source · {} build output{}",bytes(s.source),bytes(s.build),if s.incomplete {" · lower bound"}else{""}))))
            .child(div().text_size(px(11.)).text_color(rgb(colors().subtle)).child(a11y_text("worktrees-size-note","Logical bytes, including private build cache. Shared Git storage excluded; this is not space reclaimable.")))
            .child(a11y_text("worktrees-last-used",format!("Last used: {}",self.worktrees.ages[n])))
    }
}

// Match the conversation's one-point line, seven-point node and nine-point ring.
fn graph_mark(x: f32, hollow: bool, color: u32, ground: u32) -> Div {
    let size = if hollow { 9. } else { 7. };
    div()
        .absolute()
        .left(px(x + 0.5 - (size + 4.) / 2.))
        .top(px(14. - (size + 4.) / 2.))
        .size(px(size + 4.))
        .rounded_full()
        .bg(rgb(ground))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(size))
                .rounded_full()
                .bg(rgb(color))
                .flex()
                .items_center()
                .justify_center()
                .when(hollow, |el| {
                    el.child(div().size(px(size - 3.)).rounded_full().bg(rgb(ground)))
                }),
        )
}
