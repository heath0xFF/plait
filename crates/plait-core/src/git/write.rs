//! Explicit Git writes the user asks for: staging, unstaging, committing,
//! and exchanging commits with the branch's remote (push, publish, fetch).
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
    sync::OnceLock,
    time::Duration,
};

/// Filters (LFS) can be slow on large files.
const STAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// Pre-commit hooks may run formatters or tests.
const COMMIT_TIMEOUT: Duration = Duration::from_secs(120);
/// Push and fetch may run hooks and cross a slow network.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
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
    /// Push as the user's own `git push` would, to the configured destination.
    Push,
    /// Push a branch with no upstream to its remote and make that its upstream.
    Publish,
    /// Fetch from the current branch's remote, or `origin`.
    Fetch,
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
        WriteOp::Push => {
            remote_git(root, &["push", "-q"])?;
            Ok(done(match destination(root) {
                Some(to) => format!("Pushed to {to}"),
                None => "Pushed".into(),
            }))
        }
        WriteOp::Publish => {
            let remote = publish_remote(root)?;
            remote_git(
                root,
                &["push", "-q", "-u", "--end-of-options", &remote, "HEAD"],
            )?;
            Ok(done(format!("Published to {remote}")))
        }
        WriteOp::Fetch => {
            remote_git(root, &["fetch", "-q"])?;
            Ok(done("Fetched".into()))
        }
    }
}

/// The ref a plain push updates, such as `origin/main`, for the summary.
fn destination(root: &Path) -> Option<String> {
    let mut command = write_command(root);
    command.args([
        "rev-parse",
        "--abbrev-ref",
        "--symbolic-full-name",
        "@{push}",
    ]);
    let raw = run(&mut command, None, STATUS_LIMIT, TIMEOUT).ok()?;
    let text = String::from_utf8_lossy(&raw.out.bytes).trim().to_owned();
    (raw.status.success() && !text.is_empty()).then_some(text)
}

/// `remote.pushDefault`, else `origin`, else the only remote.
fn publish_remote(root: &Path) -> Result<String> {
    let read = |args: &[&str]| -> Result<String> {
        let mut command = write_command(root);
        let raw = run(command.args(args), None, STATUS_LIMIT, TIMEOUT)?;
        Ok(String::from_utf8_lossy(&raw.out.bytes).into_owned())
    };
    let preferred = read(&["config", "--get", "remote.pushDefault"])?;
    let remotes = read(&["remote"])?;
    let remotes: Vec<&str> = remotes
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .collect();
    let chosen = match preferred.trim() {
        name if !name.is_empty() => name,
        _ if remotes.contains(&"origin") => "origin",
        _ if remotes.len() == 1 => remotes[0],
        _ if remotes.is_empty() => {
            return Err("This repository has no remote to publish to.".into());
        }
        _ => {
            return Err(
                "There are several remotes and none is named origin. Publish this branch once from Terminal."
                    .into(),
            );
        }
    };
    Ok(chosen.to_owned())
}

/// A push or fetch. The user's credential helpers are asked first; the GitHub
/// CLI's helper, when installed, is tried after them, so a `gh auth login` is
/// enough for HTTPS remotes. Git hands the token to itself; Plait never sees it.
fn remote_git(root: &Path, args: &[&str]) -> Result<()> {
    let mut command = write_command(root);
    if let Some(helper) = gh_helper() {
        command.args(["-c", helper]);
    }
    let raw = run(command.args(args), None, STATUS_LIMIT, NETWORK_TIMEOUT)?;
    if raw.status.success() {
        return Ok(());
    }
    let text = failure_text(&raw);
    Err(network_hint(&text).map_or(text.clone(), |hint| format!("{hint}\n{text}")))
}

/// `credential.helper=!'/path/to/gh' auth git-credential`, found once.
fn gh_helper() -> Option<&'static str> {
    static HELPER: OnceLock<Option<String>> = OnceLock::new();
    HELPER
        .get_or_init(|| {
            let gh = find_program("gh")?;
            let gh = gh.to_str()?;
            // Quoted for the shell Git runs `!` helpers in.
            let quoted = format!("'{}'", gh.replace('\'', r"'\''"));
            Some(format!("credential.helper=!{quoted} auth git-credential"))
        })
        .as_deref()
}

/// An app started from Finder has a minimal PATH, so also look where
/// package managers install command-line tools.
fn find_program(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/run/current-system/sw/bin",
            ]
            .map(PathBuf::from),
        )
        .chain(
            home.iter()
                .flat_map(|home| [home.join(".nix-profile/bin"), home.join(".local/bin")]),
        )
        .map(|dir| dir.join(name))
        .find(|candidate| {
            std::fs::metadata(candidate)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// What to do about the common push and fetch failures, in words.
pub(super) fn network_hint(text: &str) -> Option<&'static str> {
    let has = |needle: &str| text.contains(needle);
    if has("could not read Username")
        || has("terminal prompts disabled")
        || has("Authentication failed")
    {
        Some(
            "The remote needs a sign-in. For GitHub over HTTPS, run `gh auth login`; otherwise set up a credential helper or an SSH key.",
        )
    } else if has("Permission denied (publickey)") {
        Some(
            "The remote refused your SSH key. Check that ssh-agent holds a key the remote accepts.",
        )
    } else if has("Host key verification failed") {
        Some(
            "SSH does not know this remote's host key yet. Connect once from Terminal to confirm it.",
        )
    } else if has("[rejected]") || has("non-fast-forward") || has("fetch first") {
        Some(
            "The remote has commits this branch does not. Pull them from Terminal, then push again.",
        )
    } else if has("Could not resolve host")
        || has("Network is unreachable")
        || has("Connection timed out")
    {
        Some("Cannot reach the remote. Check the network connection.")
    } else {
        None
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
