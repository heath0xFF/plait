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
/// One configuration read per checkout serves the whole pass.
pub fn inventory(root: &Path, records: &[ManagedWorktree]) -> Result<Vec<Checkout>> {
    crate::git::sharing_config(|| read_inventory(root, records))
}
fn read_inventory(root: &Path, records: &[ManagedWorktree]) -> Result<Vec<Checkout>> {
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
    // A preview takes no lease: runs take theirs without waiting, so even a
    // brief hold here refuses one. Removal rechecks everything under its own.
    let id = expected.lease()?;
    crate::lease::git_cleanup::require_clear(id).map_err(String::from)?;
    crate::lease::cleanup::require_clear(id).map_err(String::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::Repo;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    const ID: &str = "12345678-1234-4abc-8abc-123456789abc";
    fn git_in(root: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn commit(root: &Path, name: &str) {
        fs::write(root.join(name), "work\n").unwrap();
        git_in(root, &["add", name]);
        git_in(
            root,
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@t",
                "commit",
                "-qm",
                name,
            ],
        );
    }
    fn repository() -> (Repo, ManagedWorktree) {
        let mut repo = Repo::new();
        repo.0 = repo.0.canonicalize().unwrap();
        fs::write(repo.0.join("file"), "committed\n").unwrap();
        fs::write(repo.0.join(".gitignore"), "*.log\n").unwrap();
        repo.git(&["add", "."]);
        repo.git(&["commit", "-qm", "initial"]);
        let tree =
            create_managed_worktree(&worktree_parent(&repo.0).unwrap(), ID, None, |_| Ok(()))
                .unwrap();
        (repo, tree)
    }
    fn clean(tree: &ManagedWorktree) {
        if let Ok(base) = worktree_base(&tree.parent.main) {
            let _ = fs::remove_dir_all(base);
        }
    }
    /// The Git commands a pass started, one line each, from a logging `git`
    /// placed first on PATH. The test re-runs itself in a child process so the
    /// changed PATH never reaches tests running beside it.
    pub(crate) fn counted_git(test: &str, child: impl FnOnce(&Path)) -> Option<Vec<String>> {
        if let Some(log) = std::env::var_os("PLAIT_GIT_COUNT_LOG") {
            child(Path::new(&log));
            return None;
        }
        let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join("git"))
            .find(|git| git.is_file())
            .unwrap();
        let dir = std::env::temp_dir().join(format!(
            "plait-git-count-{}-{}",
            std::process::id(),
            test.rsplit("::").next().unwrap()
        ));
        fs::create_dir_all(&dir).unwrap();
        let (shim, log) = (dir.join("git"), dir.join("log"));
        fs::write(
            &shim,
            format!("#!/bin/sh\nprintf 'GIT %s\\n' \"$*\" >> \"$PLAIT_GIT_COUNT_LOG\"\nexec '{}' \"$@\"\n", real.display()),
        )
        .unwrap();
        fs::set_permissions(&shim, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let mut path = std::ffi::OsString::from(&dir);
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap());
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env("PATH", path)
            .env("PLAIT_GIT_COUNT_LOG", &log)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let text = fs::read_to_string(&log).unwrap();
        let _ = fs::remove_dir_all(&dir);
        // Only what the pass started, not the repository's setup.
        let lines: Vec<_> = text.lines().map(str::to_owned).collect();
        let start = lines
            .iter()
            .rposition(|l| l == "PASS")
            .expect("pass marker");
        Some(lines[start + 1..].to_vec())
    }
    pub(crate) fn mark_pass(log: &Path) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(log)
            .unwrap();
        writeln!(file, "PASS").unwrap();
    }
    /// Each checkout's filter configuration is read once per inventory, not
    /// once per Git command; every read still carries the hardened flags.
    #[test]
    fn inventory_reads_each_checkouts_configuration_once() {
        let test =
            "git::worktree::inventory::tests::inventory_reads_each_checkouts_configuration_once";
        let Some(lines) = counted_git(test, |log| {
            let (repo, tree) = repository();
            repo.git(&["config", "filter.probe.clean", "false"]);
            for name in ["outside-a", "outside-b"] {
                let path = repo.0.with_file_name(format!(
                    "{}-{name}",
                    repo.0.file_name().unwrap().to_string_lossy()
                ));
                repo.git(&["worktree", "add", "-q", "-b", name, path.to_str().unwrap()]);
            }
            mark_pass(log);
            let rows = inventory(&repo.0, std::slice::from_ref(&tree)).unwrap();
            assert_eq!(rows.len(), 4);
            clean(&tree);
        }) else {
            return;
        };
        let config: Vec<_> = lines
            .iter()
            .filter(|l| l.ends_with("config --null --name-only --list"))
            .collect();
        let mut roots: Vec<_> = config
            .iter()
            .map(|l| l.split(" -C ").nth(1).unwrap().split(' ').next().unwrap())
            .collect();
        roots.sort();
        roots.dedup();
        assert_eq!(
            (config.len(), roots.len()),
            (4, 4),
            "one configuration read per checkout:\n{}",
            lines.join("\n")
        );
        let hardened = "GIT --no-pager --no-optional-locks --literal-pathspecs --no-replace-objects -c core.fsmonitor=false -c core.untrackedCache=false -c color.ui=false -C ";
        for line in &lines {
            assert!(line.starts_with(hardened), "unhardened read: {line}");
            assert!(
                config.contains(&line) || line.contains(" -c filter.probe.clean= "),
                "a read kept the filter driver: {line}"
            );
        }
        eprintln!(
            "inventory, main + 3 worktrees: {} Git processes",
            lines.len()
        );
    }
    /// Runs take their writer lease without waiting; a preview must never be the
    /// reason one is refused.
    #[test]
    fn inventory_never_refuses_a_run_starting_beside_it() {
        let (repo, tree) = repository();
        let stop = Arc::new(AtomicBool::new(false));
        let (path, done) = (tree.path.clone(), stop.clone());
        let runner = std::thread::spawn(move || {
            let mut refused = Vec::new();
            while !done.load(Ordering::Relaxed) {
                match crate::lease::acquire_checkout(&path, None, crate::lease::LeaseClass::Writer)
                {
                    Ok(lease) => drop(lease),
                    Err(error) => refused.push(error.message),
                }
            }
            refused
        });
        for _ in 0..8 {
            let rows = inventory(&repo.0, std::slice::from_ref(&tree)).unwrap();
            assert_eq!(rows.len(), 2);
        }
        stop.store(true, Ordering::Relaxed);
        let refused = runner.join().unwrap();
        assert!(
            refused.is_empty(),
            "runs refused beside a preview: {refused:?}"
        );
        clean(&tree);
    }
    /// The preview's answer and removal's answer come from one function. Feed
    /// both the same work and require the same decision and the same words.
    #[test]
    fn preview_and_removal_agree_on_every_kind_of_work() {
        type Setup<'a> = &'a dyn Fn(&ManagedWorktree);
        let cases: &[(&str, Setup)] = &[
            ("no upstream", &|_| {}),
            ("published", &|_| {}),
            ("untracked", &|t| {
                fs::write(t.path.join("new"), "x").unwrap()
            }),
            ("ignored", &|t| {
                fs::write(t.path.join("out.log"), "x").unwrap()
            }),
            ("staged", &|t| {
                fs::write(t.path.join("file"), "staged\n").unwrap();
                git_in(&t.path, &["add", "file"]);
            }),
            ("assume-unchanged", &|t| {
                git_in(&t.path, &["update-index", "--assume-unchanged", "file"]);
                fs::write(t.path.join("file"), "only copy\n").unwrap();
            }),
            ("skip-worktree", &|t| {
                git_in(&t.path, &["update-index", "--skip-worktree", "file"]);
                fs::write(t.path.join("file"), "only copy\n").unwrap();
            }),
            ("worktree ref", &|t| {
                commit(&t.path, "kept");
                git_in(&t.path, &["update-ref", "refs/worktree/keep", "HEAD"]);
                git_in(&t.path, &["reset", "-q", "--hard", "HEAD~1"]);
                git_in(&t.path, &["reflog", "expire", "--expire=now", "--all"]);
            }),
            ("reflog only", &|t| {
                git_in(&t.path, &["checkout", "-q", "--detach"]);
                commit(&t.path, "detached");
                git_in(&t.path, &["checkout", "-q", &t.branch]);
            }),
            ("unpushed", &|t| commit(&t.path, "ahead")),
        ];
        for (name, setup) in cases {
            let (repo, tree) = repository();
            if *name != "no upstream" {
                repo.git(&["remote", "add", "origin", "/nonexistent"]);
                let remote = format!("refs/remotes/origin/{}", tree.branch);
                git_in(&repo.0, &["update-ref", &remote, "HEAD"]);
                let upstream = format!("--set-upstream-to=origin/{}", tree.branch);
                git_in(&tree.path, &["branch", &upstream]);
            }
            setup(&tree);
            let preview = inventory(&repo.0, std::slice::from_ref(&tree)).unwrap()[1]
                .preservation
                .refusal
                .clone();
            let identity = tree.identity.clone().unwrap();
            let removal = remove_managed_worktree(tree.clone(), &identity, false, |_| Ok(()));
            match (&preview, &removal) {
                (None, Ok(removed)) => assert_eq!(removed.stage, ManagedStage::Removed),
                (Some(preview), Err(removal)) => assert_eq!(preview, removal, "{name}"),
                _ => panic!("{name}: preview {preview:?}, removal {removal:?}"),
            }
            assert_eq!(preview.is_none(), *name == "published", "{name}");
            clean(&tree);
        }
    }
}
