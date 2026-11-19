//! Prepared text ranges and one-point strokes; no ancestry work in a frame.
use super::{Graph, Item};
use std::ops::Range;

pub(super) struct LabelStyle {
    pub title: Range<usize>,
    pub state: Range<usize>,
    pub code: Range<usize>,
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
