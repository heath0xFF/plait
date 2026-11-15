use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "plait-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let repo = Self(path);
        repo.git(&["init", "-b", "main"]);
        repo.git(&["config", "user.name", "Plait Test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }
    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn write(&self, path: &str, text: impl AsRef<[u8]>) {
        fs::write(self.0.join(path), text).unwrap();
    }
    fn commit(&self) {
        self.git(&["add", "."]);
        self.git(&["commit", "-m", "fixture"]);
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(diff: &Diff) -> String {
    diff.lines
        .iter()
        .map(|line| &*line.text)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn porcelain_preserves_nul_delimited_names_and_rename_order() {
    let (_, changes) = parse_status(
        b"## main\0 M spaces and\nnewlines.rs\0R  destination\0source name\0?? -odd\0",
    )
    .unwrap();
    assert_eq!(changes.len(), 3);
    assert_eq!(changes[0].path, PathBuf::from("spaces and\nnewlines.rs"));
    assert_eq!(changes[1].path, PathBuf::from("destination"));
    assert_eq!(changes[1].original, Some(PathBuf::from("source name")));
    assert_eq!(changes[0].display_path(), "spaces and\\nnewlines.rs");
    assert!(parse_status(b"## main\0R  incomplete\0").is_err());
    assert!(parse_status(b"## main\0?? ../outside\0").is_err());
    assert!(parse_status(b"## main\0?? /outside\0").is_err());
}

#[test]
fn unborn_clean_and_non_repository_states() {
    let repo = Repo::new();
    let initial = status(&repo.0).unwrap();
    assert!(initial.branch.contains("main"));
    assert!(initial.changes.is_empty());
    repo.write("hello.txt", "hello\n");
    let snapshot = status(&repo.0).unwrap();
    assert_eq!(snapshot.changes[0].label(), "New");
    assert!(
        text(&diff(&snapshot.root, &snapshot.changes[0], DiffMode::Unstaged).unwrap())
            .contains("+hello")
    );
    repo.commit();
    assert!(status(&repo.0).unwrap().changes.is_empty());
    let outside = repo.0.join("not-a-repository");
    fs::create_dir(&outside).unwrap();
    assert!(status(&repo.0.join("missing")).is_err());
    assert!(status(Path::new("/dev/null")).is_err());
}

#[test]
fn staged_and_unstaged_diffs_are_separate_and_reads_preserve_index() {
    let repo = Repo::new();
    repo.write("file.txt", "base\n");
    repo.commit();
    repo.write("file.txt", "staged\n");
    repo.git(&["add", "file.txt"]);
    repo.write("file.txt", "working\n");
    let index_before = fs::read(repo.0.join(".git/index")).unwrap();
    let snapshot = status(&repo.0).unwrap();
    let change = &snapshot.changes[0];
    assert!(change.staged() && change.unstaged());
    let staged = text(&diff(&snapshot.root, change, DiffMode::Staged).unwrap());
    let working = text(&diff(&snapshot.root, change, DiffMode::Unstaged).unwrap());
    assert!(staged.contains("-base") && staged.contains("+staged"));
    assert!(working.contains("-staged") && working.contains("+working"));
    assert!(!staged.contains("+working"));
    assert_eq!(index_before, fs::read(repo.0.join(".git/index")).unwrap());
    assert_eq!(
        fs::read_to_string(repo.0.join("file.txt")).unwrap(),
        "working\n"
    );
    assert!(!repo.0.join(".git/index.lock").exists());
}

#[test]
fn rename_deletion_binary_and_literal_pathspecs() {
    let repo = Repo::new();
    for path in ["old name", "deleted", ":(glob)*", "other"] {
        repo.write(path, "base\n");
    }
    repo.commit();
    repo.git(&["mv", "old name", "new name"]);
    fs::remove_file(repo.0.join("deleted")).unwrap();
    repo.write(":(glob)*", "literal\n");
    repo.write("other", "not selected\n");
    repo.write("binary", [0, 1, 2, 3]);
    let snapshot = status(&repo.0).unwrap();
    let rename = snapshot
        .changes
        .iter()
        .find(|c| c.path == Path::new("new name"))
        .unwrap();
    assert_eq!(rename.label(), "Renamed");
    assert!(text(&diff(&snapshot.root, rename, DiffMode::Staged).unwrap()).contains("rename from"));
    let deleted = snapshot
        .changes
        .iter()
        .find(|c| c.path == Path::new("deleted"))
        .unwrap();
    assert!(text(&diff(&snapshot.root, deleted, DiffMode::Unstaged).unwrap()).contains("-base"));
    let literal = snapshot
        .changes
        .iter()
        .find(|c| c.path == Path::new(":(glob)*"))
        .unwrap();
    let patch = text(&diff(&snapshot.root, literal, DiffMode::Unstaged).unwrap());
    assert!(patch.contains("+literal") && !patch.contains("not selected"));
    let binary = snapshot
        .changes
        .iter()
        .find(|c| c.path == Path::new("binary"))
        .unwrap();
    assert!(text(&diff(&snapshot.root, binary, DiffMode::Unstaged).unwrap()).contains("Binary"));
}

#[test]
fn symlink_and_non_utf8_names_do_not_escape_or_change_identity() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let repo = Repo::new();
    symlink("/etc/passwd", repo.0.join("link")).unwrap();
    let raw = OsStr::from_bytes(b"non-utf8-\xff");
    let (_, parsed) = parse_status(b"## main\0?? non-utf8-\xff\0").unwrap();
    assert_eq!(parsed[0].path.as_os_str(), raw);
    let snapshot = status(&repo.0).unwrap();
    let link = snapshot
        .changes
        .iter()
        .find(|c| c.path == Path::new("link"))
        .unwrap();
    let preview = text(&diff(&snapshot.root, link, DiffMode::Unstaged).unwrap());
    assert!(preview.contains("not followed") && !preview.contains("root:"));
}

#[test]
fn external_diff_and_textconv_are_never_executed() {
    let repo = Repo::new();
    repo.write("file.txt", "base\n");
    repo.write(".gitattributes", "*.txt diff=unsafe\n");
    repo.commit();
    repo.write("file.txt", "changed\n");
    repo.git(&["config", "diff.external", "false"]);
    repo.git(&["config", "diff.unsafe.textconv", "false"]);
    let snapshot = status(&repo.0).unwrap();
    assert!(
        text(&diff(&snapshot.root, &snapshot.changes[0], DiffMode::Unstaged).unwrap())
            .contains("+changed")
    );
}

#[test]
fn configured_clean_filters_are_not_executed() {
    let repo = Repo::new();
    repo.write("file.txt", "base\n");
    repo.write(".gitattributes", "*.txt filter=unsafe\n");
    repo.commit();
    repo.git(&["config", "filter.unsafe.clean", "false"]);
    repo.git(&["config", "filter.unsafe.process", "false"]);
    repo.git(&["config", "filter.unsafe.required", "true"]);
    repo.write("file.txt", "changed\n");
    let snapshot = status(&repo.0).unwrap();
    let preview = diff(&snapshot.root, &snapshot.changes[0], DiffMode::Unstaged).unwrap();
    assert!(text(&preview).contains("+changed"));
}

#[test]
fn patch_line_numbers_and_empty_untracked_files() {
    let patch = format_diff(
        b"diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -5,2 +8,2 @@\n old\n-gone\n+++literal pluses\n",
        false,
        Duration::ZERO,
    );
    assert_eq!(patch.lines[4].old_line, Some(5));
    assert_eq!(patch.lines[4].new_line, Some(8));
    assert_eq!(patch.lines[5].old_line, Some(6));
    assert_eq!(patch.lines[5].new_line, None);
    assert_eq!(patch.lines[6].new_line, Some(9));
    assert_eq!(patch.lines[6].kind, DiffKind::Added);
    assert_eq!(patch.widest_line, 0);
    let repo = Repo::new();
    repo.write("empty", "");
    let snapshot = status(&repo.0).unwrap();
    let change = &snapshot.changes[0];
    assert!(
        text(&diff(&snapshot.root, change, DiffMode::Unstaged).unwrap())
            .contains("Empty untracked")
    );
    assert!(
        text(&diff(&snapshot.root, change, DiffMode::Staged).unwrap())
            .contains("no staged version")
    );
}

#[test]
fn preview_rejects_parent_symlinks_and_special_files() {
    use std::os::unix::fs::symlink;
    let repo = Repo::new();
    let outside = Repo::new();
    outside.write("secret", "must not read");
    symlink(&outside.0, repo.0.join("outside")).unwrap();
    assert!(open_regular_file(&repo.0, Path::new("outside/secret")).is_err());
    assert!(open_regular_file(&repo.0, Path::new("../secret")).is_err());
    assert!(open_regular_file(&repo.0, Path::new(".git")).is_err());
    repo.write("ordinary", "safe");
    assert!(open_regular_file(&repo.0, Path::new("ordinary")).is_ok());
}

#[test]
fn detached_head_and_merge_conflict_remain_readable() {
    let repo = Repo::new();
    repo.write("file", "base\n");
    repo.commit();
    repo.git(&["checkout", "-b", "other"]);
    repo.write("file", "other\n");
    repo.commit();
    repo.git(&["checkout", "main"]);
    repo.write("file", "main\n");
    repo.commit();
    let merge = Command::new("git")
        .arg("-C")
        .arg(&repo.0)
        .args(["merge", "other"])
        .output()
        .unwrap();
    assert!(!merge.status.success());
    let snapshot = status(&repo.0).unwrap();
    assert_eq!(snapshot.changes[0].label(), "Conflict");
    assert!(
        text(&diff(&snapshot.root, &snapshot.changes[0], DiffMode::Unstaged).unwrap())
            .contains("<<<<<<<")
    );
    repo.git(&["merge", "--abort"]);
    repo.git(&["checkout", "--detach"]);
    assert!(status(&repo.0).unwrap().branch.contains("HEAD"));
}

#[test]
fn subprocess_deadline_reaps_the_child() {
    let start = Instant::now();
    let error = execute(
        Command::new("sleep").arg("5"),
        100,
        Duration::from_millis(30),
    )
    .err()
    .unwrap();
    assert!(error.contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn output_reader_drains_but_does_not_retain_unbounded_data() {
    let out = drain(&b"0123456789"[..], 4).unwrap();
    assert_eq!(out.bytes, b"0123");
    assert!(out.truncated);
}

#[test]
fn large_previews_and_long_lines_are_bounded() {
    let repo = Repo::new();
    repo.write("large", "a\n".repeat(DIFF_LIMIT));
    let snapshot = status(&repo.0).unwrap();
    let preview = diff(&snapshot.root, &snapshot.changes[0], DiffMode::Unstaged).unwrap();
    assert!(preview.truncated);
    assert_eq!(preview.lines.len(), 10_000);
    let preview = format_diff("x".repeat(9000).as_bytes(), false, Duration::ZERO);
    assert!(preview.lines[0].text.ends_with("[line clipped]"));
    assert!(preview.lines[0].text.len() < 2100);
}
