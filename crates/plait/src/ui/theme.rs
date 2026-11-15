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
            "down" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="m4 6 4 4 4-4" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
            }
            "right" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="m6 4 4 4-4 4" fill="none" stroke="black" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"#
            }
            "folder" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 4h5l1 2h6v7H2Z" fill="none" stroke="black" stroke-width="1.3" stroke-linejoin="round"/></svg>"#
            }
            "machine" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 3h12v8H2ZM5 14h6M8 11v3" fill="none" stroke="black" stroke-width="1.2" stroke-linejoin="round"/></svg>"#
            }
            "branch" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><g fill="none" stroke="black" stroke-width="1.2"><circle cx="4" cy="3" r="2"/><circle cx="4" cy="13" r="2"/><circle cx="12" cy="4" r="2"/><path d="M4 5v6m0-1c5 0 8-1 8-4"/></g></svg>"#
            }
            "braid" => {
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 28"><g fill="none" stroke="black" stroke-width="2.4" stroke-linecap="round"><path d="M5 3c0 8 14 14 14 22M12 3c0 8-7 14-7 22M19 3c0 8-7 14-7 22"/></g></svg>"#
            }
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(svg.as_bytes())))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
