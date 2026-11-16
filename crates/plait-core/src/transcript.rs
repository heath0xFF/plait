use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use crate::host::{Message, Snapshot, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row<'a> {
    Message(&'a Message),
    Streaming(&'a str),
    Tool(&'a Tool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key<'a> {
    Message(&'a str),
    Streaming,
    Tool(&'a str),
}

impl<'a> Row<'a> {
    fn key(self) -> Key<'a> {
        match self {
            Self::Message(message) => Key::Message(&message.id),
            Self::Streaming(_) => Key::Streaming,
            Self::Tool(tool) => Key::Tool(&tool.id),
        }
    }
}

pub fn row_count(snapshot: &Snapshot) -> usize {
    snapshot.messages.len() + usize::from(!snapshot.live_text.is_empty()) + snapshot.tools.len()
}

pub fn row(snapshot: &Snapshot, index: usize) -> Option<Row<'_>> {
    if let Some(message) = snapshot.messages.get(index) {
        return Some(Row::Message(message));
    }
    let offset = index.checked_sub(snapshot.messages.len())?;
    let streaming = usize::from(!snapshot.live_text.is_empty());
    if offset < streaming {
        return Some(Row::Streaming(&snapshot.live_text));
    }
    snapshot.tools.get(offset - streaming).map(Row::Tool)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Splice {
    // Ranges apply in order, after all earlier splices, not to the original list.
    pub range: Range<usize>,
    pub count: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Update {
    Reset(usize),
    Patch {
        splices: Vec<Splice>,
        remeasure: Vec<Range<usize>>,
    },
}

pub fn changes(previous: Option<&Snapshot>, next: Option<&Snapshot>) -> Update {
    let count = next.map_or(0, row_count);
    let (Some(previous), Some(next)) = (previous, next) else {
        return Update::Reset(count);
    };
    if previous.id != next.id || previous.checkout != next.checkout || previous.model != next.model
    {
        return Update::Reset(count);
    }
    let old: Vec<_> = (0..row_count(previous))
        .filter_map(|index| row(previous, index))
        .collect();
    let new: Vec<_> = (0..count).filter_map(|index| row(next, index)).collect();
    let positions: HashMap<_, _> = old
        .iter()
        .enumerate()
        .map(|(index, row)| (row.key(), index))
        .collect();
    let keys: HashSet<_> = new.iter().map(|row| row.key()).collect();
    // Ambiguous identity must not preserve measurements or disclosure on the wrong row.
    if positions.len() != old.len() || keys.len() != new.len() {
        return Update::Reset(count);
    }

    let mut splices = Vec::new();
    let mut remeasure: Vec<Range<usize>> = Vec::new();
    let (mut old_start, mut new_start) = (0, 0);
    // Retain ordered stable IDs in linear time, including a clipped history prefix.
    // Reordered rows are removed/reinserted; they never inherit another row's height.
    for (new_index, row) in new.iter().enumerate() {
        let Some(&old_index) = positions
            .get(&row.key())
            .filter(|&&index| index >= old_start)
        else {
            continue;
        };
        splice(
            &mut splices,
            new_start,
            old_index - old_start,
            new_index - new_start,
        );
        if old[old_index] != *row {
            if let Some(range) = remeasure.last_mut().filter(|range| range.end == new_index) {
                range.end += 1;
            } else {
                remeasure.push(new_index..new_index + 1);
            }
        }
        old_start = old_index + 1;
        new_start = new_index + 1;
    }
    splice(
        &mut splices,
        new_start,
        old.len() - old_start,
        new.len() - new_start,
    );
    Update::Patch { splices, remeasure }
}

fn splice(splices: &mut Vec<Splice>, start: usize, removed: usize, inserted: usize) {
    if removed != 0 || inserted != 0 {
        splices.push(Splice {
            range: start..start + removed,
            count: inserted,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{MessageRole, ThreadStatus};

    fn snapshot(ids: &[usize]) -> Snapshot {
        let mut snapshot: Snapshot = serde_json::from_value(serde_json::json!({
            "id":"thread", "checkout":"/repo", "model":{"provider":"p", "model_id":"m"},
            "status":"running", "messages":[], "live_text":"", "tools":[],
            "pending_approval":null, "error":null, "truncated":false
        }))
        .unwrap();
        snapshot.messages = ids
            .iter()
            .map(|id| Message {
                id: id.to_string(),
                role: MessageRole::Assistant,
                text: format!("Message {id}"),
            })
            .collect();
        snapshot
    }

    fn assert_applies(old: &Snapshot, new: &Snapshot) {
        let mut result: Vec<_> = (0..row_count(old))
            .map(|index| row(old, index).unwrap())
            .collect();
        let expected: Vec<_> = (0..row_count(new))
            .map(|index| row(new, index).unwrap())
            .collect();
        match changes(Some(old), Some(new)) {
            Update::Reset(count) => assert_eq!(count, expected.len()),
            Update::Patch { splices, remeasure } => {
                for splice in splices {
                    let replacement = expected
                        [splice.range.start..splice.range.start + splice.count]
                        .iter()
                        .copied();
                    result.splice(splice.range, replacement);
                }
                for range in remeasure {
                    result[range.clone()].copy_from_slice(&expected[range]);
                }
                assert_eq!(result, expected);
            }
        }
    }

    #[test]
    fn streaming_changes_only_one_row_and_status_changes_none() {
        let mut old = snapshot(&(0..2000).collect::<Vec<_>>());
        old.live_text = "First".into();
        let mut new = old.clone();
        new.live_text.push_str(" word");
        assert_eq!(
            changes(Some(&old), Some(&new)),
            Update::Patch {
                splices: vec![],
                remeasure: std::iter::once(2000..2001).collect()
            }
        );
        new = old.clone();
        new.status = ThreadStatus::Stopped;
        assert_eq!(
            changes(Some(&old), Some(&new)),
            Update::Patch {
                splices: vec![],
                remeasure: vec![]
            }
        );
    }

    #[test]
    fn clipping_and_appending_keep_the_overlapping_history() {
        let old = snapshot(&[0, 1, 2]);
        let new = snapshot(&[1, 2, 3]);
        assert_eq!(
            changes(Some(&old), Some(&new)),
            Update::Patch {
                splices: vec![
                    Splice {
                        range: 0..1,
                        count: 0
                    },
                    Splice {
                        range: 2..2,
                        count: 1
                    }
                ],
                remeasure: vec![],
            }
        );
        assert_applies(&old, &new);
    }

    #[test]
    fn messages_stream_and_tools_have_distinct_stable_identities() {
        let mut old = snapshot(&[1]);
        old.live_text = "partial".into();
        old.tools.push(serde_json::from_value(serde_json::json!({"id":"1","name":"read","status":"running","input":"file","output":""})).unwrap());
        let mut new = old.clone();
        new.live_text.clear();
        new.messages.push(snapshot(&[2]).messages.remove(0));
        new.tools[0].output = "done".into();
        assert_applies(&old, &new);
        assert!(matches!(row(&old, 1), Some(Row::Streaming(_))));
        assert!(matches!(row(&old, 2), Some(Row::Tool(_))));
        assert_eq!(row(&old, 3), None);
        assert_eq!(row(&old, usize::MAX), None);
    }

    #[test]
    fn identity_changes_and_duplicate_ids_reset_measurements() {
        let old = snapshot(&[1, 2]);
        let mut new = old.clone();
        new.id = "other".into();
        assert_eq!(changes(Some(&old), Some(&new)), Update::Reset(2));
        assert_eq!(changes(None, Some(&old)), Update::Reset(2));
        assert_eq!(changes(Some(&old), None), Update::Reset(0));
        assert_eq!(
            changes(Some(&old), Some(&snapshot(&[1, 1]))),
            Update::Reset(2)
        );
        assert_eq!(
            changes(Some(&snapshot(&[1, 1])), Some(&old)),
            Update::Reset(2)
        );
    }

    #[test]
    fn arbitrary_insert_delete_reorder_and_content_changes_apply_correctly() {
        let mut seed = 7u64;
        let mut random = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 32) as usize
        };
        for _ in 0..2000 {
            let mut ids: Vec<_> = (0..20).collect();
            for index in (1..ids.len()).rev() {
                ids.swap(index, random() % (index + 1));
            }
            let old = snapshot(&ids[..random() % 21]);
            for index in (1..ids.len()).rev() {
                ids.swap(index, random() % (index + 1));
            }
            let mut new = snapshot(&ids[..random() % 21]);
            for message in &mut new.messages {
                if random() % 3 == 0 {
                    message.text.push_str(" changed");
                }
            }
            assert_applies(&old, &new);
        }
    }
}
