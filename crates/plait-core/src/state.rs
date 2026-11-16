use crate::{
    git, machine,
    model::{Diff, DiffMode, Machine, Project, Snapshot},
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
    /// The checkout's file list, for naming files in a prompt.
    Files {
        epoch: u64,
        root: PathBuf,
    },
}

pub enum Reply {
    Status(Result<Arc<Snapshot>, String>),
    Diff(Result<Arc<Diff>, String>),
    Files(Result<Arc<git::FileList>, String>),
}

impl Job {
    pub fn run(&self) -> Reply {
        match self {
            Self::Status { path, .. } => Reply::Status(git::status(path).map(Arc::new)),
            Self::Diff { key, snapshot, .. } => Reply::Diff(
                git::diff(&snapshot.root, &snapshot.changes[key.file], key.mode).map(Arc::new),
            ),
            Self::Files { root, .. } => Reply::Files(git::files(root).map(Arc::new)),
        }
    }
}

pub struct RepositoryState {
    pub path: PathBuf,
    pub snapshot: Load<Arc<Snapshot>>,
    pub diff: Load<Arc<Diff>>,
    pub selected: Option<DiffKey>,
    /// Loaded on request and kept until the next refresh or checkout change.
    pub files: Load<Arc<git::FileList>>,
    epoch: u64,
    running: bool,
    pending: Option<Job>,
    // Separate from `pending`, so a file list and a diff cannot displace each other.
    files_queued: bool,
    cache: HashMap<DiffKey, Arc<Diff>>,
}

impl RepositoryState {
    pub fn new(path: PathBuf) -> Self {
        let mut state = Self {
            path,
            snapshot: Load::Empty,
            diff: Load::Empty,
            selected: None,
            files: Load::Empty,
            epoch: 0,
            running: false,
            pending: None,
            files_queued: false,
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
        self.files = Load::Empty;
        self.files_queued = false;
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

    /// Changes on every refresh and checkout change; guards cached Git data.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Ask for the checkout's file list. A cached or already queued list is kept.
    pub fn request_files(&mut self) {
        if matches!(self.files, Load::Loading | Load::Ready(_)) {
            return;
        }
        if let Load::Error(error) = &self.snapshot {
            self.files = Load::Error(error.clone());
            return;
        }
        self.files = Load::Loading;
        self.files_queued = true;
    }

    /// Status and diffs go first. The list needs the root a status read reports.
    fn files_job(&mut self) -> Option<Job> {
        if !self.files_queued {
            return None;
        }
        let root = self.snapshot.ready()?.root.clone();
        self.files_queued = false;
        Some(Job::Files {
            epoch: self.epoch,
            root,
        })
    }

    pub fn next_job(&mut self) -> Option<Job> {
        if self.running {
            return None;
        }
        let job = self.pending.take().or_else(|| self.files_job())?;
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
                    Err(error) => {
                        // No status means no root to list files under.
                        if std::mem::take(&mut self.files_queued) {
                            self.files = Load::Error(error.clone());
                        }
                        self.snapshot = Load::Error(error);
                    }
                }
            }
            (Job::Files { epoch, .. }, Reply::Files(result)) if epoch == self.epoch => {
                self.files = match result {
                    Ok(files) => Load::Ready(files),
                    Err(error) => Load::Error(error),
                };
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
    pub collapsed_projects: HashSet<usize>,
    pub collapsed_machines: HashSet<(usize, usize)>,
    pub repository: RepositoryState,
}

/// The project's name is the checkout's folder name, or the whole path for `/`.
pub fn project_name(path: &std::path::Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

impl AppState {
    /// One project, the opened checkout, on this machine. Remote machines arrive with M3.
    pub fn new(path: PathBuf) -> Self {
        Self {
            projects: vec![Project {
                checkout: path.clone(),
                name: project_name(&path),
                machines: vec![Machine {
                    name: machine::local_name(),
                    local: true,
                }],
            }],
            collapsed_projects: HashSet::new(),
            collapsed_machines: HashSet::new(),
            repository: RepositoryState::new(path),
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

    fn file_list(path: &str) -> Arc<git::FileList> {
        Arc::new(git::FileList {
            paths: vec![path.into()],
            ..git::FileList::default()
        })
    }

    #[test]
    fn file_list_waits_for_status_and_is_cached_until_refresh() {
        let mut s = RepositoryState::new("repo".into());
        s.request_files();
        assert!(matches!(s.files, Load::Loading));
        let status = s.next_job().unwrap();
        assert!(matches!(status, Job::Status { .. }));
        s.complete(status, Reply::Status(Ok(snapshot())));
        // The first changed file's diff was queued by the status result.
        let diff = s.next_job().unwrap();
        assert!(matches!(diff, Job::Diff { .. }));
        assert!(s.next_job().is_none());
        s.complete(
            diff,
            Reply::Diff(Ok(Arc::new(Diff::message("diff", Duration::ZERO)))),
        );
        let files = s.next_job().unwrap();
        assert!(matches!(&files, Job::Files { root, .. } if root == std::path::Path::new("/repo")));
        s.complete(files, Reply::Files(Ok(file_list("a"))));
        assert_eq!(s.files.ready().unwrap().paths, ["a"]);
        assert!(matches!(s.diff, Load::Ready(_)));
        s.request_files();
        assert!(s.next_job().is_none());

        let epoch = s.epoch();
        s.refresh();
        assert_ne!(s.epoch(), epoch);
        assert!(matches!(s.files, Load::Empty));
    }

    #[test]
    fn stale_file_list_cannot_enter_a_new_epoch() {
        let mut s = RepositoryState::new("old".into());
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Ok(snapshot())));
        let diff = s.next_job().unwrap();
        s.complete(
            diff,
            Reply::Diff(Ok(Arc::new(Diff::message("diff", Duration::ZERO)))),
        );
        s.request_files();
        let old = s.next_job().unwrap();
        s.open("new".into());
        s.request_files();
        s.complete(old, Reply::Files(Ok(file_list("old"))));
        assert!(matches!(s.files, Load::Loading));
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Ok(snapshot())));
        let diff = s.next_job().unwrap();
        s.complete(diff, Reply::Diff(Err("gone".into())));
        let current = s.next_job().unwrap();
        s.complete(current, Reply::Files(Ok(file_list("new"))));
        assert_eq!(s.files.ready().unwrap().paths, ["new"]);
    }

    #[test]
    fn file_list_reports_a_failed_status_or_listing() {
        let mut s = RepositoryState::new("repo".into());
        s.request_files();
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Err("not a repository".into())));
        assert!(matches!(&s.files, Load::Error(error) if error == "not a repository"));
        assert!(s.next_job().is_none());
        // A later request reports the same state without queueing work.
        s.files = Load::Empty;
        s.request_files();
        assert!(matches!(s.files, Load::Error(_)));
        assert!(s.next_job().is_none());

        let mut s = RepositoryState::new("repo".into());
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Ok(snapshot())));
        let diff = s.next_job().unwrap();
        s.complete(diff, Reply::Diff(Err("gone".into())));
        s.request_files();
        let files = s.next_job().unwrap();
        s.complete(files, Reply::Files(Err("too large".into())));
        assert!(matches!(&s.files, Load::Error(error) if error == "too large"));
        // An error is not cached: asking again retries.
        s.request_files();
        assert!(matches!(s.next_job(), Some(Job::Files { .. })));
    }

    #[test]
    fn the_checkout_is_the_only_project_on_this_machine() {
        let s = AppState::new("/tmp/plait-fixture".into());
        assert_eq!(s.projects.len(), 1);
        assert_eq!(s.projects[0].name, "plait-fixture");
        assert_eq!(s.projects[0].machines.len(), 1);
        assert!(s.projects[0].machines[0].local);
        assert_eq!(project_name(std::path::Path::new("/")), "/");
    }
}
