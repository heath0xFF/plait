//! Local project navigation and activity tracking. No filesystem access or UI types.

use crate::{
    host::{ThreadStatus, ThreadSummary},
    model::{Machine, Project},
    preferences::{Jump, JumpTarget},
    state::project_name,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};

/// Presentation state only. Thread content and execution still belong to the host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub opened: Vec<PathBuf>,
    pub hidden: Vec<PathBuf>,
    pub observed: BTreeMap<String, Activity>,
    pub unread: BTreeSet<String>,
    pub mute_notifications: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
    pub updated_at: u64,
    pub state: String,
}

impl Activity {
    fn from_thread(thread: &ThreadSummary) -> Self {
        Self {
            updated_at: thread.updated_at.unwrap_or(0),
            state: state_label(thread).into(),
        }
    }
}

impl Preferences {
    pub fn open(&mut self, checkout: &Path) {
        self.hidden.retain(|path| path != checkout);
        if !self.opened.iter().any(|path| path == checkout) {
            self.opened.push(checkout.into());
        }
        self.normalize();
    }

    pub fn hide(&mut self, checkout: &Path) {
        self.opened.retain(|path| path != checkout);
        if !self.hidden.iter().any(|path| path == checkout) {
            self.hidden.push(checkout.into());
        }
        self.normalize();
    }

    pub fn normalize(&mut self) {
        for paths in [&mut self.opened, &mut self.hidden] {
            let mut seen = HashSet::new();
            paths.retain(|path| {
                path.is_absolute() && path.as_os_str().len() <= 4096 && seen.insert(path.clone())
            });
            if paths.len() > 32 {
                paths.drain(..paths.len() - 32);
            }
        }
        self.observed
            .retain(|id, activity| id.len() <= 256 && activity.state.len() <= 32);
        if self.observed.len() > 1000 {
            let mut newest = self
                .observed
                .iter()
                .map(|(id, activity)| (activity.updated_at, id.clone()))
                .collect::<Vec<_>>();
            newest.sort();
            for (_, id) in newest.into_iter().take(self.observed.len() - 1000) {
                self.observed.remove(&id);
            }
        }
        self.unread.retain(|id| self.observed.contains_key(id));
    }

    /// Mark terminal/approval changes, but never notify from an initial listing.
    /// Persisted observations still make changes since the last launch unread.
    pub fn observe(
        &mut self,
        threads: &[ThreadSummary],
        visible: Option<&str>,
        initial: bool,
    ) -> Vec<String> {
        let mut notify = Vec::new();
        for thread in threads {
            let next = Activity::from_thread(thread);
            let previous = self.observed.insert(thread.id.clone(), next.clone());
            let changed = previous.as_ref().is_some_and(|old| old != &next);
            let attention = thread.pending_approval == Some(true)
                || matches!(
                    thread.status,
                    ThreadStatus::AwaitingApproval
                        | ThreadStatus::Error
                        | ThreadStatus::Interrupted
                        | ThreadStatus::Stopped
                )
                || (thread.status == ThreadStatus::Idle
                    && previous.as_ref().is_some_and(|old| {
                        matches!(old.state.as_str(), "Working" | "Needs approval")
                    }));
            if visible == Some(thread.id.as_str()) {
                self.unread.remove(&thread.id);
            } else if changed && attention {
                self.unread.insert(thread.id.clone());
                if !initial && !self.mute_notifications && thread.archived != Some(true) {
                    notify.push(thread.id.clone());
                }
            }
        }
        self.normalize();
        notify
    }
}

/// Overlay a bounded live listing on older loaded history. The protocol has
/// no delete operation, so leaving its newest-200 window is not deletion.
/// Validate the whole frame before mutating and ignore older pagination races.
pub fn merge_threads(
    threads: &mut Vec<ThreadSummary>,
    incoming: Vec<ThreadSummary>,
) -> Result<(), String> {
    let mut seen = HashSet::new();
    for next in &incoming {
        if next.id.is_empty() || !next.checkout.is_absolute() || !seen.insert(&next.id) {
            return Err(
                "Host returned invalid or duplicate thread identities. Reconnect to reload.".into(),
            );
        }
        if threads
            .iter()
            .any(|old| old.id == next.id && old.checkout != next.checkout)
        {
            return Err(
                "Host changed a thread's checkout. Reconnect to reload its identity.".into(),
            );
        }
    }
    for next in incoming {
        if let Some(old) = threads.iter_mut().find(|old| old.id == next.id) {
            if next.updated_at >= old.updated_at {
                *old = next;
            }
        } else {
            threads.push(next);
        }
    }
    threads.sort_by_key(|thread| std::cmp::Reverse(thread.updated_at));
    Ok(())
}

pub fn state_label(thread: &ThreadSummary) -> &'static str {
    if thread.pending_approval == Some(true) {
        return "Needs approval";
    }
    match thread.status {
        ThreadStatus::Running => "Working",
        ThreadStatus::Idle if thread.updated_at > thread.created_at => "Done",
        other => other.label(),
    }
}

pub fn needs_attention(thread: &ThreadSummary, unread: &BTreeSet<String>) -> bool {
    unread.contains(&thread.id)
        || thread.pending_approval == Some(true)
        || matches!(
            thread.status,
            ThreadStatus::AwaitingApproval | ThreadStatus::Error | ThreadStatus::Interrupted
        )
}

/// Host list order is authoritative within a project; projects follow newest activity.
pub fn projects(
    threads: &[ThreadSummary],
    preferences: &Preferences,
    current: &Path,
    machine: &str,
) -> Vec<Project> {
    let mut activity = HashMap::new();
    for thread in threads {
        let updated = activity.entry(thread.checkout.as_path()).or_insert(0);
        *updated = (*updated).max(thread.updated_at.unwrap_or(0));
    }
    let hidden = preferences
        .hidden
        .iter()
        .map(PathBuf::as_path)
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut paths = threads
        .iter()
        .map(|thread| thread.checkout.as_path())
        .chain(preferences.opened.iter().map(PathBuf::as_path))
        .chain(std::iter::once(current))
        .filter(|path| (!hidden.contains(path) || *path == current) && seen.insert(*path))
        .map(Path::to_owned)
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| std::cmp::Reverse(activity.get(path.as_path()).copied().unwrap_or(0)));
    paths
        .into_iter()
        .map(|checkout| Project {
            name: project_name(&checkout),
            checkout,
            machines: vec![Machine {
                name: machine.into(),
                local: true,
            }],
        })
        .collect()
}

/// A project match includes all its threads; a thread match retains its project.
pub fn matches(
    thread: &ThreadSummary,
    title: &str,
    project: &Project,
    query: &str,
    archived: bool,
    attention: bool,
    unread: &BTreeSet<String>,
) -> bool {
    if thread.checkout != project.checkout
        || (!archived && thread.archived == Some(true))
        || (attention && !needs_attention(thread, unread))
    {
        return false;
    }
    let haystack =
        format!("{} {} {}", project.name, project.checkout.display(), title).to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

pub fn jump(
    projects: &[Project],
    threads: &[ThreadSummary],
    indices: &[usize],
    target: JumpTarget,
    number: usize,
) -> Option<Jump> {
    let number = number.checked_sub(1)?;
    let grouped = projects
        .iter()
        .enumerate()
        .map(|(project, p)| {
            indices
                .iter()
                .copied()
                .filter(move |&index| threads[index].checkout == p.checkout)
                .map(move |thread| Jump {
                    project,
                    machine: 0,
                    thread,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    match target {
        JumpTarget::Threads => grouped.into_iter().flatten().nth(number),
        JumpTarget::Projects | JumpTarget::Machines => grouped.get(number)?.first().copied(),
    }
}

pub fn age(updated_at: Option<u64>, now: u64) -> String {
    let Some(updated_at) = updated_at else {
        return String::new();
    };
    let minutes = now.saturating_sub(updated_at) / 60_000;
    match minutes {
        0 => "now".into(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn thread(id: &str, checkout: &str, status: ThreadStatus, updated: u64) -> ThreadSummary {
        ThreadSummary {
            id: id.into(),
            checkout: checkout.into(),
            status,
            created_at: Some(1),
            updated_at: Some(updated),
            ..Default::default()
        }
    }
    #[test]
    fn live_overlay_adds_reorders_and_preserves_older_rows_without_regression() {
        let mut threads = vec![
            thread("a", "/one", ThreadStatus::Running, 20),
            thread("b", "/two", ThreadStatus::Idle, 10),
        ];
        merge_threads(
            &mut threads,
            vec![
                thread("c", "/three", ThreadStatus::Error, 40),
                thread("a", "/one", ThreadStatus::Idle, 30),
            ],
        )
        .unwrap();
        assert_eq!(
            threads.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["c", "a", "b"]
        );
        merge_threads(
            &mut threads,
            vec![thread("a", "/one", ThreadStatus::Running, 20)],
        )
        .unwrap();
        assert_eq!(threads[1].status, ThreadStatus::Idle);
        let before = threads.clone();
        assert!(
            merge_threads(
                &mut threads,
                vec![thread("a", "/wrong", ThreadStatus::Error, 50)]
            )
            .is_err()
        );
        assert_eq!(threads, before);
        assert!(
            merge_threads(
                &mut threads,
                vec![
                    thread("new", "/r", ThreadStatus::Idle, 50),
                    thread("new", "/r", ThreadStatus::Idle, 50)
                ]
            )
            .is_err()
        );
        assert_eq!(threads, before);
    }

    #[test]
    fn activity_marks_changes_once_and_survives_restart() {
        let mut preferences = Preferences::default();
        let mut t = thread("a", "/repo", ThreadStatus::Running, 1);
        assert!(preferences.observe(&[t.clone()], None, true).is_empty());
        t.status = ThreadStatus::Idle;
        t.updated_at = Some(2);
        assert_eq!(preferences.observe(&[t.clone()], None, false), ["a"]);
        assert!(preferences.unread.contains("a"));
        let saved = serde_json::to_string(&preferences).unwrap();
        let mut restored: Preferences = serde_json::from_str(&saved).unwrap();
        assert!(restored.observe(&[t.clone()], None, true).is_empty());
        assert!(restored.unread.contains("a"));
        restored.observe(&[t], Some("a"), false);
        assert!(restored.unread.is_empty());
    }
    #[test]
    fn visible_and_muted_changes_do_not_notify() {
        let mut p = Preferences::default();
        let mut t = thread("a", "/r", ThreadStatus::Running, 1);
        p.observe(&[t.clone()], None, true);
        t.status = ThreadStatus::AwaitingApproval;
        t.updated_at = Some(2);
        assert!(p.observe(&[t.clone()], Some("a"), false).is_empty());
        assert!(p.unread.is_empty());
        p.mute_notifications = true;
        t.status = ThreadStatus::Error;
        t.updated_at = Some(3);
        assert!(p.observe(&[t], None, false).is_empty());
        assert!(p.unread.contains("a"));
    }
    #[test]
    fn projects_are_grouped_and_hidden_without_deleting_threads() {
        let threads = [
            thread("a", "/one", ThreadStatus::Idle, 1),
            thread("b", "/two", ThreadStatus::Running, 2),
        ];
        let mut p = Preferences::default();
        p.open(Path::new("/empty"));
        let groups = projects(&threads, &p, Path::new("/one"), "Local");
        assert_eq!(
            groups.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["two", "one", "empty"]
        );
        assert_eq!(
            jump(&groups, &threads, &[0, 1], JumpTarget::Threads, 1)
                .unwrap()
                .thread,
            1
        );
        assert_eq!(
            jump(&groups, &threads, &[0, 1], JumpTarget::Projects, 2)
                .unwrap()
                .thread,
            0
        );
        assert!(jump(&groups, &threads, &[0, 1], JumpTarget::Projects, 3).is_none());
        p.hide(Path::new("/two"));
        assert_eq!(projects(&threads, &p, Path::new("/one"), "Local").len(), 2);
        assert_eq!(threads.len(), 2);
    }
    #[test]
    fn filtering_respects_archive_attention_and_project_names() {
        let mut t = thread("a", "/project", ThreadStatus::Idle, 2);
        let p = projects(
            &[t.clone()],
            &Preferences::default(),
            Path::new("/project"),
            "Local",
        )
        .remove(0);
        assert!(matches(
            &t,
            "Repair tests",
            &p,
            "project repair",
            false,
            false,
            &BTreeSet::new()
        ));
        assert!(!matches(
            &t,
            "Repair tests",
            &p,
            "missing",
            false,
            false,
            &BTreeSet::new()
        ));
        t.archived = Some(true);
        assert!(!matches(
            &t,
            "Repair tests",
            &p,
            "",
            false,
            false,
            &BTreeSet::new()
        ));
        assert!(matches(
            &t,
            "Repair tests",
            &p,
            "",
            true,
            false,
            &BTreeSet::new()
        ));
        assert!(!matches(
            &t,
            "Repair tests",
            &p,
            "",
            true,
            true,
            &BTreeSet::new()
        ));
    }
    #[test]
    fn older_preferences_and_future_timestamps_are_safe() {
        assert_eq!(
            serde_json::from_str::<Preferences>("{}").unwrap(),
            Preferences::default()
        );
        assert_eq!(age(Some(120_000), 60_000), "now");
        assert_eq!(age(Some(0), 60_000 * 61), "1h");
        assert_eq!(age(None, 0), "");
    }
}
