//! Commit history, read-only, with the same hardening as status and diff.

use super::{DIFF_LIMIT, Result, STATUS_LIMIT, command, format_diff, head_exists};
use crate::model::Diff;
use std::{ffi::OsStr, path::Path, time::Instant};

/// A page larger than this is a UI bug, and bounds the output Git must produce.
const MAX_PAGE: usize = 500;
const FIELDS: usize = 5;

/// One commit in the history list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// Full object ID in hex (40 for SHA-1, 64 for SHA-256 repositories).
    pub id: String,
    /// Abbreviated ID as Git prints it.
    pub short: String,
    pub author: String,
    /// Author time, seconds since the Unix epoch.
    pub time: i64,
    pub subject: String,
}

/// A page of history, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    pub commits: Vec<Commit>,
    /// More commits exist after this page.
    pub more: bool,
}

/// Up to `limit` commits reachable from `tip` (HEAD when `None`), skipping the
/// first `skip`.
/// An unborn branch has an empty history, not an error.
pub fn log(root: &Path, tip: Option<&str>, skip: usize, limit: usize) -> Result<History> {
    let limit = limit.min(MAX_PAGE);
    if tip.is_some_and(|tip| !is_object_id(tip)) {
        return Err("Not a full commit ID".into());
    }
    if limit == 0 || (tip.is_none() && !head_exists(root)?) {
        return Ok(History::default());
    }
    // Later pages start from the first page's newest commit, so a commit made
    // meanwhile cannot shift the pages and duplicate or skip entries.
    let start = tip.unwrap_or("HEAD");
    let skip = format!("--skip={skip}");
    // One more than asked, to learn whether another page exists.
    let count = format!("-n{}", limit + 1);
    let args = [
        "log",
        "--no-show-signature",
        "-z",
        // Fields are NUL-separated; -z makes NUL end each record too.
        "--format=%H%x00%h%x00%an%x00%at%x00%s",
        &count,
        &skip,
        "--end-of-options",
        start,
        "--",
    ]
    .map(OsStr::new);
    let output = command(root, &args, STATUS_LIMIT)?;
    if output.truncated {
        return Err("History page exceeds the safe read limit. Ask for fewer commits.".into());
    }
    let mut commits = parse_log(&output.bytes)?;
    let more = commits.len() > limit;
    commits.truncate(limit);
    Ok(History { commits, more })
}

pub(super) fn parse_log(bytes: &[u8]) -> Result<Vec<Commit>> {
    let mut fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    // The final piece is empty after the last record's terminator.
    if fields.last().is_some_and(|f| f.is_empty()) {
        fields.pop();
    }
    if !fields.len().is_multiple_of(FIELDS) {
        return Err("Malformed Git history record".into());
    }
    fields
        .chunks(FIELDS)
        .map(|record| {
            let id = text(record[0]);
            if !is_object_id(&id) {
                return Err("Malformed Git history record".to_owned());
            }
            Ok(Commit {
                id,
                short: text(record[1]),
                author: display(record[2]),
                time: text(record[3])
                    .parse()
                    .map_err(|_| "Malformed Git commit time".to_owned())?,
                subject: display(record[4]),
            })
        })
        .collect()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Names and subjects are untrusted; control characters could disturb the UI.
fn display(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    cleaned.trim().to_owned()
}

fn is_object_id(id: &str) -> bool {
    matches!(id.len(), 40 | 64) && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// One commit's message and patch, against its first parent, as a `Diff`.
/// `id` must be a full object ID from `log`.
pub fn show(root: &Path, id: &str) -> Result<Diff> {
    let start = Instant::now();
    if !is_object_id(id) {
        return Err("Not a full commit ID.".into());
    }
    let revision = format!("{id}^{{commit}}");
    let args = [
        "show",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-notes",
        "--no-show-signature",
        "--unified=3",
        "--diff-merges=first-parent",
        "--submodule=short",
        "--ignore-submodules=dirty",
        "--date=iso",
        // Message lines are indented so none can pass for a diff header or hunk.
        "--format=commit %H%nAuthor: %an <%ae>%nDate:   %ad%n%n%w(0,4,4)%B%w(0,0,0)",
        "--end-of-options",
        &revision,
        "--",
    ]
    .map(OsStr::new);
    let output = command(root, &args, DIFF_LIMIT)?;
    let mut diff = format_diff(&output.bytes, output.truncated, start.elapsed());
    diff.elapsed = start.elapsed();
    Ok(diff)
}
