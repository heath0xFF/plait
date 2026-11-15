use gpui::{prelude::*, *};
use plait::model::ThreadStatus;
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

pub fn row() -> Div {
    div().flex().items_center().gap_2()
}
pub fn column() -> Div {
    div().flex().flex_col()
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    let label = label.into();
    row()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .focusable()
        .tab_stop(true)
        .cursor_pointer()
        .rounded_md()
        .px_2()
        .h(px(30.))
        .border_1()
        .border_color(gpui::transparent_black())
        .text_size(px(12.))
        .text_color(rgb(MUTED))
        .hover(|s| s.bg(rgb(0x2b3039)).text_color(rgb(TEXT)))
        .focus_visible(|s| s.border_color(rgb(BLUE)))
        .child(label)
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
        .p_8()
        .gap_3()
        .child(div().text_size(px(18.)).text_color(rgb(TEXT)).child(title))
        .child(
            div()
                .max_w(px(420.))
                .text_size(px(13.))
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
