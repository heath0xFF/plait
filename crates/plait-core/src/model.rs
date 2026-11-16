use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadStatus {
    Ready,
    Working,
    NeedsInput,
    Complete,
}

impl ThreadStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Working => "Working",
            Self::NeedsInput => "Needs input",
            Self::Complete => "Complete",
        }
    }
}

#[derive(Clone)]
pub struct Event {
    pub author: &'static str,
    pub time: &'static str,
    pub body: &'static str,
    pub tool: bool,
}

pub struct Thread {
    pub title: &'static str,
    pub status: ThreadStatus,
    pub events: Arc<Vec<Event>>,
}

pub struct Machine {
    pub name: &'static str,
    pub local: bool,
    pub threads: Vec<Thread>,
}

pub struct Project {
    pub name: String,
    pub machines: Vec<Machine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub project: usize,
    pub machine: usize,
    pub thread: usize,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub path: PathBuf,
    pub original: Option<PathBuf>,
    pub index: u8,
    pub worktree: u8,
}

impl Change {
    pub fn staged(&self) -> bool {
        !matches!(self.index, b' ' | b'?')
    }
    pub fn unstaged(&self) -> bool {
        self.worktree != b' '
    }
    pub fn label(&self) -> &'static str {
        if self.index == b'?' {
            "New"
        } else if self.index == b'U'
            || self.worktree == b'U'
            || matches!((self.index, self.worktree), (b'A', b'A') | (b'D', b'D'))
        {
            "Conflict"
        } else if self.index == b'R' || self.worktree == b'R' {
            "Renamed"
        } else if self.index == b'D' || self.worktree == b'D' {
            "Deleted"
        } else if self.index == b'A' {
            "Added"
        } else {
            "Modified"
        }
    }
    pub fn display_path(&self) -> String {
        self.path
            .to_string_lossy()
            .chars()
            .flat_map(|c| {
                if c.is_control() {
                    c.escape_default().collect::<Vec<_>>()
                } else {
                    vec![c]
                }
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct Snapshot {
    pub root: PathBuf,
    pub branch: String,
    pub changes: Vec<Change>,
    pub elapsed: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffMode {
    Unstaged,
    Staged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Context,
    Added,
    Removed,
    Hunk,
    Header,
}

#[derive(Debug)]
pub struct DiffLine {
    pub text: Arc<str>,
    pub kind: DiffKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
}

#[derive(Debug)]
pub struct Diff {
    pub lines: Vec<DiffLine>,
    pub widest_line: usize,
    pub truncated: bool,
    pub elapsed: Duration,
}

impl Diff {
    pub fn message(text: &str, elapsed: Duration) -> Self {
        Self {
            lines: vec![DiffLine {
                text: Arc::from(text),
                kind: DiffKind::Header,
                old_line: None,
                new_line: None,
            }],
            widest_line: 0,
            truncated: false,
            elapsed,
        }
    }
}
