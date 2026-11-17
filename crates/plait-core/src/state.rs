use crate::{
    git, machine,
    model::{Diff, DiffMode, Machine, Project, Snapshot},
    recent::Recent,
};
use std::{collections::HashSet, path::PathBuf, sync::Arc};

#[derive(Debug, Default)]
pub enum Load<T> {
    #[default]
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
    /// A write the user asked for. Not tied to an epoch: it runs even if the
    /// view refreshes meanwhile, and its result is reported against `root`.
    Write {
        root: PathBuf,
        op: git::WriteOp,
    },
    /// A page of commit history: `skip` commits back from `tip`, or from
    /// HEAD for the first page. Later pages count from the first page's
    /// newest commit, so a commit landing meanwhile cannot shift them.
    Log {
        epoch: u64,
        root: PathBuf,
        tip: Option<String>,
        skip: usize,
    },
    /// One commit's message and patch.
    Show {
        epoch: u64,
        root: PathBuf,
        id: String,
    },
}

pub enum Reply {
    Status(Result<Arc<Snapshot>, String>),
    Diff(Result<Arc<Diff>, String>),
    Files(Result<Arc<git::FileList>, String>),
    Write(Result<git::WriteOutcome, String>),
    Log(Result<git::History, String>),
    Show(Result<Arc<Diff>, String>),
}

/// Diffs kept for quick switching. A diff is at most 10,000 parsed lines, so
/// this bounds memory to a few tens of megabytes in the worst case.
const DIFF_CACHE: usize = 16;

/// Commits listed per history page.
pub const HISTORY_PAGE: usize = 100;

impl Job {
    pub fn run(&self) -> Reply {
        match self {
            Self::Status { path, .. } => Reply::Status(git::status(path).map(Arc::new)),
            Self::Diff { key, snapshot, .. } => Reply::Diff(
                git::diff(&snapshot.root, &snapshot.changes[key.file], key.mode).map(Arc::new),
            ),
            Self::Files { root, .. } => Reply::Files(git::files(root).map(Arc::new)),
            Self::Write { root, op } => Reply::Write(git::write(root, op)),
            Self::Log {
                root, tip, skip, ..
            } => Reply::Log(git::log(root, tip.as_deref(), *skip, HISTORY_PAGE)),
            Self::Show { root, id, .. } => Reply::Show(git::show(root, id).map(Arc::new)),
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
    /// Recently read diffs, so moving between files does not rerun Git.
    cache: Recent<DiffKey, Arc<Diff>>,
    /// The next file's diff, read ahead while nothing else waits.
    prefetch: Option<Job>,
    /// Set while stepping through files, with the direction. A click is
    /// random access, so it reads nothing ahead that it might wait behind.
    ahead: Option<bool>,
    /// Staging and committing; see `request_write`.
    pub write: WriteState,
    /// Count behind `staged()`; set when a status arrives.
    staged: usize,
    /// Commit history, loaded when the History view asks for it.
    pub commits: CommitLog,
    /// Count behind `conflicts()`; set when a status arrives.
    conflicts: usize,
    /// The repository root from the last status read. Kept while a refresh
    /// runs, so writes and history need not wait for it; cleared on `open`.
    root: Option<PathBuf>,
}

/// The one write that may be queued or running, and how the last one went.
#[derive(Default)]
pub struct WriteState {
    queued: Option<git::WriteOp>,
    running: bool,
    /// The last result in words, and whether it failed. Cleared by the next write.
    pub note: Option<(String, bool)>,
    /// The file to select again once the status after a write arrives.
    reselect: Option<(PathBuf, DiffMode)>,
    /// Set when a commit succeeds, so the view can clear its message once.
    pub committed: Option<String>,
    /// The last discard's saved copy, offered as Undo until the next write.
    pub undo: Option<git::Backup>,
}

impl WriteState {
    /// A write is waiting or running, so controls that start one are disabled.
    pub fn busy(&self) -> bool {
        self.queued.is_some() || self.running
    }
}

/// History pages and the selected commit's patch.
#[derive(Default)]
pub struct CommitLog {
    pub list: Load<git::History>,
    /// A further page was asked for and has not arrived.
    pub loading_more: bool,
    pub selected: Option<String>,
    pub patch: Load<Arc<Diff>>,
    /// The History view is showing; a commit reloads the list only then.
    wanted: bool,
    /// Guards history replies. Separate from status, so staging keeps history.
    epoch: u64,
    queued_page: Option<usize>,
    queued_show: Option<String>,
    /// Patches by commit ID. A commit never changes, so these survive reloads.
    shows: Recent<String, Arc<Diff>>,
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
            cache: Recent::new(DIFF_CACHE),
            prefetch: None,
            ahead: None,
            write: WriteState::default(),
            staged: 0,
            commits: CommitLog::default(),
            conflicts: 0,
            root: None,
        };
        state.refresh();
        state
    }

    /// Switch checkouts. Nothing from the previous one carries over.
    pub fn open(&mut self, path: PathBuf) {
        self.path = path;
        self.root = None;
        let wanted = self.commits.wanted;
        self.commits = CommitLog {
            epoch: self.commits.epoch + 1,
            wanted,
            ..CommitLog::default()
        };
        self.refresh();
    }

    /// Reread everything: status, diffs, the file list, and shown history.
    pub fn refresh(&mut self) {
        self.refresh_status();
        self.reset_history();
    }

    /// Reread status and diffs only. Staging changes the index, not history,
    /// so it keeps the commit list and its open patch.
    fn refresh_status(&mut self) {
        self.epoch += 1;
        self.snapshot = Load::Loading;
        self.staged = 0;
        self.conflicts = 0;
        self.diff = Load::Empty;
        self.selected = None;
        self.files = Load::Empty;
        self.files_queued = false;
        self.cache.clear();
        self.prefetch = None;
        self.pending = Some(Job::Status {
            epoch: self.epoch,
            path: self.path.clone(),
        });
    }

    /// Reread the commit list if the History view is showing, or drop it to
    /// load on the next visit. The selected commit and cached patches stay:
    /// commits are immutable, so only the list can be out of date.
    fn reset_history(&mut self) {
        let commits = &mut self.commits;
        commits.epoch += 1;
        commits.list = Load::Empty;
        commits.loading_more = false;
        commits.queued_page = None;
        // A patch still loading was guarded by the old epoch; ask again.
        if let (Some(id), Load::Loading) = (&commits.selected, &commits.patch) {
            commits.queued_show = Some(id.clone());
        }
        if commits.wanted {
            self.request_history();
        }
    }

    /// Changed files with something staged, counted once per status read.
    pub fn staged(&self) -> usize {
        self.staged
    }

    /// Changed files Git reports as unmerged, counted once per status read.
    pub fn conflicts(&self) -> usize {
        self.conflicts
    }

    /// Show a file's diff, as a click does.
    pub fn select(&mut self, file: usize, mode: DiffMode) {
        self.ahead = None;
        self.show(file, mode);
    }

    /// Show a file's diff while stepping through the list, and read the one
    /// after it (in the same direction) ahead of time.
    pub fn step(&mut self, file: usize, mode: DiffMode, forward: bool) {
        self.ahead = Some(forward);
        self.show(file, mode);
    }

    fn show(&mut self, file: usize, mode: DiffMode) {
        let Some(snapshot) = self.snapshot.ready() else {
            return;
        };
        if file >= snapshot.changes.len() {
            return;
        }
        let key = DiffKey { file, mode };
        self.selected = Some(key.clone());
        if let Some(diff) = self.cache.get(&key).cloned() {
            self.diff = Load::Ready(diff);
            self.pending = None;
            self.queue_prefetch(&key);
        } else {
            self.diff = Load::Loading;
            self.pending = Some(Job::Diff {
                epoch: self.epoch,
                key,
                snapshot: snapshot.clone(),
            });
        }
    }

    /// While stepping, read the next file in that direction ahead of time, in
    /// the view it would open in, so the next step shows a cached diff. One
    /// file only: one background Git process per step.
    fn queue_prefetch(&mut self, from: &DiffKey) {
        self.prefetch = None;
        let (Some(forward), Some(snapshot)) = (self.ahead, self.snapshot.ready()) else {
            return;
        };
        let count = snapshot.changes.len();
        // Stepping wraps around the list, so read-ahead does too.
        let file = if forward {
            (from.file + 1) % count.max(1)
        } else {
            (from.file + count - 1) % count.max(1)
        };
        if file == from.file {
            return;
        }
        let Some(change) = snapshot.changes.get(file) else {
            return;
        };
        let mode = if change.unstaged() {
            DiffMode::Unstaged
        } else {
            DiffMode::Staged
        };
        let key = DiffKey { file, mode };
        if !self.cache.contains(&key) {
            self.prefetch = Some(Job::Diff {
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

    /// Queue a write. Refused while another is queued or running, or before a
    /// status has found the repository root. Returns whether it was queued.
    pub fn request_write(&mut self, op: git::WriteOp) -> bool {
        if self.write.busy() || self.root.is_none() {
            return false;
        }
        // Keep the file in view across the refresh the write causes.
        self.write.reselect = self.selected_change();
        self.write.note = None;
        self.write.undo = None;
        self.write.queued = Some(op);
        true
    }

    /// The selected file's path and view, if one is selected.
    fn selected_change(&self) -> Option<(PathBuf, DiffMode)> {
        let key = self.selected.as_ref()?;
        let change = self.snapshot.ready()?.changes.get(key.file)?;
        Some((change.path.clone(), key.mode))
    }

    fn write_job(&mut self) -> Option<Job> {
        // `root` survives a refresh, so a write never waits on status. Only
        // `open` clears it; a write queued for the old checkout is then
        // refused with a note rather than run against the new one.
        let root = self.root.clone();
        let op = self.write.queued.take()?;
        let Some(root) = root else {
            self.write.note = Some((
                "Plait lost track of the repository; try again.".into(),
                true,
            ));
            return None;
        };
        self.write.running = true;
        Some(Job::Write { root, op })
    }

    /// The History view closed: a commit no longer needs to reload the list.
    pub fn leave_history(&mut self) {
        self.commits.wanted = false;
    }

    /// Start loading history, once, for the History view.
    pub fn request_history(&mut self) {
        self.commits.wanted = true;
        if matches!(self.commits.list, Load::Empty | Load::Error(_)) {
            self.commits.list = Load::Loading;
            self.commits.queued_page = Some(0);
        }
    }

    /// Load the page after the commits already listed.
    pub fn request_more_history(&mut self) {
        let Some(history) = self.commits.list.ready() else {
            return;
        };
        if history.more && !self.commits.loading_more {
            self.commits.loading_more = true;
            self.commits.queued_page = Some(history.commits.len());
        }
    }

    /// Show a commit's patch, from the cache when it was seen recently.
    pub fn select_commit(&mut self, id: String) {
        self.commits.selected = Some(id.clone());
        if let Some(patch) = self.commits.shows.get(&id).cloned() {
            self.commits.patch = Load::Ready(patch);
            self.commits.queued_show = None;
        } else {
            self.commits.patch = Load::Loading;
            self.commits.queued_show = Some(id);
        }
    }

    /// History needs the root that a status reports.
    fn history_job(&mut self) -> Option<Job> {
        let root = self.root.clone()?;
        let epoch = self.commits.epoch;
        if let Some(skip) = self.commits.queued_page.take() {
            let tip = match &self.commits.list {
                Load::Ready(history) if skip > 0 => history.commits.first().map(|c| c.id.clone()),
                _ => None,
            };
            return Some(Job::Log {
                epoch,
                root,
                tip,
                skip,
            });
        }
        let id = self.commits.queued_show.take()?;
        Some(Job::Show { epoch, root, id })
    }

    /// One job at a time. A write goes first so a click is never starved by
    /// reads; then status and diffs, then history, then the file list.
    pub fn next_job(&mut self) -> Option<Job> {
        if self.running {
            return None;
        }
        let job = self
            .write_job()
            .or_else(|| self.pending.take())
            .or_else(|| self.history_job())
            .or_else(|| self.files_job())
            .or_else(|| self.prefetch.take())?;
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
                        let again = self.write.reselect.take().and_then(|(path, mode)| {
                            let file = snapshot.changes.iter().position(|c| c.path == path)?;
                            let change = &snapshot.changes[file];
                            // After staging, the file may only have a staged side left.
                            let mode = match mode {
                                DiffMode::Unstaged if !change.unstaged() => DiffMode::Staged,
                                DiffMode::Staged if !change.staged() => DiffMode::Unstaged,
                                mode => mode,
                            };
                            Some((file, mode))
                        });
                        self.staged = snapshot.changes.iter().filter(|c| c.staged()).count();
                        self.conflicts = snapshot.changes.iter().filter(|c| c.conflicted()).count();
                        self.root = Some(snapshot.root.clone());
                        self.snapshot = Load::Ready(snapshot);
                        if let Some((file, mode)) = again {
                            self.select(file, mode);
                        } else if let Some(mode) = mode {
                            self.select(0, mode);
                        }
                    }
                    Err(error) => {
                        // No status means no root to list files under.
                        if std::mem::take(&mut self.files_queued) {
                            self.files = Load::Error(error.clone());
                        }
                        // Without a root, history waiting on one can never load.
                        if self.root.is_none() {
                            self.fail_history(&error);
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
                    self.cache.insert(key.clone(), diff.clone());
                }
                if self.selected.as_ref() == Some(&key) {
                    let ok = result.is_ok();
                    self.diff = match result {
                        Ok(diff) => Load::Ready(diff),
                        Err(error) => Load::Error(error),
                    };
                    self.pending = None;
                    if ok {
                        self.queue_prefetch(&key);
                    }
                }
            }
            (Job::Write { root, .. }, Reply::Write(result)) => {
                self.write.running = false;
                if let Ok(outcome) = &result {
                    self.write.committed = outcome.commit.clone();
                    self.write.undo = outcome.backup.clone();
                }
                self.write.note = Some(match &result {
                    Ok(outcome) => (outcome.summary.clone(), false),
                    Err(error) => (error.clone(), true),
                });
                // The repository changed on disk whatever the view shows now.
                // Only a commit changes history; staging rereads status alone.
                if self.root.as_ref().is_none_or(|known| *known == root) {
                    let reselect = self.write.reselect.take();
                    self.refresh_status();
                    if self.write.committed.is_some() {
                        self.reset_history();
                    }
                    self.write.reselect = reselect;
                }
            }
            (Job::Log { epoch, skip, .. }, Reply::Log(result)) if epoch == self.commits.epoch => {
                self.commits.loading_more = false;
                match (result, &mut self.commits.list) {
                    (Ok(page), Load::Ready(history)) if skip > 0 => {
                        history.commits.extend(page.commits);
                        history.more = page.more;
                    }
                    (Ok(page), list) => *list = Load::Ready(page),
                    // A failed later page keeps what is listed.
                    (Err(_), Load::Ready(_)) if skip > 0 => {}
                    (Err(error), list) => *list = Load::Error(error),
                }
            }
            (Job::Show { epoch, id, .. }, Reply::Show(result)) if epoch == self.commits.epoch => {
                if let Ok(patch) = &result {
                    self.commits.shows.insert(id.clone(), patch.clone());
                }
                if self.commits.selected.as_ref() == Some(&id) {
                    self.commits.patch = match result {
                        Ok(patch) => Load::Ready(patch),
                        Err(error) => Load::Error(error),
                    };
                }
            }
            (Job::Write { .. }, _) => self.write.running = false,
            _ => {}
        }
    }
}

impl RepositoryState {
    fn fail_history(&mut self, error: &str) {
        let commits = &mut self.commits;
        if commits.queued_page.take().is_some() {
            commits.list = Load::Error(error.to_owned());
        }
        if commits.queued_show.take().is_some() {
            commits.patch = Load::Error(error.to_owned());
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

    trait CloneForTest {
        fn clone_for_test(&self) -> Snapshot;
    }

    impl CloneForTest for Snapshot {
        fn clone_for_test(&self) -> Snapshot {
            Snapshot {
                root: self.root.clone(),
                branch: self.branch.clone(),
                named: self.named,
                tracking: self.tracking.clone(),
                changes: self.changes.clone(),
                elapsed: self.elapsed,
            }
        }
    }

    fn snapshot() -> Arc<Snapshot> {
        Arc::new(Snapshot {
            root: "/repo".into(),
            branch: "main".into(),
            named: true,
            tracking: None,
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
        // A click reads nothing ahead.
        assert!(s.next_job().is_none());
    }

    /// Run the queued read-ahead and return which file it read.
    fn prefetched(s: &mut RepositoryState) -> usize {
        let job = s.next_job().expect("a read-ahead is queued");
        let Job::Diff { key, .. } = &job else {
            panic!("expected a diff read-ahead");
        };
        let file = key.file;
        s.complete(
            job,
            Reply::Diff(Ok(Arc::new(Diff::message("ahead", Duration::ZERO)))),
        );
        file
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

    fn ready(path: &str) -> RepositoryState {
        let mut s = RepositoryState::new(path.into());
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Ok(snapshot())));
        let diff = s.next_job().unwrap();
        s.complete(
            diff,
            Reply::Diff(Ok(Arc::new(Diff::message("diff", Duration::ZERO)))),
        );
        s
    }

    fn outcome(summary: &str) -> Reply {
        Reply::Write(Ok(git::WriteOutcome {
            summary: summary.into(),
            commit: None,
            backup: None,
        }))
    }

    #[test]
    fn a_write_waits_for_a_status_runs_first_and_is_never_doubled() {
        let mut s = RepositoryState::new("repo".into());
        // No root yet: nothing to write to.
        assert!(!s.request_write(git::WriteOp::StageAll));
        let mut s = ready("repo");
        s.select(1, DiffMode::Unstaged);
        assert!(s.request_write(git::WriteOp::Stage(vec!["b".into()])));
        assert!(s.write.busy());
        assert!(!s.request_write(git::WriteOp::StageAll));
        // The queued diff for the selection waits behind the write.
        let write = s.next_job().unwrap();
        assert!(matches!(&write, Job::Write { root, .. } if root == std::path::Path::new("/repo")));
        assert!(s.next_job().is_none());
        s.complete(write, outcome("Staged 1 file"));
        assert!(!s.write.busy());
        assert_eq!(s.write.note, Some(("Staged 1 file".into(), false)));
        // The write refreshed status, and the staged file stays selected.
        let status = s.next_job().unwrap();
        assert!(matches!(status, Job::Status { .. }));
        let mut staged = (*snapshot()).clone_for_test();
        staged.changes[1].index = b'A';
        staged.changes[1].worktree = b' ';
        s.complete(status, Reply::Status(Ok(Arc::new(staged))));
        assert_eq!(
            s.selected,
            Some(DiffKey {
                file: 1,
                mode: DiffMode::Staged
            })
        );
    }

    #[test]
    fn a_failed_write_reports_and_still_refreshes() {
        let mut s = ready("repo");
        assert!(s.request_write(git::WriteOp::Commit {
            message: "x".into()
        }));
        let write = s.next_job().unwrap();
        s.complete(write, Reply::Write(Err("hook failed".into())));
        assert_eq!(s.write.note, Some(("hook failed".into(), true)));
        assert!(matches!(s.next_job(), Some(Job::Status { .. })));
    }

    fn commit(id: &str) -> git::Commit {
        git::Commit {
            id: id.into(),
            short: id.into(),
            author: "A".into(),
            time: 0,
            subject: id.into(),
        }
    }

    #[test]
    fn history_pages_append_and_a_refresh_reloads_only_when_shown() {
        let mut s = ready("repo");
        s.request_history();
        let first = s.next_job().unwrap();
        assert!(matches!(first, Job::Log { skip: 0, .. }));
        s.complete(
            first,
            Reply::Log(Ok(git::History {
                commits: vec![commit("a"), commit("b")],
                more: true,
            })),
        );
        s.request_more_history();
        s.request_more_history();
        let next = s.next_job().unwrap();
        assert!(matches!(next, Job::Log { skip: 2, .. }));
        assert!(s.next_job().is_none());
        s.complete(
            next,
            Reply::Log(Ok(git::History {
                commits: vec![commit("c")],
                more: false,
            })),
        );
        let list = s.commits.list.ready().unwrap();
        assert_eq!(list.commits.len(), 3);
        assert!(!list.more && !s.commits.loading_more);

        // Shown history reloads after a refresh, once a status provides the root.
        s.refresh();
        assert!(matches!(s.commits.list, Load::Loading));
        assert!(matches!(s.next_job(), Some(Job::Status { .. })));
    }

    #[test]
    fn staging_keeps_history_and_a_commit_reloads_it() {
        let mut s = ready("repo");
        s.request_history();
        let page = s.next_job().unwrap();
        s.complete(
            page,
            Reply::Log(Ok(git::History {
                commits: vec![commit("a")],
                more: false,
            })),
        );
        assert_eq!(s.staged(), 1);
        assert!(s.request_write(git::WriteOp::StageAll));
        let write = s.next_job().unwrap();
        s.complete(write, outcome("Staged all changes"));
        // Only status is reread; the listed commits stay.
        assert!(matches!(s.next_job(), Some(Job::Status { .. })));
        assert_eq!(s.commits.list.ready().unwrap().commits.len(), 1);

        let mut s = ready("repo");
        s.request_history();
        let page = s.next_job().unwrap();
        s.complete(page, Reply::Log(Ok(git::History::default())));
        assert!(s.request_write(git::WriteOp::Commit {
            message: "m".into()
        }));
        let write = s.next_job().unwrap();
        s.complete(
            write,
            Reply::Write(Ok(git::WriteOutcome {
                summary: "Committed".into(),
                commit: Some("abc".into()),
                backup: None,
            })),
        );
        assert!(matches!(s.commits.list, Load::Loading));
        let status = s.next_job().unwrap();
        assert!(matches!(status, Job::Status { .. }));
        s.complete(status, Reply::Status(Ok(snapshot())));
        let diff = s.next_job().unwrap();
        assert!(matches!(diff, Job::Diff { .. }));
        s.complete(diff, Reply::Diff(Err("gone".into())));
        assert!(matches!(s.next_job(), Some(Job::Log { skip: 0, .. })));
    }

    #[test]
    fn a_stale_commit_patch_is_cached_but_not_shown() {
        let mut s = ready("repo");
        s.select_commit("a".into());
        let a = s.next_job().unwrap();
        s.select_commit("b".into());
        s.complete(
            a,
            Reply::Show(Ok(Arc::new(Diff::message("a", Duration::ZERO)))),
        );
        assert!(matches!(s.commits.patch, Load::Loading));
        let b = s.next_job().unwrap();
        assert!(matches!(&b, Job::Show { id, .. } if id == "b"));
        s.complete(
            b,
            Reply::Show(Ok(Arc::new(Diff::message("b", Duration::ZERO)))),
        );
        s.select_commit("a".into());
        assert_eq!(&*s.commits.patch.ready().unwrap().lines[0].text, "a");
        assert!(s.next_job().is_none());
    }

    fn page(ids: &[&str], more: bool) -> Reply {
        Reply::Log(Ok(git::History {
            commits: ids.iter().map(|id| commit(id)).collect(),
            more,
        }))
    }

    #[test]
    fn later_history_pages_count_from_the_first_pages_newest_commit() {
        let mut s = ready("repo");
        s.request_history();
        let first = s.next_job().unwrap();
        assert!(matches!(
            &first,
            Job::Log {
                tip: None,
                skip: 0,
                ..
            }
        ));
        s.complete(first, page(&["new", "old"], true));
        s.request_more_history();
        let next = s.next_job().unwrap();
        assert!(matches!(&next, Job::Log { tip: Some(tip), skip: 2, .. } if tip == "new"));
    }

    #[test]
    fn a_write_runs_while_a_refresh_is_still_reading_status() {
        let mut s = ready("repo");
        s.refresh();
        assert!(s.snapshot.ready().is_none());
        assert!(s.request_write(git::WriteOp::StageAll));
        assert!(matches!(s.next_job(), Some(Job::Write { .. })));
    }

    #[test]
    fn a_write_queued_before_switching_checkouts_is_refused_with_a_note() {
        let mut s = ready("repo");
        assert!(s.request_write(git::WriteOp::StageAll));
        s.open("other".into());
        assert!(matches!(s.next_job(), Some(Job::Status { .. })));
        assert!(!s.write.busy());
        assert!(matches!(&s.write.note, Some((_, true))));
    }

    #[test]
    fn history_waiting_on_a_failed_status_reports_the_error() {
        let mut s = RepositoryState::new("repo".into());
        s.request_history();
        s.select_commit("a".into());
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Err("not a repository".into())));
        assert!(matches!(&s.commits.list, Load::Error(e) if e == "not a repository"));
        assert!(matches!(&s.commits.patch, Load::Error(_)));
        assert!(s.next_job().is_none());
    }

    #[test]
    fn leaving_history_stops_a_commit_from_rereading_it() {
        let mut s = ready("repo");
        s.request_history();
        let first = s.next_job().unwrap();
        s.complete(first, page(&["a"], false));
        s.leave_history();
        assert!(s.request_write(git::WriteOp::Commit {
            message: "m".into()
        }));
        let write = s.next_job().unwrap();
        s.complete(
            write,
            Reply::Write(Ok(git::WriteOutcome {
                summary: "Committed".into(),
                commit: Some("b".into()),
                backup: None,
            })),
        );
        assert!(matches!(s.commits.list, Load::Empty));
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Ok(snapshot())));
        let diff = s.next_job().unwrap();
        s.complete(diff, Reply::Diff(Err("gone".into())));
        assert!(s.next_job().is_none());
        // Coming back loads it fresh.
        s.request_history();
        assert!(matches!(s.next_job(), Some(Job::Log { skip: 0, .. })));
    }

    #[test]
    fn a_refresh_keeps_the_selected_commit_and_its_patch() {
        let mut s = ready("repo");
        s.request_history();
        let first = s.next_job().unwrap();
        s.complete(first, page(&["a"], false));
        s.select_commit("a".into());
        let show = s.next_job().unwrap();
        s.complete(
            show,
            Reply::Show(Ok(Arc::new(Diff::message("a", Duration::ZERO)))),
        );
        s.refresh();
        assert_eq!(s.commits.selected.as_deref(), Some("a"));
        assert!(s.commits.patch.ready().is_some());
        // The list reloads; the patch is not read again.
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Ok(snapshot())));
        let diff = s.next_job().unwrap();
        s.complete(diff, Reply::Diff(Err("gone".into())));
        assert!(matches!(s.next_job(), Some(Job::Log { .. })));
        assert!(s.next_job().is_none());
    }

    #[test]
    fn conflicts_are_counted_once_per_status() {
        let mut s = RepositoryState::new("repo".into());
        let status = s.next_job().unwrap();
        let mut conflicted = (*snapshot()).clone_for_test();
        conflicted.changes[0].index = b'U';
        conflicted.changes[0].worktree = b'U';
        s.complete(status, Reply::Status(Ok(Arc::new(conflicted))));
        assert_eq!(s.conflicts(), 1);
        s.refresh();
        assert_eq!(s.conflicts(), 0);
    }

    #[test]
    fn stepping_reads_the_next_file_ahead_and_a_click_does_not() {
        let mut s = ready("repo");
        assert!(s.next_job().is_none());
        // Stepping forward to file 1 would read file 0 next (the list wraps),
        // which is cached already, so only file 1 itself is read.
        s.step(1, DiffMode::Unstaged, true);
        let diff = s.next_job().unwrap();
        s.complete(
            diff,
            Reply::Diff(Ok(Arc::new(Diff::message("one", Duration::ZERO)))),
        );
        assert!(s.next_job().is_none(), "file 0 is cached");
        // With an empty cache, a step reads the following file ahead.
        s.refresh();
        let status = s.next_job().unwrap();
        s.complete(status, Reply::Status(Ok(snapshot())));
        let first = s.next_job().unwrap();
        s.complete(first, Reply::Diff(Err("gone".into())));
        s.step(1, DiffMode::Unstaged, false);
        let one = s.next_job().unwrap();
        s.complete(
            one,
            Reply::Diff(Ok(Arc::new(Diff::message("one", Duration::ZERO)))),
        );
        assert_eq!(prefetched(&mut s), 0);
        s.step(0, DiffMode::Unstaged, false);
        assert_eq!(&*s.diff.ready().unwrap().lines[0].text, "ahead");
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
