use crate::{
    fixtures, git,
    model::{Diff, DiffMode, Project, Selection, Snapshot},
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

#[derive(Debug)]
pub enum Load<T> {
    Empty,
    Loading,
    Ready(T),
    Error(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        if let Self::Ready(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiffKey {
    pub file: usize,
    pub mode: DiffMode,
}

#[derive(Clone, Debug)]
pub enum Job {
    Status {
        epoch: u64,
        path: PathBuf,
    },
    Diff {
        epoch: u64,
        key: DiffKey,
        snapshot: Arc<Snapshot>,
    },
}

pub enum Reply {
    Status(Result<Arc<Snapshot>, String>),
    Diff(Result<Arc<Diff>, String>),
}

impl Job {
    pub fn run(&self) -> Reply {
        match self {
            Self::Status { path, .. } => Reply::Status(git::status(path).map(Arc::new)),
            Self::Diff { key, snapshot, .. } => Reply::Diff(
                git::diff(&snapshot.root, &snapshot.changes[key.file], key.mode).map(Arc::new),
            ),
        }
    }
}

pub struct RepositoryState {
    pub path: PathBuf,
    pub snapshot: Load<Arc<Snapshot>>,
    pub diff: Load<Arc<Diff>>,
    pub selected: Option<DiffKey>,
    epoch: u64,
    running: bool,
    pending: Option<Job>,
    cache: HashMap<DiffKey, Arc<Diff>>,
}

impl RepositoryState {
    pub fn new(path: PathBuf) -> Self {
        let mut state = Self {
            path,
            snapshot: Load::Empty,
            diff: Load::Empty,
            selected: None,
            epoch: 0,
            running: false,
            pending: None,
            cache: HashMap::new(),
        };
        state.refresh();
        state
    }

    pub fn open(&mut self, path: PathBuf) {
        self.path = path;
        self.refresh();
    }

    pub fn refresh(&mut self) {
        self.epoch += 1;
        self.snapshot = Load::Loading;
        self.diff = Load::Empty;
        self.selected = None;
        self.cache.clear();
        self.pending = Some(Job::Status {
            epoch: self.epoch,
            path: self.path.clone(),
        });
    }

    pub fn select(&mut self, file: usize, mode: DiffMode) {
        let Some(snapshot) = self.snapshot.ready() else {
            return;
        };
        if file >= snapshot.changes.len() {
            return;
        }
        let key = DiffKey { file, mode };
        self.selected = Some(key.clone());
        if let Some(diff) = self.cache.get(&key) {
            self.diff = Load::Ready(diff.clone());
            self.pending = None;
        } else {
            self.diff = Load::Loading;
            self.pending = Some(Job::Diff {
                epoch: self.epoch,
                key,
                snapshot: snapshot.clone(),
            });
        }
    }

    pub fn next_job(&mut self) -> Option<Job> {
        if self.running {
            return None;
        }
        let job = self.pending.take()?;
        self.running = true;
        Some(job)
    }

    pub fn complete(&mut self, job: Job, reply: Reply) {
        self.running = false;
        match (job, reply) {
            (Job::Status { epoch, .. }, Reply::Status(result)) if epoch == self.epoch => {
                match result {
                    Ok(snapshot) => {
                        self.path = snapshot.root.clone();
                        let mode = snapshot.changes.first().map(|c| {
                            if c.unstaged() {
                                DiffMode::Unstaged
                            } else {
                                DiffMode::Staged
                            }
                        });
                        self.snapshot = Load::Ready(snapshot);
                        if let Some(mode) = mode {
                            self.select(0, mode);
                        }
                    }
                    Err(error) => self.snapshot = Load::Error(error),
                }
            }
            (Job::Diff { epoch, key, .. }, Reply::Diff(result)) if epoch == self.epoch => {
                if let Ok(diff) = &result {
                    // Keep memory bounded without making cached switching depend on repository size.
                    if self.cache.len() >= 8 {
                        self.cache.clear();
                    }
                    self.cache.insert(key.clone(), diff.clone());
                }
                if self.selected.as_ref() == Some(&key) {
                    self.diff = match result {
                        Ok(diff) => Load::Ready(diff),
                        Err(error) => Load::Error(error),
                    };
                    self.pending = None;
                }
            }
            _ => {}
        }
    }
}

pub struct AppState {
    pub projects: Vec<Project>,
    pub selection: Selection,
    pub collapsed_projects: HashSet<usize>,
    pub collapsed_machines: HashSet<(usize, usize)>,
    pub repository: RepositoryState,
}

impl AppState {
    pub fn new(path: PathBuf) -> Self {
        Self {
            projects: fixtures::projects(),
            selection: Selection {
                project: 0,
                machine: 0,
                thread: 0,
            },
            collapsed_projects: HashSet::new(),
            collapsed_machines: HashSet::new(),
            repository: RepositoryState::new(path),
        }
    }

    pub fn select(&mut self, selection: Selection) {
        if self
            .projects
            .get(selection.project)
            .and_then(|p| p.machines.get(selection.machine))
            .and_then(|m| m.threads.get(selection.thread))
            .is_some()
        {
            self.selection = selection;
        }
    }

    pub fn local(&self) -> bool {
        self.projects[self.selection.project].machines[self.selection.machine].local
    }

    pub fn step_thread(&mut self, forward: bool) {
        let selections: Vec<_> = self
            .projects
            .iter()
            .enumerate()
            .flat_map(|(project, p)| {
                p.machines.iter().enumerate().flat_map(move |(machine, m)| {
                    (0..m.threads.len()).map(move |thread| Selection {
                        project,
                        machine,
                        thread,
                    })
                })
            })
            .collect();
        if let Some(index) = selections.iter().position(|s| s == &self.selection) {
            let next = if forward {
                (index + 1) % selections.len()
            } else {
                (index + selections.len() - 1) % selections.len()
            };
            self.selection = selections[next];
            self.collapsed_projects.remove(&self.selection.project);
            self.collapsed_machines
                .remove(&(self.selection.project, self.selection.machine));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Change;
    use std::time::Duration;

    fn snapshot() -> Arc<Snapshot> {
        Arc::new(Snapshot {
            root: "/repo".into(),
            branch: "main".into(),
            changes: vec![
                Change {
                    path: "a".into(),
                    original: None,
                    index: b'M',
                    worktree: b'M',
                },
                Change {
                    path: "b".into(),
                    original: None,
                    index: b'?',
                    worktree: b'?',
                },
            ],
            elapsed: Duration::ZERO,
        })
    }

    #[test]
    fn stale_repository_result_cannot_replace_new_selection() {
        let mut s = RepositoryState::new("old".into());
        let old = s.next_job().unwrap();
        s.open("new".into());
        assert!(s.next_job().is_none());
        s.complete(old, Reply::Status(Ok(snapshot())));
        assert_eq!(s.path, PathBuf::from("new"));
        assert!(matches!(s.snapshot, Load::Loading));
        assert!(
            matches!(s.next_job(), Some(Job::Status { path, .. }) if path == std::path::Path::new("new"))
        );
    }

    #[test]
    fn latest_diff_wins_and_cached_switch_has_no_job() {
        let mut s = RepositoryState::new("repo".into());
        let job = s.next_job().unwrap();
        s.complete(job, Reply::Status(Ok(snapshot())));
        let old = s.next_job().unwrap();
        s.select(1, DiffMode::Unstaged);
        s.select(0, DiffMode::Staged);
        assert!(s.next_job().is_none());
        s.complete(
            old,
            Reply::Diff(Ok(Arc::new(Diff::message("old", Duration::ZERO)))),
        );
        assert!(matches!(s.diff, Load::Loading));
        let current = s.next_job().unwrap();
        assert!(matches!(current, Job::Diff { ref key, .. } if key.mode == DiffMode::Staged));
        s.complete(
            current,
            Reply::Diff(Ok(Arc::new(Diff::message("new", Duration::ZERO)))),
        );
        s.select(0, DiffMode::Unstaged);
        assert_eq!(&*s.diff.ready().unwrap().lines[0].text, "old");
        assert!(s.next_job().is_none());
    }

    #[test]
    fn navigation_is_valid_and_reveals_collapsed_parents() {
        let mut s = AppState::new(".".into());
        s.collapsed_projects.insert(1);
        s.step_thread(false);
        assert_eq!(s.selection.project, 1);
        assert!(!s.local());
        assert!(!s.collapsed_projects.contains(&1));
        let before = s.selection;
        s.select(Selection {
            project: 50,
            machine: 0,
            thread: 0,
        });
        assert_eq!(s.selection, before);
    }
}
