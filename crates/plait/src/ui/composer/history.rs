//! Prompts already sent in a thread, for recall with Up and Down.
//!
//! The host snapshot supplies the thread's recent user messages. It keeps
//! only the tail of a long prompt, so prompts sent from this composer are
//! also remembered exactly and preferred over a clipped host copy.

use std::sync::Arc;

/// Oldest prompts beyond this are dropped from recall.
const ENTRY_LIMIT: usize = 200;
const LOCAL_LIMIT: usize = 50;
const SOURCE_BYTES: usize = 512 * 1024;
/// The host shows at most this many trailing UTF-16 units of a prompt, and may
/// drop one more unit to avoid splitting a surrogate pair.
const HOST_PREVIEW_UNITS: usize = 4096;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Sent {
    text: String,
    /// How many host prompts existed when this one was sent; places it in
    /// order while the host has not listed it.
    after: usize,
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct PromptHistory {
    host: Vec<String>,
    local: Vec<Sent>,
    #[serde(with = "super::parking::strings")]
    merged: Arc<[String]>,
}

fn clipped_by_host(text: &str) -> bool {
    text.encode_utf16().count() + 1 >= HOST_PREVIEW_UNITS
}

/// Whether the host's `listed` text is the prompt `sent` from here.
fn same_prompt(listed: &str, sent: &str) -> bool {
    listed == sent || (clipped_by_host(listed) && sent.ends_with(listed))
}

impl PromptHistory {
    pub(super) fn retained_bytes(&self) -> usize {
        self.host.capacity() * std::mem::size_of::<String>()
            + self.host.iter().map(String::capacity).sum::<usize>()
            + self.local.capacity() * std::mem::size_of::<Sent>()
            + self.local.iter().map(|s| s.text.capacity()).sum::<usize>()
            + self.merged.len() * std::mem::size_of::<String>()
            + self.merged.iter().map(String::capacity).sum::<usize>()
    }
    /// Replace the host's list, oldest first. Returns whether anything changed;
    /// an unchanged list costs comparisons only, so every snapshot can call this.
    pub fn set_host(&mut self, prompts: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
        let prompts: Vec<_> = prompts.into_iter().collect();
        let mut bytes = 0;
        let keep = prompts
            .iter()
            .rev()
            .take(ENTRY_LIMIT)
            .take_while(|p| {
                bytes += p.as_ref().len();
                bytes <= SOURCE_BYTES
            })
            .count();
        let mut changed = false;
        let mut count = 0;
        for prompt in &prompts[prompts.len().saturating_sub(keep)..] {
            let prompt = prompt.as_ref();
            match self.host.get_mut(count) {
                Some(existing) if existing == prompt => {}
                Some(existing) => {
                    *existing = prompt.to_owned();
                    changed = true;
                }
                None => {
                    self.host.push(prompt.to_owned());
                    changed = true;
                }
            }
            count += 1;
        }
        if self.host.len() > count {
            self.host.truncate(count);
            changed = true;
        }
        if changed {
            self.merge();
        }
        changed
    }

    /// Remember a prompt sent from this composer.
    pub fn record(&mut self, prompt: &str) {
        if prompt.trim().is_empty() {
            return;
        }
        if self.local.len() == LOCAL_LIMIT {
            self.local.remove(0);
        }
        self.local.push(Sent {
            text: prompt.to_owned(),
            after: self.host.len(),
        });
        let mut bytes: usize = self.local.iter().map(|p| p.text.capacity()).sum();
        while bytes > SOURCE_BYTES && self.local.len() > 1 {
            bytes -= self.local.remove(0).text.capacity();
        }
        self.merge();
    }

    /// Sent prompts, oldest first, without blanks or immediate repeats.
    pub fn entries(&self) -> Arc<[String]> {
        self.merged.clone()
    }

    fn merge(&mut self) {
        // Pair each host prompt with the exact local copy it lists, newest first.
        let mut listed = vec![false; self.local.len()];
        let exact: Vec<Option<usize>> = self
            .host
            .iter()
            .rev()
            .map(|host| {
                let index = (0..self.local.len())
                    .rev()
                    .find(|index| !listed[*index] && same_prompt(host, &self.local[*index].text))?;
                listed[index] = true;
                Some(index)
            })
            .collect();
        let mut merged: Vec<&str> = Vec::with_capacity(self.host.len() + self.local.len());
        for position in 0..=self.host.len() {
            // Prompts the host has not listed yet keep the place they were sent at.
            for (index, sent) in self.local.iter().enumerate() {
                if !listed[index] && sent.after.min(self.host.len()) == position {
                    merged.push(&sent.text);
                }
            }
            if let Some(host) = self.host.get(position) {
                let local = exact[self.host.len() - 1 - position];
                merged.push(local.map_or(host.as_str(), |index| &self.local[index].text));
            }
        }
        merged.retain(|prompt| !prompt.trim().is_empty());
        merged.dedup();
        let start = merged.len().saturating_sub(ENTRY_LIMIT);
        self.merged = merged[start..]
            .iter()
            .map(|prompt| (*prompt).to_owned())
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(history: &PromptHistory) -> Vec<String> {
        history.entries().to_vec()
    }

    #[test]
    fn host_prompts_are_listed_oldest_first_without_blanks_or_repeats() {
        let mut history = PromptHistory::default();
        assert!(history.entries().is_empty());
        assert!(history.set_host(["one", " \n", "two", "two", "one"]));
        assert_eq!(entries(&history), ["one", "two", "one"]);
        assert!(!history.set_host(["one", " \n", "two", "two", "one"]));
        assert!(history.set_host(["one"]));
        assert_eq!(entries(&history), ["one"]);
        assert!(history.set_host([""; 0]));
        assert!(history.entries().is_empty());
    }

    #[test]
    fn a_sent_prompt_is_recallable_before_the_host_lists_it_and_not_doubled_after() {
        let mut history = PromptHistory::default();
        history.set_host(["one"]);
        history.record("two");
        history.record("  ");
        assert_eq!(entries(&history), ["one", "two"]);
        history.set_host(["one", "two"]);
        assert_eq!(entries(&history), ["one", "two"]);
        // The same text sent twice is two host messages and one recall entry.
        history.record("two");
        history.set_host(["one", "two", "two", "three"]);
        assert_eq!(entries(&history), ["one", "two", "three"]);
    }

    #[test]
    fn a_prompt_the_host_never_lists_keeps_its_place() {
        let mut history = PromptHistory::default();
        history.set_host(["one"]);
        history.record("rejected");
        history.set_host(["one", "two", "three"]);
        assert_eq!(entries(&history), ["one", "rejected", "two", "three"]);
        // A shorter host preview cannot push it out of range.
        history.set_host([""; 0]);
        assert_eq!(entries(&history), ["rejected"]);
    }

    #[test]
    fn the_exact_local_prompt_replaces_the_host_tail_preview() {
        let long = format!("start {}", "x".repeat(HOST_PREVIEW_UNITS + 100));
        let tail = &long[long.len() - HOST_PREVIEW_UNITS..];
        let mut history = PromptHistory::default();
        history.record(&long);
        history.set_host([tail, "next"]);
        assert_eq!(entries(&history), [long.as_str(), "next"]);
        // A short message that merely ends a longer one is a different prompt.
        let mut history = PromptHistory::default();
        history.record("oh yes");
        history.set_host(["yes"]);
        assert_eq!(entries(&history), ["oh yes", "yes"]);
    }

    #[test]
    fn recall_is_bounded_to_the_newest_prompts() {
        let prompts: Vec<String> = (0..ENTRY_LIMIT + 25).map(|n| format!("p{n}")).collect();
        let mut history = PromptHistory::default();
        history.set_host(prompts.iter().map(String::as_str));
        let listed = history.entries();
        assert_eq!(listed.len(), ENTRY_LIMIT);
        assert_eq!(listed[0], "p25");
        assert_eq!(listed[ENTRY_LIMIT - 1], format!("p{}", ENTRY_LIMIT + 24));
        for n in 0..LOCAL_LIMIT + 5 {
            history.record(&format!("local {n}"));
        }
        assert_eq!(history.local.len(), LOCAL_LIMIT);
    }
}
