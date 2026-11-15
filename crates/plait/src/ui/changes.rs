use super::{OpenRepository, Plait, Refresh, theme::*};
use gpui::{prelude::*, *};
use plait::{
    model::{DiffKind, DiffMode},
    state::Load,
};

impl Plait {
    pub(super) fn file_panel(&self, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let repo = &self.state.repository;
        let body = match &repo.snapshot {
            Load::Empty => empty(
                "Choose a repository",
                "Open a local Git working directory to inspect its changes.",
            )
            .into_any_element(),
            Load::Loading => empty(
                "Reading repository…",
                "Git is running in the background. You can keep browsing threads.",
            )
            .into_any_element(),
            Load::Error(error) => column()
                .flex_1()
                .child(empty("Cannot read repository", error.clone()))
                .child(
                    button("choose-another", "Choose another repository…")
                        .mx_3()
                        .mb_4()
                        .text_color(rgb(BLUE))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_repository(&OpenRepository, window, cx)
                        })),
                )
                .into_any_element(),
            Load::Ready(snapshot) => {
                let count = snapshot.changes.len();
                column()
                    .flex_1()
                    .min_h_0()
                    .child(
                        column()
                            .px_4()
                            .pt_2()
                            .pb_4()
                            .gap_2()
                            .child(
                                row().child(icon("branch").text_color(rgb(MINT))).child(
                                    div()
                                        .truncate()
                                        .text_color(rgb(MINT))
                                        .child(snapshot.branch.clone()),
                                ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED))
                                    .truncate()
                                    .child(snapshot.root.to_string_lossy().into_owned()),
                            )
                            .child(tag(
                                format!(
                                    "{} changed {}",
                                    count,
                                    if count == 1 { "file" } else { "files" }
                                ),
                                MUTED,
                            )),
                    )
                    .child(if count == 0 {
                        empty(
                            "Working tree clean",
                            "There are no staged, unstaged, or untracked changes.",
                        )
                        .into_any_element()
                    } else {
                        uniform_list(
                            if compact {
                                "inspector-files"
                            } else {
                                "changed-files"
                            },
                            count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                let Some(snapshot) = this.state.repository.snapshot.ready() else {
                                    return vec![];
                                };
                                range
                                    .map(|index| {
                                        let change = &snapshot.changes[index];
                                        let selected = this
                                            .state
                                            .repository
                                            .selected
                                            .as_ref()
                                            .is_some_and(|key| key.file == index);
                                        let mode = if change.unstaged() {
                                            DiffMode::Unstaged
                                        } else {
                                            DiffMode::Staged
                                        };
                                        let label = change.label();
                                        let color = match label {
                                            "New" | "Added" => MINT,
                                            "Deleted" | "Conflict" => RED,
                                            _ => BLUE,
                                        };
                                        button(("file", index), "")
                                            .w_full()
                                            .h(px(53.))
                                            .rounded_none()
                                            .px_4()
                                            .gap_2()
                                            .aria_label(format!(
                                                "{}, {}, {}{}",
                                                change.display_path(),
                                                label,
                                                if change.staged() { "staged " } else { "" },
                                                if change.unstaged() { "unstaged" } else { "" }
                                            ))
                                            .aria_selected(selected)
                                            .when(selected, |el| el.bg(rgb(0x28312f)))
                                            .child(
                                                column()
                                                    .min_w_0()
                                                    .flex_1()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .text_color(rgb(TEXT))
                                                            .child(change.display_path()),
                                                    )
                                                    .child(
                                                        row()
                                                            .justify_between()
                                                            .child(tag(label, color))
                                                            .child(tag(
                                                                if change.staged()
                                                                    && change.unstaged()
                                                                {
                                                                    "index + worktree"
                                                                } else if change.staged() {
                                                                    "index"
                                                                } else {
                                                                    "worktree"
                                                                },
                                                                MUTED,
                                                            )),
                                                    ),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.select_file(index, mode, window, cx)
                                            }))
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.file_scroll)
                        .flex_1()
                        .min_h_0()
                        .into_any_element()
                    })
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "Status read in {} ms · manual refresh",
                                snapshot.elapsed.as_millis()
                            )),
                    )
                    .into_any_element()
            }
        };
        column().flex_1().min_h_0().child(body).child(
            row()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(rgb(BORDER))
                .child(
                    button(
                        if compact {
                            "refresh-inspector"
                        } else {
                            "refresh-files"
                        },
                        "Refresh",
                    )
                    .aria_keyshortcuts("Meta+R")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.refresh(&Refresh, window, cx)),
                    ),
                )
                .child(tag("⌘R", MUTED)),
        )
    }

    pub(super) fn changes(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.state.local() {
            return empty("Remote Git is not connected", "Marvin and Arthur are fixture machines in M1. Select Zaphod to inspect a real local repository. No connection attempt has been made.").into_any_element();
        }
        let repo = &self.state.repository;
        let mut viewer = column().flex_1().min_w_0().min_h_0();
        if let Some(key) = &repo.selected
            && let Some(snapshot) = repo.snapshot.ready()
            && let Some(change) = snapshot.changes.get(key.file)
        {
            let index = key.file;
            viewer = viewer.child(
                column()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(change.display_path()),
                    )
                    .when_some(change.original.as_ref(), |el, original| {
                        el.child(tag(
                            format!("Renamed from {}", original.to_string_lossy()),
                            MUTED,
                        ))
                    })
                    .child(
                        row()
                            .child(
                                button("unstaged", "Unstaged")
                                    .role(Role::Tab)
                                    .aria_selected(key.mode == DiffMode::Unstaged)
                                    .when(key.mode == DiffMode::Unstaged, |el| {
                                        el.bg(rgb(0x293b35)).text_color(rgb(MINT))
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_file(index, DiffMode::Unstaged, window, cx)
                                    })),
                            )
                            .child(
                                button("staged", "Staged")
                                    .role(Role::Tab)
                                    .aria_selected(key.mode == DiffMode::Staged)
                                    .when(key.mode == DiffMode::Staged, |el| {
                                        el.bg(rgb(0x293b35)).text_color(rgb(MINT))
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_file(index, DiffMode::Staged, window, cx)
                                    })),
                            )
                            .child(tag("Unified diff", MUTED)),
                    ),
            );
        }
        viewer = viewer.child(match &repo.diff {
            Load::Empty => empty("Nothing to compare", "Select a changed file. Plait shows the index and working tree separately, without staging or discarding anything.").into_any_element(),
            Load::Loading => empty("Reading diff…", "The file preview is loading in the background.").into_any_element(),
            Load::Error(error) => empty("Cannot read diff", format!("{error}\nRefresh to read the latest repository state.")).into_any_element(),
            Load::Ready(diff) => {
                let diff = diff.clone();
                let count = diff.lines.len();
                let widest = Some(diff.widest_line);
                let elapsed = diff.elapsed;
                let truncated = diff.truncated;
                column().flex_1().min_h_0()
                    .when(truncated, |el| el.child(div().px_4().py_2().bg(rgb(0x3b3224)).text_color(rgb(AMBER)).child(text!("Preview limited to 512 KiB / 10,000 lines. Repository files are unchanged."))))
                    .child(uniform_list("diff-lines", count, move |range: std::ops::Range<usize>, _, _| {
                        range.map(|index| {
                            let line = &diff.lines[index];
                            let (fg, bg) = match line.kind {
                                DiffKind::Added => (MINT, 0x20312b), DiffKind::Removed => (RED, 0x36262c),
                                DiffKind::Hunk => (BLUE, 0x232d3c), DiffKind::Header => (MUTED, PANEL),
                                DiffKind::Context => (TEXT, BG),
                            };
                            row().id(("diff-line", index)).role(Role::Group).aria_label(SharedString::from(line.text.clone()))
                                .gap_0().w_full().h(px(23.)).bg(rgb(bg)).font_family("Menlo").text_size(px(12.)).line_height(px(23.))
                                .child(div().w(px(44.)).flex_shrink_0().pr_2().text_right().text_color(rgb(MUTED)).child(line.old_line.map_or(String::new(), |n| n.to_string())))
                                .child(div().w(px(44.)).flex_shrink_0().pr_3().text_right().text_color(rgb(MUTED)).child(line.new_line.map_or(String::new(), |n| n.to_string())))
                                .child(div().flex_shrink_0().pr_6().whitespace_nowrap().text_color(rgb(fg)).child(SharedString::from(line.text.clone())))
                        }).collect::<Vec<_>>()
                    }).with_width_from_item(widest).with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                        .track_scroll(&self.diff_scroll).flex_1().min_h_0())
                    .child(row().px_4().py_2().text_size(px(11.)).text_color(rgb(MUTED)).justify_between()
                        .child(format!("{} patch lines · {} ms", count, elapsed.as_millis()))
                        .child("⌥↑ / ⌥↓ switch file"))
                    .into_any_element()
            }
        });
        row()
            .gap_0()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(
                column()
                    .w(px(234.))
                    .flex_shrink_0()
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(0x191b20))
                    .child(self.file_panel(false, cx)),
            )
            .child(viewer)
            .into_any_element()
    }
}
