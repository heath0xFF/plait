//! Explicit Git writes the user asks for: staging, unstaging, and committing.
//!
//! Reads elsewhere in `git` neutralize filters and never run hooks. A write the
//! user chose must produce what their own `git add` or `git commit` would, so
//! it runs with the repository's configuration: clean filters (such as LFS)
//! run while staging, and commit hooks run while committing.

use super::{
    Result, STATUS_LIMIT, TIMEOUT, base_command, head_exists, log::log, run, validate_relative,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// Filters (LFS) can be slow on large files.
const STAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// Pre-commit hooks may run formatters or tests.
const COMMIT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PATHS: usize = 100_000;
/// Hook output shown to the user is clipped to this many characters.
const ERROR_TEXT_LIMIT: usize = 2000;
const BUSY: &str = "Another Git process is using this repository. Try again when it finishes.";

/// One write. Paths are relative to the repository root, as `Change::path`
/// reports them; a rename lists both its path and its original.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteOp {
    Stage(Vec<PathBuf>),
    Unstage(Vec<PathBuf>),
    StageAll,
    UnstageAll,
    /// Commit what is staged. The message is used as written, apart from
    /// trailing whitespace; it must not be blank.
    Commit {
        message: String,
    },
}

/// What a write did, in words for the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteOutcome {
    /// Such as "Staged 2 files" or "Committed 1a2b3c4: Fix the parser".
    pub summary: String,
    /// The new commit's full ID, after a commit.
    pub commit: Option<String>,
}

/// Run one write in the repository at `root`. Blocking: call on a background thread.
pub fn write(root: &Path, op: &WriteOp) -> Result<WriteOutcome> {
    match op {
        WriteOp::Stage(paths) => {
            let input = pathspec_input(paths)?;
            let summary = format!("Staged {}", files(paths_count(paths)));
            git(
                root,
                &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
                Some(input),
                STAGE_TIMEOUT,
            )?;
            Ok(done(summary))
        }
        WriteOp::StageAll => {
            git(root, &["add", "-A"], None, STAGE_TIMEOUT)?;
            Ok(done("Staged all changes".into()))
        }
        WriteOp::Unstage(paths) => {
            let input = pathspec_input(paths)?;
            let summary = format!("Unstaged {}", files(paths_count(paths)));
            unstage(root, Some(input))?;
            Ok(done(summary))
        }
        WriteOp::UnstageAll => {
            unstage(root, None)?;
            Ok(done("Unstaged all changes".into()))
        }
        WriteOp::Commit { message } => commit(root, message),
    }
}

fn done(summary: String) -> WriteOutcome {
    WriteOutcome {
        summary,
        commit: None,
    }
}

fn files(count: usize) -> String {
    match count {
        1 => "1 file".into(),
        n => format!("{n} files"),
    }
}

fn unique(paths: &[PathBuf]) -> BTreeSet<&Path> {
    paths.iter().map(PathBuf::as_path).collect()
}

fn paths_count(paths: &[PathBuf]) -> usize {
    unique(paths).len()
}

/// NUL-terminated pathspecs for `--pathspec-from-file=-`, which has no argv length limit.
fn pathspec_input(paths: &[PathBuf]) -> Result<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    let paths = unique(paths);
    if paths.is_empty() {
        return Err("Choose at least one file.".into());
    }
    if paths.len() > MAX_PATHS {
        return Err(format!("Too many files at once (limit {MAX_PATHS})."));
    }
    let mut input = Vec::new();
    for path in paths {
        validate_relative(path)?;
        let bytes = path.as_os_str().as_bytes();
        if bytes.contains(&0) {
            return Err("Git returned an unsafe relative path".into());
        }
        input.extend_from_slice(bytes);
        input.push(0);
    }
    Ok(input)
}

/// Return paths (or everything) from the index to their HEAD state.
fn unstage(root: &Path, input: Option<Vec<u8>>) -> Result<()> {
    let nul = ["--pathspec-from-file=-", "--pathspec-file-nul"];
    let timeout = STAGE_TIMEOUT;
    if head_exists(root)? {
        return match input {
            Some(input) => {
                let args = ["restore", "--staged", nul[0], nul[1]];
                git(root, &args, Some(input), timeout).map(drop)
            }
            None => git(root, &["reset", "-q"], None, timeout).map(drop),
        };
    }
    // No HEAD to restore from: dropping index entries returns new files to untracked.
    // -f because the index entry may differ from both the file and the (absent) HEAD.
    match input {
        Some(input) => {
            let args = ["rm", "--cached", "-r", "-q", "-f", nul[0], nul[1]];
            git(root, &args, Some(input), timeout).map(drop)
        }
        None => {
            let args = [
                "rm",
                "--cached",
                "-r",
                "-q",
                "-f",
                "--ignore-unmatch",
                "--",
                ".",
            ];
            git(root, &args, None, timeout).map(drop)
        }
    }
}

fn commit(root: &Path, message: &str) -> Result<WriteOutcome> {
    let message = message.trim_end();
    if message.trim().is_empty() {
        return Err("Write a commit message first.".into());
    }
    if message.contains('\0') {
        return Err("The commit message cannot contain NUL characters.".into());
    }
    if !has_staged_changes(root)? {
        return Err("Nothing is staged. Stage changes before committing.".into());
    }
    let mut input = message.as_bytes().to_vec();
    input.push(b'\n');
    // whitespace cleanup keeps lines that start with '#', unlike Git's default.
    let args = ["commit", "-q", "--cleanup=whitespace", "-F", "-"];
    git(root, &args, Some(input), COMMIT_TIMEOUT)?;
    // Read back what Git recorded, after hooks and cleanup.
    let head = log(root, None, 0, 1)?
        .commits
        .into_iter()
        .next()
        .ok_or("Git committed but HEAD is unreadable")?;
    Ok(WriteOutcome {
        summary: format!("Committed {}: {}", head.short, head.subject),
        commit: Some(head.id),
    })
}

/// `diff --cached --quiet` exits 1 when the index differs from HEAD (or from empty).
fn has_staged_changes(root: &Path) -> Result<bool> {
    let mut command = write_command(root);
    let raw = run(
        command.args(["diff", "--cached", "--quiet", "--no-ext-diff"]),
        None,
        STATUS_LIMIT,
        TIMEOUT,
    )?;
    match raw.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(failure_text(&raw)),
    }
}

/// Repository configuration applies as for the user's own command; only the
/// terminal-facing and ambient behavior is pinned.
fn write_command(root: &Path) -> Command {
    let mut command = base_command(root);
    // A commit must never wait on an editor nobody can see.
    command
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true");
    command
}

fn git(root: &Path, args: &[&str], input: Option<Vec<u8>>, timeout: Duration) -> Result<()> {
    let mut command = write_command(root);
    // A stopped write can leave Git's lock behind; say how to recover.
    let raw = run(command.args(args), input, STATUS_LIMIT, timeout).map_err(|error| {
        if error.contains("was stopped") {
            format!("{error} If .git/index.lock remains, remove it once no Git process is running.")
        } else {
            error
        }
    })?;
    if raw.status.success() {
        return Ok(());
    }
    Err(failure_text(&raw))
}

/// A user-facing reason: Git's stderr plus any stdout, so hook output shows.
fn failure_text(raw: &super::Raw) -> String {
    let mut text = String::from_utf8_lossy(&raw.err.bytes).trim().to_owned();
    let out = String::from_utf8_lossy(&raw.out.bytes);
    if !out.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(out.trim());
    }
    explain(&text, raw.status.code())
}

fn explain(text: &str, code: Option<i32>) -> String {
    if text.contains("index.lock") {
        return BUSY.into();
    }
    if text.is_empty() {
        return format!("Git failed with status {}.", code.unwrap_or(-1));
    }
    let mut shown: String = text.chars().take(ERROR_TEXT_LIMIT).collect();
    if text.chars().count() > ERROR_TEXT_LIMIT {
        shown.push_str(" … [output clipped]");
    }
    format!("Git: {shown}")
}
