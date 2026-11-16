use gpui::{prelude::*, *};
use plait_core::{host, model::ThreadStatus};
use std::borrow::Cow;

pub const BG: u32 = 0x171819;
pub const RAIL: u32 = 0x1c1d1f;
pub const PANEL: u32 = 0x26282b;
pub const BORDER: u32 = 0x303238;
pub const TEXT: u32 = 0xe8eaf0;
pub const MUTED: u32 = 0x9da5b4;
pub const MINT: u32 = 0x94dec5;
pub const BLUE: u32 = 0x93b8f5;
pub const VIOLET: u32 = 0xc0adf3;
pub const RED: u32 = 0xf0a6ac;
pub const AMBER: u32 = 0xe4c58d;
/// Section labels, hints, and counts: quieter than MUTED, still readable.
pub const SUBTLE: u32 = 0x7f8696;
/// Selected navigation and list rows. Neutral, so accents stay meaningful.
pub const SELECTED: u32 = 0x2b2d31;
/// Outline of raised controls such as the composer and approval card.
pub const EDGE: u32 = 0x3a3d45;
/// Translucent so one hover step reads the same on every surface.
pub const HOVER: u32 = 0xffffff0d;

/// Readable column for conversation text and the composer.
pub const CONTENT_WIDTH: f32 = 760.;
/// Shared chrome row: the sidebar brand row and the workspace header align.
pub const CHROME_HEIGHT: f32 = 44.;
/// Compact row for navigation, changed files, and model choices.
pub const ROW_HEIGHT: f32 = 28.;

#[cfg(target_os = "macos")]
pub const MONO: &str = "Menlo";
#[cfg(not(target_os = "macos"))]
pub const MONO: &str = "DejaVu Sans Mono";

pub fn row() -> Div {
    div().flex().items_center().gap_2()
}
pub fn column() -> Div {
    div().flex().flex_col()
}

/// How much a control asks for attention. GPUI accepts one hover style per
/// element, so the tone is chosen when the control is built.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Quiet until hovered: navigation, toolbars, secondary actions.
    Ghost,
    /// The current row or tab. Pair it with `aria_selected`.
    Selected,
    /// A raised secondary action.
    Filled,
    /// The one affirmative action in a view.
    Primary,
}

/// Focusable, keyboard-reachable base for every original control.
fn control(id: impl Into<ElementId>, name: SharedString, tone: Tone) -> Stateful<Div> {
    let base = row()
        .id(id)
        .role(Role::Button)
        .aria_label(name)
        .focusable()
        .tab_stop(true)
        .cursor_pointer()
        .rounded_md()
        .border_1()
        .border_color(gpui::transparent_black())
        .focus_visible(|s| s.border_color(rgb(BLUE)));
    match tone {
        Tone::Ghost => base
            .text_color(rgb(MUTED))
            .hover(|s| s.bg(rgba(HOVER)).text_color(rgb(TEXT))),
        Tone::Selected => base.bg(rgb(SELECTED)).text_color(rgb(TEXT)),
        Tone::Filled => base
            .bg(rgb(PANEL))
            .border_color(rgb(EDGE))
            .text_color(rgb(TEXT))
            .hover(|s| s.bg(rgb(0x2f3136))),
        Tone::Primary => base
            .bg(rgb(TEXT))
            .text_color(rgb(BG))
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.bg(rgb(0xffffff))),
    }
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    toned_button(id, label, Tone::Ghost)
}

pub fn toned_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tone: Tone,
) -> Stateful<Div> {
    let label = label.into();
    let raised = matches!(tone, Tone::Filled | Tone::Primary);
    control(id, label.clone(), tone)
        .h(px(ROW_HEIGHT))
        .flex_shrink_0()
        .text_size(px(12.))
        .map(|el| if raised { el.px_3() } else { el.px_2() })
        // Callers that pass no label supply their own children.
        .when(!label.is_empty(), |el| el.child(label))
}

/// Square glyph control. The name is the accessible label; no text is drawn.
pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: &'static str,
    name: impl Into<SharedString>,
) -> Stateful<Div> {
    toned_icon_button(id, glyph, name, Tone::Ghost)
}

pub fn toned_icon_button(
    id: impl Into<ElementId>,
    glyph: &'static str,
    name: impl Into<SharedString>,
    tone: Tone,
) -> Stateful<Div> {
    // GPUI tints a glyph with its own color, not the control's text color.
    let tint = match tone {
        Tone::Ghost => MUTED,
        Tone::Selected | Tone::Filled => TEXT,
        Tone::Primary => BG,
    };
    control(id, name.into(), tone)
        .size(px(ROW_HEIGHT))
        .flex_shrink_0()
        .justify_center()
        .child(icon(glyph).text_color(rgb(tint)))
}

/// Full-width row for the navigation rail and lists. Callers add the content,
/// and `aria_selected` where the row's role supports selection.
pub fn nav_row(
    id: impl Into<ElementId>,
    name: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    let tone = if selected {
        Tone::Selected
    } else {
        Tone::Ghost
    };
    control(id, name.into(), tone)
        .w_full()
        .min_w_0()
        .h(px(ROW_HEIGHT))
        .px_2()
        .gap(px(6.))
        .text_size(px(13.))
}

pub trait ButtonAction {
    fn on_activate<T: 'static>(
        self,
        cx: &Context<T>,
        action: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    ) -> Self;
}

impl ButtonAction for Stateful<Div> {
    fn on_activate<T: 'static>(
        self,
        cx: &Context<T>,
        action: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
    ) -> Self {
        let action = std::rc::Rc::new(action);
        let click = action.clone();
        let entity = cx.weak_entity();
        self.on_click(cx.listener(move |this, _, window, cx| click(this, window, cx)))
            // Bind identity directly; GPUI's fallback simulates a click at cached bounds.
            .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                let _ = entity.update(cx, |this, cx| action(this, window, cx));
            })
    }
}

pub trait DisabledControl {
    fn disabled(self, disabled: bool) -> Self;
}

impl DisabledControl for Stateful<Div> {
    fn disabled(self, disabled: bool) -> Self {
        self.when(disabled, |el| {
            el.opacity(0.5)
                .cursor_default()
                .a11y_synthetic_children(|builder| {
                    builder.parent_node().set_disabled();
                })
        })
    }
}

pub fn tag(text: impl Into<SharedString>, color: u32) -> Div {
    let text = text.into();
    div()
        .text_size(px(11.))
        .text_color(rgb(color))
        .child(text!(id = text.clone(), text))
}

pub fn status_color(status: ThreadStatus) -> u32 {
    match status {
        ThreadStatus::Ready => MINT,
        ThreadStatus::Working => BLUE,
        ThreadStatus::NeedsInput => AMBER,
        ThreadStatus::Complete => VIOLET,
    }
}

pub fn host_status_color(status: host::ThreadStatus) -> u32 {
    match status {
        host::ThreadStatus::Idle => MINT,
        host::ThreadStatus::Running => BLUE,
        host::ThreadStatus::AwaitingApproval | host::ThreadStatus::Interrupted => AMBER,
        host::ThreadStatus::Stopped => MUTED,
        host::ThreadStatus::Error => RED,
    }
}

/// Small uppercase-free heading above a group, in the manner of a list label.
pub fn section_label(text: impl Into<SharedString>) -> Div {
    tag(text, SUBTLE).font_weight(FontWeight::MEDIUM)
}

/// Two-option switch. The caller supplies tab buttons with `aria_selected`.
pub fn segmented() -> Div {
    row()
        .gap(px(2.))
        .p(px(2.))
        .flex_shrink_0()
        .rounded_lg()
        .bg(rgb(RAIL))
        .border_1()
        .border_color(rgb(BORDER))
}

pub fn segment(id: impl Into<ElementId>, label: &'static str, selected: bool) -> Stateful<Div> {
    let tone = if selected {
        Tone::Selected
    } else {
        Tone::Ghost
    };
    toned_button(id, label, tone)
        .h(px(24.))
        .role(Role::Tab)
        .aria_selected(selected)
}

/// Open assistant text on the conversation surface.
pub fn prose() -> Div {
    div().text_size(px(13.)).line_height(px(21.))
}

/// Quiet, right-aligned user message.
pub fn bubble() -> Div {
    prose()
        .max_w(relative(0.8))
        .px(px(14.))
        .py_2()
        .rounded_xl()
        .bg(rgb(PANEL))
}

/// Pulls a disclosure row left so its chevron sits on the text edge.
pub const TOOL_INSET: f32 = 5.;

/// Indented monospace block for tool input, output, and exact arguments.
pub fn code_block() -> Div {
    column()
        .gap_1()
        .ml(px(6.))
        .pl_4()
        .py_1()
        .border_l_1()
        .border_color(rgb(BORDER))
        .font_family(MONO)
        .text_size(px(12.))
        .line_height(px(19.))
        .text_color(rgb(MUTED))
}

pub fn dot(color: u32) -> Div {
    div()
        .size(px(6.))
        .flex_shrink_0()
        .rounded_full()
        .bg(rgb(color))
}

pub fn icon(name: &'static str) -> Svg {
    svg()
        .path(name)
        .size(px(14.))
        .flex_shrink_0()
        .text_color(rgb(MUTED))
}

pub fn empty(title: &'static str, detail: impl Into<SharedString>) -> Stateful<Div> {
    let detail = detail.into();
    column()
        .id(title)
        .role(Role::Group)
        .aria_label(format!("{title}. {detail}"))
        .flex_1()
        .min_h_0()
        .justify_center()
        .items_center()
        .p_6()
        .gap_2()
        .child(
            div()
                .text_size(px(15.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(TEXT))
                .child(title),
        )
        .child(
            div()
                .max_w(px(380.))
                .text_center()
                .text_size(px(12.))
                .line_height(px(19.))
                .text_color(rgb(MUTED))
                .child(detail),
        )
}

pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let svg = match path {
            "plait-mark.svg" => {
                include_str!("../../../../assets/branding/marks/plait-mark-gradient.svg")
            }
            "plait-wordmark.svg" => {
                include_str!("../../../../assets/branding/wordmarks/plait-wordmark-light.svg")
            }
            "down" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="m4 6 4 4 4-4" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
            }
            "right" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="m6 4 4 4-4 4" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
            }
            // Original stroke glyphs for controls; category glyphs come from the brand kit.
            "plus" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round"/></svg>"#
            }
            "sliders" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.4" stroke-linecap="round"><path d="M2.5 5h5.2M11.3 5h2.2M2.5 11h2.2M8.3 11h5.2"/><circle cx="9.5" cy="5" r="1.8"/><circle cx="6.5" cy="11" r="1.8"/></g></svg>"#
            }
            "panel" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.4" stroke-linejoin="round"><rect x="2.5" y="3" width="11" height="10" rx="2"/><path d="M6.5 3v10"/></g></svg>"#
            }
            "send" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 12.5v-9M4.5 7 8 3.5 11.5 7" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
            }
            "stop" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect x="4.5" y="4.5" width="7" height="7" rx="1.5" fill="black"/></svg>"#
            }
            "refresh" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M13 8a5 5 0 1 1-1.6-3.7"/><path d="M13 2.8v3.4H9.6"/></g></svg>"#
            }
            "folder" => include_str!("../../../../assets/branding/icons/plait-projects.svg"),
            "machine" => include_str!("../../../../assets/branding/icons/plait-machines.svg"),
            "thread" => include_str!("../../../../assets/branding/icons/plait-threads.svg"),
            "branch" => include_str!("../../../../assets/branding/icons/plait-git.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(svg.as_bytes())))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::Assets;
    use gpui::{AssetSource, SvgRenderer};
    use std::sync::Arc;

    #[test]
    fn embedded_icons_render_and_brand_keeps_color() {
        let renderer = SvgRenderer::new(Arc::new(Assets));
        for name in [
            "plait-mark.svg",
            "plait-wordmark.svg",
            "folder",
            "machine",
            "thread",
            "branch",
            "down",
            "right",
            "plus",
            "sliders",
            "panel",
            "send",
            "stop",
            "refresh",
        ] {
            let bytes = Assets.load(name).unwrap().expect("embedded icon");
            let image = renderer.render_single_frame(&bytes, 1.0).unwrap();
            let (pixels, remainder) = image.as_bytes(0).unwrap().as_chunks::<4>();
            assert!(remainder.is_empty());
            assert!(pixels.iter().any(|pixel| pixel[3] > 0), "{name}");
            if name == "plait-mark.svg" {
                assert!(pixels.iter().any(|pixel| pixel[0] != pixel[1]));
            }
        }
        assert!(Assets.load("missing.svg").unwrap().is_none());
    }
}
