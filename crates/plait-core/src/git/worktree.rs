//! Optional per-thread worktrees: a second checkout of the same repository on
//! its own branch, so a thread can work beside another without sharing files.
//!
//! Plait creates them beside the repository, in `<name>.worktrees/`, and removes
//! them only through `git worktree remove`, which refuses uncommitted work.

use super::{Result, head_exists, write::git};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Checkout runs the repository's filters and post-checkout hook.
const CHECKOUT_TIMEOUT: Duration = Duration::from_secs(120);
/// Fresh names to try before giving up on collisions.
const ATTEMPTS: u32 = 8;

/// A worktree Plait created: where it is checked out and the branch it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

/// Check out HEAD of the repository at `root` on a new `plait/<name>` branch
/// in a new folder beside it. Blocking: call on a background thread.
pub fn add_worktree(root: &Path) -> Result<Worktree> {
    add_worktree_with_common_wait(root, crate::lease::COMMON_METADATA_WAIT)
}

pub(crate) fn add_worktree_with_common_wait(
    root: &Path,
    common_wait: Duration,
) -> Result<Worktree> {
    // Hold a shared lifecycle lease on the parent checkout (allowing an active
    // writer or reader there to continue) and serialize shared repository
    // metadata (refs and the worktree registry) exclusively on common_dir.
    let lease = crate::lease::acquire_git_checkout(
        root,
        None,
        crate::lease::LeaseClass::Reader,
        Some(common_wait),
    )
    .map_err(String::from)?;
    add_worktree_unlocked(lease.root())
}

fn add_worktree_unlocked(root: &Path) -> Result<Worktree> {
    if !head_exists(root)? {
        return Err("Make a first commit before starting a thread in its own worktree.".into());
    }
    let (Some(parent), Some(name)) = (root.parent(), root.file_name()) else {
        return Err("This repository has no folder to put a worktree beside.".into());
    };
    let mut folder = name.to_owned();
    folder.push(".worktrees");
    let base = parent.join(folder);
    let seed = seed();
    for attempt in 0..ATTEMPTS {
        let name = format!("{:06x}", seed.wrapping_add(attempt) & 0xff_ffff);
        let path = base.join(&name);
        let branch = format!("plait/{name}");
        if path.symlink_metadata().is_ok() || branch_exists(root, &branch)? {
            continue;
        }
        let target = path
            .to_str()
            .ok_or("The worktree folder path is not UTF-8.")?;
        git(
            root,
            &["worktree", "add", "-q", "-b", &branch, target, "HEAD"],
            None,
            CHECKOUT_TIMEOUT,
        )?;
        // The host names checkouts by their resolved path; match it.
        let path = path.canonicalize().map_err(|error| error.to_string())?;
        return Ok(Worktree { path, branch });
    }
    Err("Could not find an unused worktree name. Try again.".into())
}

/// Remove a worktree of the repository at `main`. Git refuses one with
/// modified or untracked files; its branch and commits are kept.
pub fn remove_worktree(main: &Path, path: &Path) -> Result<()> {
    remove_worktree_with_common_wait(main, path, crate::lease::COMMON_METADATA_WAIT)
}

pub(crate) fn remove_worktree_with_common_wait(
    main: &Path,
    path: &Path,
    common_wait: Duration,
) -> Result<()> {
    let main_paths = crate::lease::resolve_git_checkout(main).map_err(String::from)?;
    let target_paths = crate::lease::resolve_git_checkout(path).map_err(String::from)?;
    if main_paths.root == target_paths.root {
        return Err("Cannot remove the main repository checkout as a worktree.".into());
    }
    if main_paths.common_dir != target_paths.common_dir {
        return Err("This folder is not a worktree of the selected repository.".into());
    }
    // Hold exclusive lifecycle + writer leases on the target worktree before
    // taking the shared common-directory metadata lock.
    let _lease = crate::lease::acquire_git_checkout(
        &target_paths.root,
        None,
        crate::lease::LeaseClass::Exclusive,
        Some(common_wait),
    )
    .map_err(String::from)?;
    remove_worktree_unlocked(&main_paths.root, &target_paths.root)
}

fn remove_worktree_unlocked(main: &Path, path: &Path) -> Result<()> {
    let target = path
        .to_str()
        .ok_or("The worktree folder path is not UTF-8.")?;
    git(
        main,
        &["worktree", "remove", target],
        None,
        CHECKOUT_TIMEOUT,
    )
    .map_err(|error| {
        if error.contains("modified or untracked files") {
            "This worktree has uncommitted changes. Commit or discard them first.".into()
        } else {
            error
        }
    })?;
    // Leave the shared folder once its last worktree is gone; a non-empty one stays.
    if let Some(parent) = path.parent()
        && parent.extension().is_some_and(|ext| ext == "worktrees")
    {
        let _ = std::fs::remove_dir(parent);
    }
    Ok(())
}

pub(super) fn branch_exists(root: &Path, branch: &str) -> Result<bool> {
    let reference = format!("refs/heads/{branch}");
    let mut command = super::read_command(root)?;
    let raw = super::run(
        command.args(["rev-parse", "--verify", "-q", &reference]),
        None,
        super::STATUS_LIMIT,
        super::TIMEOUT,
    )?;
    match raw.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(raw.failure()),
    }
}

/// Short names only need to differ from the few worktrees beside them.
fn seed() -> u32 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (nanos as u32) ^ (std::process::id() << 12)
}
