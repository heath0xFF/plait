use std::{path::PathBuf, sync::Arc, time::Duration};

/// An independent execution environment. Before M3 only this computer exists.
pub struct Machine {
    pub name: String,
    pub local: bool,
}

/// A checkout and the machines that hold it. Threads come from each machine's host.
pub struct Project {
    pub checkout: PathBuf,
    pub name: String,
    pub machines: Vec<Machine>,
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
