//! Native conversation projection. No HTML execution, image loading, or URL actions.
//! Parse on the background executor and retain unchanged messages by identity.

use crate::host::Snapshot;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::{collections::HashMap, ops::Range, sync::Arc};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Paragraph,
    Heading(u8),
    Code(String),
    Quote,
    List(usize),
    Rule,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    pub text: String,
    pub spans: Vec<(Range<usize>, Style)>,
}

#[derive(Debug)]
pub struct Message {
    pub source: String,
    pub blocks: Vec<Block>,
}

impl Message {
    pub fn parse(source: &str) -> Self {
        let mut projection = Projection::default();
        for event in Parser::new_ext(source, Options::ENABLE_TASKLISTS) {
            projection.event(event);
            if projection.blocks.len() > 128 {
                // A pathological message remains readable without thousands of native elements.
                return Self {
                    source: source.into(),
                    blocks: vec![Block {
                        text: source.into(),
                        ..Block::default()
                    }],
                };
            }
        }
        projection.flush();
        Self {
            source: source.into(),
            blocks: projection.blocks,
        }
    }
}

#[derive(Default)]
struct Projection {
    blocks: Vec<Block>,
    current: Block,
    strong: usize,
    emphasis: usize,
    quotes: usize,
    lists: Vec<Option<u64>>,
    links: Vec<String>,
}

impl Projection {
    fn flush(&mut self) {
        if !self.current.text.is_empty() || matches!(self.current.kind, Kind::Rule | Kind::Code(_))
        {
            self.blocks.push(std::mem::take(&mut self.current));
        }
    }

    fn text(&mut self, text: &str, code: bool) {
        let start = self.current.text.len();
        self.current.text.push_str(text);
        let end = self.current.text.len();
        let style = Style {
            strong: self.strong > 0,
            emphasis: self.emphasis > 0,
            code,
        };
        if start == end || style == Style::default() {
            return;
        }
        if let Some((range, previous)) = self.current.spans.last_mut()
            && *previous == style
            && range.end == start
        {
            range.end = end;
        } else {
            self.current.spans.push((start..end, style));
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                self.flush();
                self.current.kind = Kind::Code(match kind {
                    CodeBlockKind::Fenced(language) => language
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(40)
                        .collect(),
                    CodeBlockKind::Indented => String::new(),
                });
            }
            Event::Start(Tag::Heading { level, .. }) => {
                self.flush();
                self.current.kind = Kind::Heading(level as u8);
            }
            Event::Start(Tag::Paragraph) => {
                if !matches!(self.current.kind, Kind::List(_)) {
                    self.flush();
                    if self.quotes > 0 {
                        self.current.kind = Kind::Quote;
                    }
                }
            }
            Event::Start(Tag::List(first)) => {
                self.flush();
                self.lists.push(first);
            }
            Event::Start(Tag::Item) => {
                self.flush();
                self.current.kind = Kind::List(self.lists.len().saturating_sub(1).min(8));
                let prefix = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let prefix = format!("{number}. ");
                        *number = number.saturating_add(1);
                        prefix
                    }
                    _ => "• ".into(),
                };
                self.text(&prefix, false);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.flush();
                self.quotes += 1;
            }
            Event::Start(Tag::Strong) => self.strong += 1,
            Event::Start(Tag::Emphasis) => self.emphasis += 1,
            Event::End(TagEnd::Strong) => self.strong = self.strong.saturating_sub(1),
            Event::End(TagEnd::Emphasis) => self.emphasis = self.emphasis.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) => self.links.push(dest_url.into_string()),
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.links.push(dest_url.into_string());
                self.text("[image: ", false);
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if matches!(event, Event::End(TagEnd::Image)) {
                    self.text("]", false);
                }
                if let Some(url) = self.links.pop() {
                    self.text(&format!(" ({url})"), false);
                }
            }
            Event::End(TagEnd::List(_)) => {
                self.flush();
                self.lists.pop();
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush();
                self.quotes = self.quotes.saturating_sub(1);
            }
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::Item
                | TagEnd::HtmlBlock,
            ) => self.flush(),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                self.text(&text, false)
            }
            Event::Code(text) => self.text(&text, true),
            Event::SoftBreak => self.text(" ", false),
            Event::HardBreak => self.text("\n", false),
            Event::Rule => {
                self.flush();
                self.current.kind = Kind::Rule;
                self.flush();
            }
            Event::TaskListMarker(checked) => self.text(if checked { "☑ " } else { "☐ " }, false),
            _ => {}
        }
    }
}

#[derive(Default)]
pub struct Transcript {
    id: String,
    pub messages: HashMap<String, Arc<Message>>,
    pub live: Option<Arc<Message>>,
}

impl Transcript {
    pub fn update(previous: &Self, snapshot: &Snapshot) -> Self {
        let previous = (previous.id == snapshot.id).then_some(previous);
        let messages = snapshot
            .messages
            .iter()
            .map(|message| {
                let cached = previous
                    .and_then(|p| p.messages.get(&message.id))
                    .filter(|cached| cached.source == message.text);
                (
                    message.id.clone(),
                    cached
                        .cloned()
                        .unwrap_or_else(|| Arc::new(Message::parse(&message.text))),
                )
            })
            .collect();
        let live = (!snapshot.live_text.is_empty()).then(|| {
            previous
                .and_then(|p| p.live.as_ref())
                .filter(|cached| cached.source == snapshot.live_text)
                .cloned()
                .unwrap_or_else(|| Arc::new(Message::parse(&snapshot.live_text)))
        });
        Self {
            id: snapshot.id.clone(),
            messages,
            live,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_literal_and_unclosed_streaming_fences_are_readable() {
        for source in [
            "```rust\nfn main() { **literal** }\n```",
            "```rust\nfn main() { **literal** }",
        ] {
            let message = Message::parse(source);
            assert_eq!(message.blocks.len(), 1);
            assert_eq!(message.blocks[0].kind, Kind::Code("rust".into()));
            assert!(message.blocks[0].text.contains("**literal**"));
        }
    }

    #[test]
    fn paragraphs_headings_lists_and_unicode_spans_are_native_data() {
        let message = Message::parse(
            "# Result\n\n**🙂 strong** and `code`.\n\n1. First\n2. Second\n\n> A quote",
        );
        assert_eq!(message.blocks[0].kind, Kind::Heading(1));
        assert!(
            message.blocks[1]
                .spans
                .iter()
                .any(|(_, style)| style.strong)
        );
        assert!(message.blocks[1].spans.iter().any(|(_, style)| style.code));
        assert!(message.blocks.iter().any(|block| block.text == "2. Second"));
        assert!(message.blocks.iter().any(|block| block.kind == Kind::Quote));
        for block in &message.blocks {
            for (range, _) in &block.spans {
                assert!(block.text.is_char_boundary(range.start));
                assert!(block.text.is_char_boundary(range.end));
            }
        }
    }

    #[test]
    fn html_images_and_unsafe_urls_are_text_not_actions() {
        let message = Message::parse(
            "<script>alert(1)</script>\n\n[x](javascript:bad) ![alt](file:///private/file)",
        );
        let rendered: String = message
            .blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect();
        assert!(rendered.contains("<script>"));
        assert!(rendered.contains("javascript:bad"));
        assert!(rendered.contains("file:///private/file"));
    }

    #[test]
    fn excessive_blocks_fall_back_without_losing_content() {
        let source = "line\n\n".repeat(300);
        let message = Message::parse(&source);
        assert_eq!(message.blocks.len(), 1);
        assert_eq!(message.blocks[0].text, source);
    }

    #[test]
    fn streaming_retains_unchanged_message_projections() {
        let mut snapshot: Snapshot = serde_json::from_value(serde_json::json!({
            "id":"thread", "checkout":"/repo", "model":{"provider":"p","model_id":"m"},
            "status":"running", "messages":[{"id":"message","role":"assistant","text":"**hello**"}],
            "live_text":"hello", "tools":[], "pending_approval":null,"error":null,"truncated":false
        }))
        .unwrap();
        let first = Transcript::update(&Transcript::default(), &snapshot);
        snapshot.live_text.push_str(" world");
        let second = Transcript::update(&first, &snapshot);
        assert!(Arc::ptr_eq(
            &first.messages["message"],
            &second.messages["message"]
        ));
        assert!(!Arc::ptr_eq(
            first.live.as_ref().unwrap(),
            second.live.as_ref().unwrap()
        ));
        snapshot.messages.clear();
        assert!(Transcript::update(&second, &snapshot).messages.is_empty());
    }
}
