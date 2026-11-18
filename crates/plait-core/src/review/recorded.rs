//! Historical Changes selection and reply identity. No filesystem or UI work.
use crate::host::{RecordChange, RecordKind, RecordState, TurnRecord};
use std::{collections::HashMap, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub thread: String,
    pub checkout: PathBuf,
    pub record: String,
    pub original: String,
    pub path: Option<PathBuf>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub thread: String,
    pub checkout: PathBuf,
    pub connection: u64,
    pub watch: u64,
    pub repository: u64,
    pub generation: u64,
    pub target: Option<Target>,
    pub current: bool,
}
#[derive(Clone, Debug)]
pub struct File {
    pub path: PathBuf,
    pub name: String,
    pub directory: String,
    pub counts: String,
    pub label: &'static str,
    pub detail: String,
}
#[derive(Debug)]
pub struct Record {
    pub id: String,
    pub original: String,
    pub label: String,
    pub detail: String,
    pub restored: bool,
    pub files: Vec<File>,
    pub indices: HashMap<PathBuf, usize>,
}
impl Record {
    /// The list is immutable and complete; neither status nor undo replaces it.
    pub fn prepare(records: &[TurnRecord], target: &Target) -> Result<Self, &'static str> {
        let source = records
            .iter()
            .find(|r| r.id == target.record)
            .ok_or("This turn's record is no longer retained.")?;
        if source.paths_offset != 0
            || source.paths.len() != source.paths_total
            || source.request_ids_offset != 0
            || source.request_ids.len() != source.request_ids_total
            || source.not_covered_offset != 0
            || source.not_covered.len() != source.not_covered_total
        {
            return Err("The record's file list is incomplete. Refresh to try again.");
        }
        match source.state {
            RecordState::Recording => return Err("The host is still recording this turn."),
            RecordState::Failed | RecordState::NoEffect => {
                return Err("This turn's after record could not be made.");
            }
            RecordState::Ready | RecordState::Undone => (),
        }
        let restored = source.kind == RecordKind::Undo;
        let files: Vec<_> = source
            .paths
            .iter()
            .map(|file| {
                let path = file.path.to_string_lossy();
                let counts = match (file.added, file.removed) {
                    (Some(a), Some(r)) => format!("+{a} −{r}"),
                    _ => String::new(),
                };
                let label = match file.change {
                    RecordChange::Added => "Added",
                    RecordChange::Deleted => "Deleted",
                    RecordChange::Mode => "Mode",
                    RecordChange::Modified => "Modified",
                };
                File {
                    path: file.path.clone(),
                    name: file
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    directory: file
                        .path
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new(""))
                        .to_string_lossy()
                        .into_owned(),
                    detail: format!(
                        "{path}, {label}, {counts}. {}",
                        if restored {
                            "Recorded by this undo."
                        } else {
                            "Recorded during this run."
                        }
                    ),
                    counts,
                    label,
                }
            })
            .collect();
        let indices = files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.path.clone(), i))
            .collect();
        let short = (4..=source.id.len())
            .find(|&n| {
                source.id.is_char_boundary(n)
                    && records.iter().all(|other| {
                        other.id == source.id || !other.id.starts_with(&source.id[..n])
                    })
            })
            .unwrap_or(source.id.len());
        let label = if restored {
            "Restored".into()
        } else {
            format!("Turn {}", &source.id[..short])
        };
        let detail = if restored {
            "Show the files this undo actually restored.".into()
        } else {
            format!(
                "Show files recorded during {}, in thread {}.",
                source.request_ids.first().unwrap_or(&source.id),
                target.thread
            )
        };
        Ok(Self {
            id: source.id.clone(),
            original: target.original.clone(),
            label,
            detail,
            restored,
            files,
            indices,
        })
    }
    /// Titles and relative time are prepared once when a record is loaded.
    pub fn describe(&mut self, records: &[TurnRecord], snapshot: Option<&crate::host::Snapshot>) {
        let Some(source) = records.iter().find(|r| r.id == self.id) else {
            return;
        };
        let request = source.request_ids.first().unwrap_or(&source.id);
        let request_id = format!("request-{request}");
        let title = snapshot
            .and_then(|s| s.messages.iter().find(|m| m.id == request_id))
            .map_or(request.as_str(), |m| m.text.as_str());
        let title = crate::history::records::preview_name(title);
        let thread = snapshot
            .and_then(|s| s.title.as_deref())
            .unwrap_or("this thread");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let age = crate::navigation::age(Some(source.created_at), now);
        let time = if age == "now" {
            "just now".into()
        } else {
            format!("{age} ago")
        };
        self.detail = if self.restored {
            "Show the files this undo actually restored.".into()
        } else {
            format!("Show files recorded during {title}, {time}, in {thread}.")
        };
        for omitted in source
            .not_covered
            .iter()
            .take(crate::history::records::PREVIEW_LIMIT)
        {
            self.detail.push_str(&format!(
                "\nNot covered: {} — {}",
                crate::history::records::preview_name(&omitted.path),
                crate::history::records::coverage_reason(omitted.reason)
            ));
        }
        if source.not_covered.len() > crate::history::records::PREVIEW_LIMIT {
            self.detail.push_str(&format!(
                "\n{} more not-covered paths",
                source.not_covered.len() - crate::history::records::PREVIEW_LIMIT
            ));
        }
    }
    /// Prepare current availability for row tooltips without scanning in render.
    pub fn current_layers(&mut self, snapshot: Option<&crate::model::Snapshot>) {
        let layers: HashMap<_, _> = snapshot
            .into_iter()
            .flat_map(|s| &s.changes)
            .map(|c| (&c.path, (c.staged(), c.unstaged())))
            .collect();
        for file in &mut self.files {
            file.detail.push_str(match layers.get(&file.path) {
                Some((true, true)) => " Current staged and unstaged changes.",
                Some((true, false)) => " Current staged changes.",
                Some((false, true)) => " Current unstaged changes.",
                _ => " No current changes.",
            });
        }
    }
    pub fn selected(&self, target: &Target) -> Option<usize> {
        target
            .path
            .as_ref()
            .and_then(|p| self.indices.get(p).copied())
            .or_else(|| (!self.files.is_empty()).then_some(0))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Target {
        Target {
            thread: "thread".into(),
            checkout: "/checkout".into(),
            record: "original".into(),
            original: "original".into(),
            path: None,
        }
    }
    #[test]
    fn every_read_identity_member_rejects_a_stale_reply() {
        let stamp = Stamp {
            thread: "t".into(),
            checkout: "/c".into(),
            connection: 1,
            watch: 2,
            repository: 3,
            generation: 4,
            target: Some(target()),
            current: false,
        };
        for change in 0..8 {
            let mut other = stamp.clone();
            match change {
                0 => other.thread.push('x'),
                1 => other.checkout.push("x"),
                2 => other.connection += 1,
                3 => other.watch += 1,
                4 => other.repository += 1,
                5 => other.generation += 1,
                6 => other.target.as_mut().unwrap().record.push('x'),
                _ => other.current = true,
            };
            assert_ne!(stamp, other);
        }
    }
    #[test]
    fn selection_is_independent_of_later_runs_and_undo() {
        let parse = |id: &str, state: &str, kind: &str, path: &str| {
            serde_json::from_value::<TurnRecord>(serde_json::json!({"id":id,"request_ids":[id],"request_ids_offset":0,"request_ids_total":1,"kind":kind,"undoes":null,"state":state,"head_moved":false,"after_restart":false,"paths":[{"path":path,"change":"deleted","added":0,"removed":4}],"paths_offset":0,"paths_total":1,"not_covered":[],"not_covered_offset":0,"not_covered_total":0,"created_at":1})).unwrap()
        };
        let records = vec![
            parse("undo", "ready", "undo", "restored"),
            parse("later", "ready", "turn", "later"),
            parse("original", "undone", "turn", "deleted"),
        ];
        let target = target();
        let saved = Record::prepare(&records, &target).unwrap();
        assert_eq!(saved.id, "original");
        assert_eq!(saved.files[0].path, PathBuf::from("deleted"));
        assert_eq!(saved.selected(&target), Some(0));
        let mut leaf = target.clone();
        leaf.record = "undo".into();
        let saved = Record::prepare(&records, &leaf).unwrap();
        assert!(saved.restored);
        assert_eq!(saved.original, "original");
    }
}
