//! Detects the two completion triggers in a draft: `@` for a file and `/` for
//! a local command. Pure text rules, so they are tested without a window.

/// Longer tokens are prose or pasted data, not something to complete.
const QUERY_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TriggerKind {
    /// `@` at a word boundary: name a file in the checkout.
    Mention,
    /// `/` opening an otherwise empty draft: run a local command.
    Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Trigger {
    pub kind: TriggerKind,
    /// Byte offset of the `@` or `/`.
    pub start: usize,
    /// Text typed after the sigil, up to the cursor.
    pub query: String,
}

/// Characters that may sit directly before `@`, as in `(@src/main.rs)`.
fn opens_a_mention(ch: char) -> bool {
    matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'' | '`')
}

/// The trigger the cursor is completing, if any. `cursor` is a byte offset
/// with nothing selected and no IME composition in progress.
pub(super) fn detect(text: &str, cursor: usize) -> Option<Trigger> {
    let before = text.get(..cursor)?;
    command(text, before).or_else(|| mention(before))
}

fn command(text: &str, before: &str) -> Option<Trigger> {
    let query = before.strip_prefix('/')?;
    // A draft with a space or a second slash is a message or a path.
    if text.len() > QUERY_LIMIT || text[1..].contains(|ch: char| ch.is_whitespace() || ch == '/') {
        return None;
    }
    Some(Trigger {
        kind: TriggerKind::Command,
        start: 0,
        query: query.to_owned(),
    })
}

fn mention(before: &str) -> Option<Trigger> {
    // Walk back over the word under the cursor, never further than a query can be.
    let mut token_start = before.len();
    for (offset, ch) in before.char_indices().rev() {
        if ch.is_whitespace() {
            break;
        }
        if before.len() - offset > QUERY_LIMIT {
            return None;
        }
        token_start = offset;
    }
    let token = &before[token_start..];
    let at = token.find('@')?;
    // `name@host` is an address, not a mention.
    if !token[..at].chars().all(opens_a_mention) {
        return None;
    }
    let query = &token[at + 1..];
    if query.contains('@') {
        return None;
    }
    Some(Trigger {
        kind: TriggerKind::Mention,
        start: token_start + at,
        query: query.to_owned(),
    })
}

/// The text an accepted file inserts in place of `@query`.
///
/// A mention is a fast way to type a path, so the inserted form is the
/// checkout-relative path itself and a trailing space: `src/main.rs `. Nothing
/// is attached and no file is read; the agent receives the same text a person
/// would have typed. A path that a reader could not delimit (it has spaces or
/// other punctuation that ends a word) is wrapped in backticks.
pub(super) fn mention_text(path: &str) -> String {
    let plain = path
        .chars()
        .all(|ch| ch.is_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '+' | '@' | '~'));
    if plain || path.contains('`') {
        format!("{path} ")
    } else {
        format!("`{path}` ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_end(text: &str) -> Option<Trigger> {
        detect(text, text.len())
    }

    fn mention(start: usize, query: &str) -> Option<Trigger> {
        Some(Trigger {
            kind: TriggerKind::Mention,
            start,
            query: query.into(),
        })
    }

    fn command(query: &str) -> Option<Trigger> {
        Some(Trigger {
            kind: TriggerKind::Command,
            start: 0,
            query: query.into(),
        })
    }

    #[test]
    fn mention_opens_at_a_word_boundary_and_tracks_the_query() {
        assert_eq!(at_end("@"), mention(0, ""));
        assert_eq!(at_end("@src/ma"), mention(0, "src/ma"));
        assert_eq!(at_end("look at @comp"), mention(8, "comp"));
        assert_eq!(at_end("first\n@x"), mention(6, "x"));
        assert_eq!(at_end("tab\t@x"), mention(4, "x"));
        assert_eq!(at_end("see (@ui"), mention(5, "ui"));
        assert_eq!(at_end("`@ui"), mention(1, "ui"));
        assert_eq!(at_end("日本語 @ファ"), mention(10, "ファ"));
    }

    #[test]
    fn mention_ignores_addresses_closed_tokens_and_text_without_a_sigil() {
        assert_eq!(at_end("mail me@example.com"), None);
        assert_eq!(at_end("x@"), None);
        assert_eq!(at_end("@one @"), mention(5, ""));
        assert_eq!(at_end("@one "), None);
        assert_eq!(at_end("@a@b"), None);
        assert_eq!(at_end("plain words"), None);
        assert_eq!(at_end(""), None);
        assert_eq!(at_end(&format!("@{}", "a".repeat(QUERY_LIMIT + 1))), None);
        assert!(at_end(&format!("@{}", "a".repeat(QUERY_LIMIT - 1))).is_some());
    }

    #[test]
    fn mention_uses_the_text_before_the_cursor_only() {
        assert_eq!(detect("@src/main.rs", 4), mention(0, "src"));
        assert_eq!(detect("a @b c", 4), mention(2, "b"));
        assert_eq!(detect("a @b c", 5), None);
        assert_eq!(detect("a @b c", 2), None);
        // A cursor inside a character or past the end is never a trigger.
        assert_eq!(detect("@é", 2), None);
        assert_eq!(detect("@a", 9), None);
    }

    #[test]
    fn command_opens_only_when_the_slash_starts_an_otherwise_empty_draft() {
        assert_eq!(at_end("/"), command(""));
        assert_eq!(at_end("/set"), command("set"));
        assert_eq!(detect("/settings", 3), command("se"));
        assert_eq!(at_end("/new "), None);
        assert_eq!(at_end("/new thread"), None);
        assert_eq!(at_end("/etc/hosts"), None);
        assert_eq!(at_end(" /new"), None);
        assert_eq!(at_end("run /new"), None);
        assert_eq!(at_end("a\n/new"), None);
        assert_eq!(detect("/new", 0), None);
        assert_eq!(at_end(&format!("/{}", "a".repeat(QUERY_LIMIT))), None);
    }

    #[test]
    fn a_mention_can_follow_text_but_a_command_cannot() {
        assert_eq!(at_end("/@x"), command("@x"));
        assert_eq!(at_end("see /tmp @x"), mention(9, "x"));
    }

    #[test]
    fn accepted_mentions_insert_the_path_and_quote_only_when_needed() {
        assert_eq!(mention_text("src/main.rs"), "src/main.rs ");
        assert_eq!(mention_text("docs/日本語.md"), "docs/日本語.md ");
        assert_eq!(mention_text("a b/c.rs"), "`a b/c.rs` ");
        assert_eq!(mention_text("notes (old).md"), "`notes (old).md` ");
        assert_eq!(mention_text("odd`name"), "odd`name ");
    }
}
