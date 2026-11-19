//! Prepared text ranges and one-point strokes; no ancestry work in a frame.
use super::{Graph, Item};
use std::ops::Range;

pub(super) struct LabelStyle {
    pub title: Range<usize>,
    pub state: Range<usize>,
    pub code: Range<usize>,
}

pub(super) fn head_label(
    title: &str,
    state: &str,
    branch: &str,
    facts: &[String],
) -> (String, LabelStyle) {
    let mut text = String::new();
    let title = part(&mut text, title, "");
    let state = part(&mut text, state, "  ");
    let code = part(&mut text, branch, " · ");
    for fact in facts {
        part(&mut text, fact, " · ");
    }
    (text, LabelStyle { title, state, code })
}

fn part(text: &mut String, value: &str, separator: &str) -> Range<usize> {
    if value.is_empty() {
        return text.len()..text.len();
    }
    if !text.is_empty() {
        text.push_str(separator);
    }
    let start = text.len();
    text.push_str(value);
    start..text.len()
}

pub(super) struct Stroke {
    pub lane: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub corner: Option<Corner>,
}

#[derive(Clone, Copy)]
pub(super) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

fn x(lane: usize) -> f32 {
    12. + lane as f32 * 14.
}

pub(super) fn strokes(item: &Item, graph: &Graph) -> Vec<Stroke> {
    let index = match item {
        Item::Head(i, _) | Item::Changed(i, _) | Item::Commit(i) => *i,
        Item::Detached(_) => return Vec::new(),
    };
    let row = &graph.rows[index];
    let joins = if matches!(item, Item::Commit(_)) {
        row.joins.as_slice()
    } else {
        &[]
    };
    let mut strokes = Vec::new();
    for &lane in &row.lines {
        let mut top: f32 = 0.;
        let mut bottom: f32 = 28.;
        for &(from, to) in joins {
            if row.lane == from && lane == to {
                top = 20.;
            } else if row.lane != from && lane == from {
                bottom = 8.;
            }
        }
        if bottom > top {
            strokes.push(Stroke {
                lane,
                x: x(lane),
                y: top,
                width: 1.,
                height: bottom - top,
                corner: None,
            });
        }
    }
    for &(from, to) in joins {
        let merge = row.lane == from;
        let lane = if merge { to } else { from };
        let right = to > from;
        let left = x(from.min(to));
        let width = x(from.max(to)) - left;
        let corner = match (merge, right) {
            (true, true) => Corner::TopRight,
            (true, false) => Corner::TopLeft,
            (false, true) => Corner::BottomLeft,
            (false, false) => Corner::BottomRight,
        };
        let corner_left = matches!(corner, Corner::TopLeft | Corner::BottomLeft);
        strokes.push(Stroke {
            lane,
            x: left + if corner_left { 6. } else { 0. },
            y: 14.,
            width: width - 6.,
            height: 1.,
            corner: None,
        });
        strokes.push(Stroke {
            lane,
            x: if corner_left { left } else { left + width - 6. },
            y: if merge { 14. } else { 8. },
            width: 7.,
            height: 7.,
            corner: Some(corner),
        });
    }
    strokes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_labels_omit_missing_parts_without_empty_slots() {
        for (title, state, branch, facts, expected) in [
            (
                "Main checkout",
                "",
                "main",
                vec!["Clean"],
                "Main checkout · main · Clean",
            ),
            (
                "External worktree",
                "",
                "topic",
                vec!["3 changed"],
                "External worktree · topic · 3 changed",
            ),
            (
                "Thread unavailable",
                "",
                "topic",
                vec![],
                "Thread unavailable · topic",
            ),
            (
                "Review files",
                "Ready",
                "Detached",
                vec![],
                "Review files  Ready · Detached",
            ),
            (
                "External worktree",
                "",
                "Detached",
                vec![],
                "External worktree · Detached",
            ),
            (
                "Réparer",
                "Working",
                "sujet-é",
                vec!["", "2 not pushed"],
                "Réparer  Working · sujet-é · 2 not pushed",
            ),
            ("Thread unavailable", "", "", vec![], "Thread unavailable"),
        ] {
            let facts: Vec<_> = facts.into_iter().map(str::to_owned).collect();
            let (label, style) = head_label(title, state, branch, &facts);
            assert_eq!(label, expected);
            assert_eq!(&label[style.title], title);
            assert_eq!(&label[style.state], state);
            assert_eq!(&label[style.code], branch);
        }
    }
}
