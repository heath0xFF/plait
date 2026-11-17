use gpui::{prelude::*, *};
use plait_core::host;
use std::borrow::Cow;

pub use plait_core::theme::{Palette, mix};

thread_local! {
    /// The palette every view draws with. Per thread, so parallel UI tests
    /// with different themes do not see each other's colors.
    static CURRENT: std::cell::Cell<&'static Palette> =
        const { std::cell::Cell::new(&plait_core::theme::DEFAULT_DARK) };
}

/// The current theme's colors. Read during render; cheap.
pub fn colors() -> &'static Palette {
    CURRENT.with(std::cell::Cell::get)
}

/// Draw with `palette` from now on. False when it is already in use.
pub fn set_colors(palette: &Palette) -> bool {
    if colors() == palette {
        return false;
    }
    // A few hundred bytes per switch, kept so `colors()` can hand out a
    // plain reference without locking.
    CURRENT.with(|current| current.set(Box::leak(Box::new(*palette))));
    true
}

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
        .focus_visible(|s| s.border_color(rgb(colors().blue)));
    match tone {
        Tone::Ghost => base
            .text_color(rgb(colors().muted))
            .hover(|s| s.bg(rgba(colors().hover)).text_color(rgb(colors().text))),
        Tone::Selected => base
            .bg(rgb(colors().selected))
            .text_color(rgb(colors().text)),
        Tone::Filled => base
            .bg(rgb(colors().panel))
            .border_color(rgb(colors().edge))
            .text_color(rgb(colors().text))
            .hover(|s| s.bg(rgb(colors().raised_hover))),
        Tone::Primary => base
            .bg(rgb(colors().text))
            .text_color(rgb(colors().bg))
            .font_weight(FontWeight::MEDIUM)
            .hover(|s| s.bg(rgb(colors().primary_hover))),
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

/// Secondary action that stays out of sight until the pointer is over its
/// `group` or it takes keyboard focus, so reading is not interrupted by chrome.
pub fn quiet_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    group: impl Into<SharedString>,
) -> Stateful<Div> {
    button(id, label)
        .h(px(22.))
        .opacity(0.)
        .group_hover(group, |s| s.opacity(1.))
        .focus_visible(|s| s.border_color(rgb(colors().blue)).opacity(1.))
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
        Tone::Ghost => colors().muted,
        Tone::Selected | Tone::Filled => colors().text,
        Tone::Primary => colors().bg,
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

/// Rounded, outlined label for metadata: a machine, a state. Callers add children.
pub fn pill() -> Div {
    row()
        .gap(px(5.))
        .h(px(20.))
        .px(px(7.))
        .min_w_0()
        .flex_shrink_0()
        .rounded_full()
        .border_1()
        .border_color(rgb(colors().edge))
        .text_size(px(11.))
        .text_color(rgb(colors().muted))
}

pub fn host_status_color(status: host::ThreadStatus) -> u32 {
    match status {
        host::ThreadStatus::Idle => colors().mint,
        host::ThreadStatus::Running => colors().blue,
        host::ThreadStatus::AwaitingApproval | host::ThreadStatus::Interrupted => colors().amber,
        host::ThreadStatus::Stopped => colors().muted,
        host::ThreadStatus::Error => colors().red,
    }
}

/// Small uppercase-free heading above a group, in the manner of a list label.
pub fn section_label(text: impl Into<SharedString>) -> Div {
    tag(text, colors().subtle).font_weight(FontWeight::MEDIUM)
}

/// Two-option switch. The caller supplies tab buttons with `aria_selected`.
pub fn segmented() -> Div {
    row()
        .gap(px(2.))
        .p(px(2.))
        .flex_shrink_0()
        .rounded_lg()
        .bg(rgb(colors().rail))
        .border_1()
        .border_color(rgb(colors().border))
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

/// Compact on/off filter: outlined when off, filled when on.
pub fn chip(id: impl Into<ElementId>, label: &'static str, on: bool) -> Stateful<Div> {
    let tone = if on { Tone::Selected } else { Tone::Ghost };
    control(id, label.into(), tone)
        .h(px(22.))
        .px(px(9.))
        .flex_shrink_0()
        .rounded_full()
        .text_size(px(11.))
        .when(!on, |el| el.border_color(rgb(colors().border)))
        .when(on, |el| el.border_color(rgb(colors().edge)))
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .child(label)
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
        .bg(rgb(colors().panel))
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
        .border_color(rgb(colors().border))
        .font_family(MONO)
        .text_size(px(12.))
        .line_height(px(19.))
        .text_color(rgb(colors().muted))
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
        .text_color(rgb(colors().muted))
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
                .text_color(rgb(colors().text))
                .child(title),
        )
        .child(
            div()
                .max_w(px(380.))
                .text_center()
                .text_size(px(12.))
                .line_height(px(19.))
                .text_color(rgb(colors().muted))
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
            "more" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><circle cx="3" cy="8" r="1"/><circle cx="8" cy="8" r="1"/><circle cx="13" cy="8" r="1"/></svg>"#
            }
            "close" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path d="m4 4 8 8m0-8-8 8" fill="none" stroke="black" stroke-width="1.4" stroke-linecap="round"/></svg>"#
            }
            "down" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="m4 6 4 4 4-4" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
            }
            "up" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="m4 10 4-4 4 4" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
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
            "panel-right" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.4" stroke-linejoin="round"><rect x="2.5" y="3" width="11" height="10" rx="2"/><path d="M9.5 3v10"/></g></svg>"#
            }
            // Outline and filled forms: the shape says whether a favorite is set.
            "star" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 2.8 9.41 6.46 13.33 6.67 10.28 9.14 11.29 12.93 8 10.8 4.71 12.93 5.72 9.14 2.67 6.67 6.59 6.46Z" fill="none" stroke="black" stroke-width="1.3" stroke-linejoin="round"/></svg>"#
            }
            "star-filled" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 2.8 9.41 6.46 13.33 6.67 10.28 9.14 11.29 12.93 8 10.8 4.71 12.93 5.72 9.14 2.67 6.67 6.59 6.46Z" fill="black" stroke="black" stroke-width="1.3" stroke-linejoin="round"/></svg>"#
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
            // A bell, and the same bell struck through, for alerts on and off.
            "bell" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 10.8V7.4a3.5 3.5 0 0 1 7 0v3.4l1 1.2h-9Z"/><path d="M6.8 13.6a1.3 1.3 0 0 0 2.4 0"/></g></svg>"#
            }
            "bell-off" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 10.8V7.4a3.5 3.5 0 0 1 7 0v3.4l1 1.2h-9Z"/><path d="M6.8 13.6a1.3 1.3 0 0 0 2.4 0"/><path d="m2.5 2.5 11 11"/></g></svg>"#
            }
            "eye" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.4" stroke-linejoin="round"><path d="M1.8 8S4.1 4 8 4s6.2 4 6.2 4-2.3 4-6.2 4S1.8 8 1.8 8Z"/><circle cx="8" cy="8" r="1.8"/></g></svg>"#
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

/// `~/…` for a path under the home directory, so paths stay short. The home
/// directory is read and resolved once; this runs during render.
pub fn home_relative(path: &std::path::Path) -> String {
    static HOME: std::sync::OnceLock<Vec<std::path::PathBuf>> = std::sync::OnceLock::new();
    let homes = HOME.get_or_init(|| {
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
            return Vec::new();
        };
        // Checkouts are canonical, and HOME may not be (`/tmp` is `/private/tmp`).
        let resolved = home
            .canonicalize()
            .ok()
            .filter(|resolved| *resolved != home);
        std::iter::once(home).chain(resolved).collect()
    });
    homes
        .iter()
        .find_map(|home| path.strip_prefix(home).ok())
        .map(|rest| format!("~/{}", rest.display()))
        .unwrap_or_else(|| path.display().to_string())
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
            "panel-right",
            "star",
            "star-filled",
            "send",
            "stop",
            "refresh",
            "bell",
            "bell-off",
            "up",
            "eye",
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
