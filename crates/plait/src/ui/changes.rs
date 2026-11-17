use super::{OpenRepository, Plait, Refresh, theme::*};
use gpui::{prelude::*, *};
use plait_core::{
    git::WriteOp,
    model::{Change, DiffKind, DiffMode},
    state::Load,
};
use std::path::Path;

const DIFF_LINE_HEIGHT: f32 = 21.;

/// One-letter state and its tint. The accessible name carries the full word.
fn change_mark(label: &str) -> (&'static str, u32) {
    match label {
        "New" => ("N", MINT),
        "Added" => ("A", MINT),
        "Deleted" => ("D", RED),
        "Conflict" => ("!", RED),
        "Renamed" => ("R", BLUE),
        _ => ("M", BLUE),
    }
}

/// File name first, so truncation removes the directory before the name.
/// What staging does to this file, in words. For an unmerged file, staging
/// is how Git records the conflict as resolved.
fn stage_verb(change: &Change, stage: bool) -> &'static str {
    match (stage, change.conflicted()) {
        (true, true) => "Mark resolved",
        (true, false) => "Stage",
        (false, _) => "Unstage",
    }
}

/// Where a diff's hunks stage to, captured once per frame for visible rows.
pub(super) struct HunkTarget {
    path: std::path::PathBuf,
    mode: DiffMode,
    busy: bool,
}

/// Shown on hover beside a hunk's `@@` line; built only for visible rows.
fn hunk_button(
    target: &HunkTarget,
    hunk: usize,
    header: &str,
    cx: &mut Context<Plait>,
) -> Stateful<Div> {
    let stage = target.mode == DiffMode::Unstaged;
    let op = WriteOp::Hunk {
        path: target.path.clone(),
        from: target.mode,
        index: hunk,
        header: header.to_owned(),
    };
    quiet_button(
        ("hunk-stage", hunk),
        if stage { "Stage hunk" } else { "Unstage hunk" },
        "diff-hunk",
    )
    .h(px(DIFF_LINE_HEIGHT - 2.))
    .font_family(".SystemUIFont")
    .text_size(px(11.))
    .disabled(target.busy)
    .on_activate(cx, move |this, _, cx| this.git_write(op.clone(), cx))
}

fn split_path(change: &Change) -> (String, String) {
    let display = change.display_path();
    let path = Path::new(&display);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let parent = path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name {
        Some(name) => (name, parent),
        None => (display, String::new()),
    }
}

impl Plait {
    fn file_row(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let Some(snapshot) = self.state.repository.snapshot.ready() else {
            return div().id(("file", index));
        };
        let change = &snapshot.changes[index];
        let selected = self
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
        let (mark, tint) = change_mark(label);
        let (name, parent) = split_path(change);
        let group = SharedString::from(format!("file-row-{index}"));
        nav_row(
            ("file", index),
            format!(
                "{}, {}, {}{}",
                change.display_path(),
                label,
                if change.staged() { "staged " } else { "" },
                if change.unstaged() { "unstaged" } else { "" }
            ),
            selected,
        )
        .aria_selected(selected)
        .group(group.clone())
        .relative()
        .rounded_none()
        .px_3()
        .child(
            div()
                .w(px(12.))
                .flex_shrink_0()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(tint))
                .child(mark),
        )
        .child(
            row()
                .flex_1()
                .min_w_0()
                .child(div().truncate().text_color(rgb(TEXT)).child(name))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(rgb(SUBTLE))
                        .child(parent),
                ),
        )
        .child(tag(
            if change.staged() && change.unstaged() {
                "index + worktree"
            } else if change.staged() {
                "index"
            } else {
                "worktree"
            },
            SUBTLE,
        ))
        // Anything left in the worktree stages; otherwise the row unstages.
        // It overlays the state tag on hover, so the path keeps its width.
        .child({
            let stage = change.unstaged();
            let verb = stage_verb(change, stage);
            quiet_button(("file-stage", index), verb, group.clone())
                .aria_label(format!("{verb} {}", change.display_path()))
                .absolute()
                .right(px(8.))
                .bg(rgb(PANEL))
                .disabled(self.state.repository.write.busy())
                .on_activate(cx, move |this, _, cx| {
                    this.stage_change(index, stage, cx);
                    cx.stop_propagation();
                })
        })
        .on_click(cx.listener(move |this, _, window, cx| this.select_file(index, mode, window, cx)))
    }

    pub(super) fn file_panel(&self, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let repo = &self.state.repository;
        let mut read = None;
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
                .items_center()
                .child(empty("Cannot read repository", error.clone()))
                .child(
                    toned_button("choose-another", "Choose another repository…", Tone::Filled)
                        .mb_4()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_repository(&OpenRepository, window, cx)
                        })),
                )
                .into_any_element(),
            Load::Ready(snapshot) => {
                let count = snapshot.changes.len();
                read = Some(snapshot.elapsed.as_millis());
                column()
                    .flex_1()
                    .min_h_0()
                    .child(
                        column()
                            .px_3()
                            .pt_1()
                            .pb_2()
                            .gap_1()
                            .flex_shrink_0()
                            .child(
                                row()
                                    .child(icon("branch").text_color(rgb(MUTED)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(snapshot.branch.clone()),
                                    )
                                    .child(tag(
                                        format!(
                                            "{} changed {}",
                                            count,
                                            if count == 1 { "file" } else { "files" }
                                        ),
                                        SUBTLE,
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(SUBTLE))
                                    .truncate()
                                    .child(snapshot.root.to_string_lossy().into_owned()),
                            ),
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
                                range
                                    .map(|index| this.file_row(index, cx))
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.file_scroll)
                        .flex_1()
                        .min_h_0()
                        .into_any_element()
                    })
                    .into_any_element()
            }
        };
        let ready = self.state.repository.snapshot.ready().is_some();
        column()
            .flex_1()
            .min_h_0()
            .child(body)
            .when(!compact && ready, |el| el.child(self.commit_box(cx)))
            .child(
                row()
                    .h(px(36.))
                    .px_2()
                    .flex_shrink_0()
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
                    .child(tag("⌘R", SUBTLE))
                    .child(div().flex_1())
                    .when_some(read, |el, elapsed| {
                        el.child(
                            tag(format!("Read in {elapsed} ms"), SUBTLE)
                                .pr_1()
                                .truncate(),
                        )
                    }),
            )
    }

    fn diff_header(&self, cx: &mut Context<Self>) -> Option<Div> {
        let repo = &self.state.repository;
        let key = repo.selected.as_ref()?;
        let change = repo.snapshot.ready()?.changes.get(key.file)?;
        let index = key.file;
        let mode = |id: &'static str, label: &'static str, mode: DiffMode| {
            segment(id, label, key.mode == mode).on_click(
                cx.listener(move |this, _, window, cx| this.select_file(index, mode, window, cx)),
            )
        };
        Some(
            row()
                .h(px(44.))
                .px_4()
                .gap_3()
                .flex_shrink_0()
                .border_b_1()
                .border_color(rgb(BORDER))
                .child(
                    column()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(change.display_path()),
                        )
                        .when_some(change.original.as_ref(), |el, original| {
                            el.child(
                                tag(
                                    format!("Renamed from {}", original.to_string_lossy()),
                                    SUBTLE,
                                )
                                .truncate(),
                            )
                        }),
                )
                .child(tag("Unified diff", SUBTLE).flex_shrink_0())
                .child({
                    let stage = key.mode == DiffMode::Unstaged;
                    let possible = if stage {
                        change.unstaged()
                    } else {
                        change.staged()
                    };
                    button(
                        "diff-stage",
                        match stage_verb(change, stage) {
                            "Stage" => "Stage file",
                            "Unstage" => "Unstage file",
                            verb => verb,
                        },
                    )
                    .disabled(!possible || repo.write.busy())
                    .on_activate(cx, move |this, _, cx| this.stage_change(index, stage, cx))
                })
                .child(
                    segmented()
                        .child(mode("unstaged", "Unstaged", DiffMode::Unstaged))
                        .child(mode("staged", "Staged", DiffMode::Staged)),
                ),
        )
    }

    fn diff_view(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.state.repository.diff {
            Load::Empty => empty(
                "Nothing to compare",
                "Select a changed file to see its staged and unstaged changes separately.",
            )
            .into_any_element(),
            Load::Loading => empty(
                "Reading diff…",
                "The file preview is loading in the background.",
            )
            .into_any_element(),
            Load::Error(error) => empty(
                "Cannot read diff",
                format!("{error}\nRefresh to read the latest repository state."),
            )
            .into_any_element(),
            Load::Ready(diff) => self.diff_lines(
                "diff-lines",
                diff.clone(),
                &self.diff_scroll,
                Some("⌥↑ / ⌥↓ switch file"),
                self.hunk_target(),
                cx,
            ),
        }
    }

    /// The selected file, if its hunks can be staged one at a time. Renames,
    /// new files, and conflicts stage whole.
    fn hunk_target(&self) -> Option<HunkTarget> {
        let repo = &self.state.repository;
        let key = repo.selected.as_ref()?;
        let change = repo.snapshot.ready()?.changes.get(key.file)?;
        let whole = change.index == b'?' || change.original.is_some() || change.conflicted();
        (!whole).then(|| HunkTarget {
            path: change.path.clone(),
            mode: key.mode,
            busy: repo.write.busy(),
        })
    }

    /// A patch as numbered, colored lines, virtualized. Shared by Changes and History.
    pub(super) fn diff_lines(
        &self,
        id: &'static str,
        diff: std::sync::Arc<plait_core::model::Diff>,
        scroll: &UniformListScrollHandle,
        hint: Option<&'static str>,
        hunks: Option<HunkTarget>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        {
            {
                let count = diff.lines.len();
                let widest = Some(diff.widest_line);
                let elapsed = diff.elapsed;
                let truncated = diff.truncated;
                let gutter = || {
                    div()
                        .w(px(44.))
                        .flex_shrink_0()
                        .text_right()
                        .text_color(rgb(SUBTLE))
                };
                column()
                    .flex_1()
                    .min_h_0()
                    .when(truncated, |el| {
                        el.child(
                            div()
                                .px_4()
                                .py_2()
                                .text_size(px(12.))
                                .bg(rgb(0x3b3224))
                                .text_color(rgb(AMBER))
                                .child(text!(
                                    "Preview limited to 512 KiB / 10,000 lines. Repository files are unchanged."
                                )),
                        )
                    })
                    .child(
                        uniform_list(
                            id,
                            count,
                            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let line = &diff.lines[index];
                                        let (fg, bg) = match line.kind {
                                            DiffKind::Added => (MINT, 0x1d2b27),
                                            DiffKind::Removed => (RED, 0x30232a),
                                            DiffKind::Hunk => (BLUE, 0x1f2733),
                                            DiffKind::Header => (MUTED, RAIL),
                                            DiffKind::Context => (TEXT, BG),
                                        };
                                        row()
                                            .id(("diff-line", index))
                                            .role(Role::Group)
                                            .aria_label(SharedString::from(line.text.clone()))
                                            .gap_0()
                                            .w_full()
                                            .h(px(DIFF_LINE_HEIGHT))
                                            .bg(rgb(bg))
                                            .font_family(MONO)
                                            .text_size(px(12.))
                                            .line_height(px(DIFF_LINE_HEIGHT))
                                            .child(gutter().pr_2().child(
                                                line.old_line
                                                    .map_or(String::new(), |n| n.to_string()),
                                            ))
                                            .child(gutter().pr_3().child(
                                                line.new_line
                                                    .map_or(String::new(), |n| n.to_string()),
                                            ))
                                            .child(
                                                div()
                                                    .flex_shrink_0()
                                                    .pr_3()
                                                    .whitespace_nowrap()
                                                    .text_color(rgb(fg))
                                                    .child(SharedString::from(line.text.clone())),
                                            )
                                            .when_some(
                                                hunks.as_ref().and_then(|target| {
                                                    let hunk = diff.hunks.binary_search(&index).ok()?;
                                                    Some((target, hunk))
                                                }),
                                                |el, (target, hunk)| {
                                                    el.group("diff-hunk").child(hunk_button(
                                                        target,
                                                        hunk,
                                                        &line.text,
                                                        cx,
                                                    ))
                                                },
                                            )
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .with_width_from_item(widest)
                        .with_horizontal_sizing_behavior(
                            ListHorizontalSizingBehavior::Unconstrained,
                        )
                        .track_scroll(scroll)
                        .flex_1()
                        .min_h_0(),
                    )
                    .child(
                        row()
                            .h(px(28.))
                            .px_4()
                            .flex_shrink_0()
                            .justify_between()
                            .text_size(px(11.))
                            .text_color(rgb(SUBTLE))
                            .child(format!("{} patch lines · {} ms", count, elapsed.as_millis()))
                            .children(hint),
                    )
                    .into_any_element()
            }
        }
    }

    pub(super) fn changes(&self, wide: bool, cx: &mut Context<Self>) -> impl IntoElement {
        row()
            .gap_0()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(
                column()
                    .w(px(if wide { 264. } else { 224. }))
                    .flex_shrink_0()
                    .pt_2()
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(RAIL))
                    .child(self.file_panel(false, cx)),
            )
            .child(
                column()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .children(self.diff_header(cx))
                    .child(self.diff_view(cx)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{change_mark, split_path, stage_verb};
    use crate::ui::theme::{BLUE, MINT, RED};
    use plait_core::model::Change;
    use std::path::PathBuf;

    fn change(path: &str) -> Change {
        Change {
            path: PathBuf::from(path),
            original: None,
            index: b' ',
            worktree: b'M',
        }
    }

    #[test]
    fn file_name_leads_and_directory_follows() {
        assert_eq!(
            split_path(&change("crates/plait/src/main.rs")),
            ("main.rs".into(), "crates/plait/src".into())
        );
        assert_eq!(
            split_path(&change("README.md")),
            ("README.md".into(), String::new())
        );
        // Control characters stay escaped, as in the accessible name.
        assert_eq!(
            split_path(&change("notes/a\nb.md")),
            ("a\\nb.md".into(), "notes".into())
        );
    }

    #[test]
    fn every_change_label_has_a_distinct_mark() {
        let marks = ["New", "Added", "Modified", "Deleted", "Renamed", "Conflict"]
            .map(|label| change_mark(label).0);
        for (index, mark) in marks.iter().enumerate() {
            assert!(!marks[..index].contains(mark), "{mark} repeats");
        }
        assert_eq!(change_mark("New").1, MINT);
        assert_eq!(change_mark("Conflict").1, RED);
        assert_eq!(change_mark("Modified").1, BLUE);
    }

    #[test]
    fn staging_an_unmerged_file_is_called_marking_it_resolved() {
        let mut file = change("a.rs");
        file.index = b'U';
        file.worktree = b'U';
        assert_eq!(stage_verb(&file, true), "Mark resolved");
        file.index = b' ';
        file.worktree = b'M';
        assert_eq!(stage_verb(&file, true), "Stage");
        assert_eq!(stage_verb(&file, false), "Unstage");
    }
}
