//! One window bar: the rail's top strip and the header share a row with the
//! native traffic lights, and their empty parts move the window.

use super::{Plait, theme::CHROME_HEIGHT};
use gpui::*;

/// AppKit's standard window buttons are taken to be 16 points tall; GPUI reads
/// their real frames, but only while laying them out. It centers them in a
/// titlebar as tall as a button plus twice the vertical offset, so this offset
/// centers them on the 44-point bar.
const BUTTON_HEIGHT: f32 = 16.;

/// Where the traffic lights sit: 14 points in, on the bar's center line.
pub const TRAFFIC_LIGHTS: Point<Pixels> = point(px(14.), px((CHROME_HEIGHT - BUTTON_HEIGHT) / 2.));

/// The header's left padding while the lights sit over it. The three lights
/// span roughly 54 points, and a few more when built with the macOS 26 SDK,
/// so this leaves a gap after them on either.
const LIGHTS_INSET: f32 = 84.;

/// The header's left padding: its usual 8 points, or clear of the lights when
/// the rail is hidden. Full screen hides the lights, so it needs no inset.
pub fn header_inset(sidebar: bool, fullscreen: bool) -> Pixels {
    if sidebar || fullscreen {
        px(8.)
    } else {
        px(LIGHTS_INSET)
    }
}

/// What a left press on the bar does. A control under the pointer takes the
/// press first: focusing it prevents default, so the bar leaves it alone.
#[derive(Debug, PartialEq, Eq)]
enum Press {
    Ignore,
    /// Move the window if the pointer drags before release.
    Arm,
    /// The system's title-bar double-click action (zoom, minimize, or none).
    DoubleClick,
}

fn press(click_count: usize, taken: bool) -> Press {
    match (taken, click_count) {
        (true, _) => Press::Ignore,
        (false, 2) => Press::DoubleClick,
        (false, _) => Press::Arm,
    }
}

impl Plait {
    /// Let the empty parts of `bar` move the window. The move starts on the
    /// first drag, not the press, so a press that never moves stays a click
    /// and a second one can still count as a double-click.
    pub(super) fn window_drag<E: InteractiveElement>(&self, bar: E, cx: &mut Context<Self>) -> E {
        bar.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, window, _| {
                this.window_drag = false;
                match press(event.click_count, window.default_prevented()) {
                    Press::Ignore => {}
                    Press::Arm => this.window_drag = true,
                    Press::DoubleClick => window.titlebar_double_click(),
                }
            }),
        )
        // Any release disarms, wherever it lands, so a later drag that only
        // passes over the bar (a text selection, say) cannot move the window.
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, _| this.window_drag = false),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, _| this.window_drag = false),
        )
        // No notify: the flag never changes what is drawn.
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, _| {
            if this.window_drag && event.pressed_button == Some(MouseButton::Left) {
                this.window_drag = false;
                window.start_window_move();
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{Press, TRAFFIC_LIGHTS, header_inset, press};
    use crate::ui::theme::CHROME_HEIGHT;
    use gpui::px;

    #[test]
    fn the_lights_center_on_the_bar() {
        assert_eq!(TRAFFIC_LIGHTS.x, px(14.));
        assert_eq!(TRAFFIC_LIGHTS.y * 2. + px(16.), px(CHROME_HEIGHT));
    }

    #[test]
    fn the_header_clears_the_lights_only_when_they_sit_over_it() {
        assert_eq!(header_inset(true, false), px(8.));
        assert_eq!(header_inset(true, true), px(8.));
        assert_eq!(header_inset(false, true), px(8.));
        assert!(header_inset(false, false) >= px(76.));
    }

    #[test]
    fn a_press_a_control_took_never_moves_the_window() {
        assert_eq!(press(1, true), Press::Ignore);
        assert_eq!(press(2, true), Press::Ignore);
        assert_eq!(press(1, false), Press::Arm);
        assert_eq!(press(2, false), Press::DoubleClick);
        assert_eq!(press(3, false), Press::Arm);
    }
}
