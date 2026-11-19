//! Host-side live inventory. Removal and preview consume the same preservation facts.
use super::*;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Preservation {
    pub changed: Option<u64>,
    pub unpushed: Option<u64>,
    pub exclusive: Option<u64>,
    pub refusal: Option<String>,
}
impl Preservation {
    pub fn require_safe(&self) -> Result<()> {
        self.refusal.clone().map_or(Ok(()), Err)
    }
}
const DIRTY: &str = "This worktree has uncommitted or ignored files. Preserve or remove them before removing the worktree.";
const EXCLUSIVE: &str = "This worktree has commits that are on no branch (made on a detached HEAD or kept only by its own refs). Removing it would lose them. Put them on a branch, then remove the worktree.";

pub(super) fn preservation(path: &Path, main: &Path, allow_unpushed: bool) -> Result<Preservation> {
    let dirty = read_worktree(
        path,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?;
    let mut entries = dirty.split(|b| *b == 0).filter(|v| !v.is_empty());
    let mut changed = 0;
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            return Err("Invalid Git status entry.".into());
        }
        changed += 1;
        if entry[..2].iter().any(|c| matches!(c, b'R' | b'C')) {
            entries.next().ok_or("Incomplete Git rename.")?;
        }
    }
    let hidden = refuse_hidden_edits(path).err();
    let exclusive = worktree_only_commits(path, main);
    let git_dir = text_read(path, &["rev-parse", "--absolute-git-dir"])?;
    let unfinished = [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "sequencer",
    ]
    .iter()
    .any(|name| fs::symlink_metadata(Path::new(&git_dir).join(name)).is_ok());
    let publication = publication(path);
    let refusal = if changed > 0 {
        Some(DIRTY.into())
    } else if hidden.is_some() {
        hidden
    } else if let Err(error) = &exclusive {
        Some(error.clone())
    } else if exclusive.as_ref().is_ok_and(|n| *n > 0) {
        Some(EXCLUSIVE.into())
    } else if unfinished {
        Some("This worktree has an unfinished Git operation.".into())
    } else if !allow_unpushed {
        match &publication {
            Err(error) => Some(error.clone()),
            Ok(n) if *n > 0 => Some("This worktree has unpushed commits. Push them or explicitly confirm preserving only the local branch.".into()),
            _ => None,
        }
    } else {
        None
    };
    Ok(Preservation {
        changed: Some(changed),
        unpushed: publication.ok(),
        exclusive: exclusive.ok(),
        refusal,
    })
}
fn publication(path: &Path) -> Result<u64> {
    let remote = text_read(path, &["rev-parse", "--symbolic-full-name", "@{upstream}"])
        .map_err(|_| "This worktree has unpushed or unknown publication status. Explicitly confirm preserving only the local branch.".to_owned())?;
    if !remote.starts_with("refs/remotes/") {
        return Err("This worktree tracks a local branch; publication is unknown. Explicitly confirm preserving only the local branch.".into());
    }
    let upstream = text_read(path, &["rev-parse", "--verify", "@{upstream}^{commit}"])
        .map_err(|_| "This worktree has unpushed or unknown publication status. Explicitly confirm keeping its work only on the local branch.".to_owned())?;
    text_read(path, &["rev-list", "--count", &format!("{upstream}..HEAD")])?
        .parse()
        .map_err(|_| "Invalid commit count.".into())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Checkout {
    pub path: PathBuf,
    pub main: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub base: Option<String>,
    pub thread_id: Option<String>,
    pub identity: Option<Identity>,
    pub missing: bool,
    pub removed: bool,
    pub preservation: Preservation,
}
/// All registered checkouts, including ones created outside Plait. Never infer ownership.
pub fn inventory(root: &Path, records: &[ManagedWorktree]) -> Result<Vec<Checkout>> {
    let paths = crate::lease::resolve_git_checkout(root).map_err(String::from)?;
    let raw = read_worktree(&paths.root, &["worktree", "list", "--porcelain", "-z"])?;
    let text = std::str::from_utf8(&raw).map_err(|_| "Worktree paths must be UTF-8.")?;
    let mut rows = Vec::new();
    for block in text.split("\0\0").filter(|s| !s.is_empty()) {
        if rows.len() == 256 {
            return Err(
                "This repository has more than 256 checkouts. Use Git to inspect the full list."
                    .into(),
            );
        }
        let mut fields = block.split('\0');
        let path = PathBuf::from(
            fields
                .next()
                .and_then(|s| s.strip_prefix("worktree "))
                .ok_or("Invalid Git worktree list.")?,
        );
        let fields: Vec<_> = fields.collect();
        let main = rows.is_empty();
        let managed = records.iter().find(|record| {
            record.path == path
                && record.parent.common == paths.common_dir
                && record.stage != ManagedStage::Removed
        });
        let branch = fields
            .iter()
            .find_map(|s| s.strip_prefix("branch refs/heads/"))
            .map(str::to_owned);
        let head = fields
            .iter()
            .find_map(|s| s.strip_prefix("HEAD "))
            .filter(|s| !s.bytes().all(|b| b == b'0'))
            .map(str::to_owned);
        let missing = fs::symlink_metadata(&path).is_err();
        rows.push(Checkout {
            path: path.clone(),
            main,
            branch,
            head,
            base: managed.map(|r| r.parent.base.clone()),
            thread_id: managed.map(|r| r.thread_id.clone()),
            identity: managed.and_then(|r| r.identity.clone()).or_else(|| {
                fs::symlink_metadata(&path)
                    .ok()
                    .filter(|m| m.is_dir() && !m.is_symlink())
                    .map(|m| Identity {
                        dev: m.dev().to_string(),
                        ino: m.ino().to_string(),
                    })
            }),
            missing,
            removed: false,
            preservation: Preservation::default(),
        });
    }
    let main = rows
        .first()
        .ok_or("Git returned no checkouts.")?
        .path
        .clone();
    // Removal keeps private build output. Keep its attribution visible afterwards.
    for record in records
        .iter()
        .filter(|r| r.stage == ManagedStage::Removed && r.parent.common == paths.common_dir)
    {
        if rows.iter().any(|row| row.path == record.path) {
            continue;
        }
        if rows.len() == 256 {
            return Err("This repository has more than 256 checkout records. Use Git to inspect the full list.".into());
        }
        rows.push(Checkout {path:record.path.clone(),main:false,branch:Some(record.branch.clone()),head:text_read(&main,&["rev-parse","--verify",&format!("refs/heads/{}^{{commit}}",record.branch)]).ok(),base:Some(record.parent.base.clone()),thread_id:Some(record.thread_id.clone()),identity:record.identity.clone(),missing:true,removed:true,preservation:Preservation {refusal:Some("This worktree was removed. Its branch, thread history and private build cache remain.".into()),..Default::default()}});
    }
    for row in &mut rows {
        if row.removed {
            continue;
        }
        let managed = records
            .iter()
            .find(|record| Some(&record.thread_id) == row.thread_id.as_ref());
        let result = if row.missing {
            Err(format!(
                "The worktree folder {} is missing. Nothing was removed. If you moved it, move it back; if its disk is not connected, connect it.",
                row.path.display()
            ))
        } else {
            preservation(&row.path, &main, false)
        };
        row.preservation = result.unwrap_or_else(|error| Preservation {
            refusal: Some(error),
            ..Default::default()
        });
        let ownership = if row.main {
            Some("The main checkout cannot be removed here.".into())
        } else if let Some(record) = managed {
            removal_identity(record).err()
        } else {
            Some("Plait did not create this worktree. Remove it with your Git tools.".into())
        };
        if ownership.is_some() && !row.missing {
            row.preservation.refusal = ownership;
        }
    }
    Ok(rows)
}
fn removal_identity(record: &ManagedWorktree) -> Result<()> {
    if !owned_shape(
        record,
        &worktree_base(&record.parent.main)?,
        &uuid_hex(&record.thread_id)?,
    ) {
        return Err(
            "This record does not name a worktree Plait created. Nothing was removed.".into(),
        );
    }
    verify_repository(&record.parent)?;
    verify_managed(record)?;
    let expected = record
        .identity
        .as_ref()
        .ok_or("Worktree has no recorded directory identity.")?;
    let _lease = crate::lease::acquire_git_checkout(
        &record.path,
        Some(expected.lease()?),
        crate::lease::LeaseClass::Exclusive,
        Some(Duration::ZERO),
    )
    .map_err(String::from)?;
    Ok(())
}
