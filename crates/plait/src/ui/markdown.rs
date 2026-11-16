//! Presentation for precomputed native Markdown blocks. Parsing never runs here.

use super::{Plait, theme::*};
use gpui::{prelude::*, *};
use plait_core::markdown::{Kind, Message};

impl Plait {
    pub(super) fn markdown(
        &self,
        owner: &str,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let blocks = message
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| {
                if let Kind::Code(language) = &block.kind {
                    let copy = block.text.clone();
                    return column()
                        .w_full()
                        .min_w_0()
                        .rounded_lg()
                        .bg(rgb(RAIL))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .overflow_hidden()
                        .child(
                            row()
                                .px_3()
                                .h(px(28.))
                                .justify_between()
                                .child(tag(
                                    if language.is_empty() {
                                        "Code"
                                    } else {
                                        language
                                    },
                                    SUBTLE,
                                ))
                                .child(
                                    button(
                                        SharedString::from(format!("copy-code-{owner}-{index}")),
                                        "Copy code",
                                    )
                                    .aria_label("Copy code block")
                                    .on_activate(
                                        cx,
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.clone(),
                                            ))
                                        },
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("code-{owner}-{index}")))
                                .px_3()
                                .py_2()
                                .w_full()
                                .overflow_x_scroll()
                                .font_family(MONO)
                                .text_size(px(12.))
                                .line_height(px(19.))
                                .whitespace_nowrap()
                                .child(block.text.clone()),
                        )
                        .into_any_element();
                }
                if block.kind == Kind::Rule {
                    return div()
                        .w_full()
                        .my_2()
                        .border_b_1()
                        .border_color(rgb(BORDER))
                        .into_any_element();
                }
                let text = StyledText::new(block.text.clone())
                    .with_highlights(block.spans.iter().map(|(range, style)| {
                        (
                            range.clone(),
                            HighlightStyle {
                                font_weight: style.strong.then_some(FontWeight::BOLD),
                                font_style: style.emphasis.then_some(FontStyle::Italic),
                                background_color: style.code.then(|| rgb(PANEL).into()),
                                color: style.code.then(|| rgb(MINT).into()),
                                ..HighlightStyle::default()
                            },
                        )
                    }))
                    .with_font_family_overrides(
                        block
                            .spans
                            .iter()
                            .filter(|(_, style)| style.code)
                            .map(|(range, _)| (range.clone(), SharedString::from(MONO))),
                    );
                let mut body = prose().w_full().min_w_0();
                match block.kind {
                    Kind::Heading(level) => {
                        body = body
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(match level {
                                1 => 18.,
                                2 => 16.,
                                _ => 14.,
                            }));
                    }
                    Kind::Quote => {
                        body = body
                            .pl_3()
                            .border_l_2()
                            .border_color(rgb(EDGE))
                            .text_color(rgb(MUTED))
                    }
                    Kind::List(depth) => body = body.pl(px(depth as f32 * 12.)),
                    _ => {}
                }
                body.child(text).into_any_element()
            })
            .collect::<Vec<_>>();
        column()
            .w_full()
            .min_w_0()
            .gap_2()
            .children(blocks)
            .into_any_element()
    }
}
