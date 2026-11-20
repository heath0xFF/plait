use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const THREAD: &str = "12345678-1234-4234-8234-123456789abc";
const GENERATION: &str = "12345678-1234-4234-8234-123456789abd";
struct Fixture {
    base: PathBuf,
    root: PathBuf,
    store: PathBuf,
    identity: Identity,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "plait-output-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let root = base.join("repo");
        fs::create_dir(&root).unwrap();
        let store = base.join("store");
        fs::create_dir(&store).unwrap();
        fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "user.email", "test@example.invalid"]);
        fs::write(
            root.join(".gitignore"),
            "target/\nassets/\nnode_modules/\nunapproved/\n.env\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='test'\nversion='0.1.0'\n",
        )
        .unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "initial"]);
        for path in ["target", "assets", "unapproved"] {
            fs::create_dir(root.join(path)).unwrap();
            fs::write(root.join(path).join("file"), format!("{path} source")).unwrap();
        }
        fs::write(root.join(".env"), "SECRET").unwrap();
        let identity = Identity::read(&root).unwrap();
        Self {
            base,
            root,
            store,
            identity,
        }
    }
    fn approve(&self, paths: &[&str]) -> policy::Policy {
        let previous = policy::load(&self.store, &self.root, &self.identity).unwrap();
        policy::set(
            &self.store,
            &self.root,
            &self.identity,
            previous.revision,
            paths.iter().map(|p| p.to_string()).collect(),
        )
        .unwrap()
    }
    fn tree(&self) -> ManagedWorktree {
        git::worktree::create_managed_worktree(
            &git::worktree::worktree_parent(&self.root).unwrap(),
            THREAD,
            None,
            |_| Ok(()),
        )
        .unwrap()
    }
    fn seed(&self) -> io::Result<Outcome> {
        seed(&self.store, &self.root, &self.identity, GENERATION, None)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn approval_is_private_and_survives_tracked_configuration_edits() {
    let f = Fixture::new();
    let p = f.approve(&["assets"]);
    fs::write(
        f.root.join("Cargo.toml"),
        "[build_output]\napproved=['target','unapproved','.env']\n",
    )
    .unwrap();
    fs::write(
        f.root.join(".plait.json"),
        "{\"approved\":[\"unapproved\"]}",
    )
    .unwrap();
    assert_eq!(
        policy::load(&f.store, &f.root, &f.identity).unwrap().paths,
        p.paths
    );
    assert!(
        policy::set(
            &f.store,
            &f.root,
            &f.identity,
            p.revision - 1,
            vec!["target".into()]
        )
        .is_err()
    );
    fs::create_dir(f.root.join("assets/nested")).unwrap();
    fs::create_dir(f.root.join("assets/nested/.git")).unwrap();
    assert!(git::build_output::validate(&f.root, "assets").is_err());
    assert!(git::build_output::validate(&f.root, ".env").is_err());
    assert!(git::build_output::validate(&f.root, "../store").is_err());
    fs::remove_dir(f.root.join("assets/nested/.git")).unwrap();
    fs::write(f.root.join("assets/pyvenv.cfg"), "home = /old/python").unwrap();
    assert!(git::build_output::validate(&f.root, "assets").is_err());
}
#[test]
fn a_source_with_a_writer_is_never_seeded() {
    let f = Fixture::new();
    f.approve(&["target"]);
    let _run = lease::acquire_checkout(&f.root, None, LeaseClass::Writer).unwrap();
    assert!(f.seed().is_err());
    assert!(!seed_pointer(&f.store, &f.identity).unwrap().exists());
    let tree = f.tree();
    let outcome = transaction::prepare(&f.store, &tree, false).unwrap();
    assert!(!outcome.warm);
    assert!(outcome.reason.contains("no idle build seed"));
    assert!(!tree.path.join("target").exists());
}
#[cfg(target_os = "macos")]
#[test]
fn approved_custom_output_is_independent_and_unapproved_data_is_never_copied() {
    let f = Fixture::new();
    f.approve(&["assets", "target"]);
    assert!(f.seed().unwrap().warm);
    let _run = lease::acquire_checkout(&f.root, None, LeaseClass::Writer).unwrap();
    fs::write(f.root.join("assets/file"), "later run").unwrap();
    let tree = f.tree();
    let result = transaction::prepare(&f.store, &tree, false).unwrap();
    assert!(result.warm);
    assert_eq!(
        fs::read(tree.path.join("assets/file")).unwrap(),
        b"assets source"
    );
    for path in ["unapproved", ".env"] {
        assert!(!tree.path.join(path).exists());
    }
    fs::write(tree.path.join("assets/file"), "clone edit").unwrap();
    assert_eq!(fs::read(f.root.join("assets/file")).unwrap(), b"later run");
    assert_eq!(
        transaction::prepare(&f.store, &tree, false).unwrap(),
        result
    );
    let seed: Seed = policy::read(&seed_pointer(&f.store, &f.identity).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        fs::read(seed.path.join("0/file")).unwrap(),
        b"assets source"
    );
}
#[cfg(target_os = "macos")]
#[test]
fn pi_receives_the_private_target_and_other_output_stays_in_its_worktree() {
    let f = Fixture::new();
    f.approve(&["target", "assets"]);
    let cache = private_cache(&f.store, GENERATION).unwrap();
    fs::create_dir(cache.join("target")).unwrap();
    fs::write(cache.join("target/file"), "sandbox artifact").unwrap();
    seed(&f.store, &f.root, &f.identity, GENERATION, Some(GENERATION)).unwrap();
    let tree = f.tree();
    assert!(transaction::prepare(&f.store, &tree, true).unwrap().warm);
    assert!(!tree.path.join("target").exists());
    assert_eq!(
        fs::read(private_cache(&f.store, THREAD).unwrap().join("target/file")).unwrap(),
        b"sandbox artifact"
    );
    assert!(tree.path.join("assets/file").exists());
}
#[test]
fn interrupted_install_rolls_back_before_a_retry_and_preserves_adjacent_data() {
    let f = Fixture::new();
    f.approve(&["assets"]);
    let tree = f.tree();
    let staging = PathBuf::from(format!(".plait-build-{THREAD}"));
    let stage = tree.path.join(&staging);
    fs::create_dir(&stage).unwrap();
    fs::create_dir(tree.path.join("assets")).unwrap();
    fs::write(tree.path.join("assets/partial"), "partial").unwrap();
    fs::write(tree.path.join(".env"), "preserve").unwrap();
    let journal = serde_json::json!({"checkout":tree.path,"identity":tree.identity,"staging":staging,"staging_identity":Identity::read(&stage).unwrap(),"installed":[{"root":tree.path,"root_identity":tree.identity,"path":"assets","identity":Identity::read(&tree.path.join("assets")).unwrap()}],"outcome":null});
    let file = policy::folder(&f.store, "build-output-operations")
        .unwrap()
        .join(format!("{THREAD}.json"));
    policy::save(&file, &journal).unwrap();
    let result = transaction::prepare(&f.store, &tree, false).unwrap();
    assert!(!result.warm);
    assert!(result.reason.contains("interrupted"));
    assert!(!tree.path.join("assets").exists());
    assert!(!stage.exists());
    assert_eq!(fs::read(tree.path.join(".env")).unwrap(), b"preserve");
    assert_eq!(transaction::recover(&f.store, &tree).unwrap(), result);
}
#[test]
fn no_approval_starts_cold() {
    let f = Fixture::new();
    let tree = f.tree();
    let result = transaction::prepare(&f.store, &tree, false).unwrap();
    assert_eq!(
        result.reason,
        "No build directories are approved for this project."
    );
    assert!(!tree.path.join("target").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn revoked_seed_is_not_reused_and_a_published_pending_seed_survives_recovery() {
    let f = Fixture::new();
    f.approve(&["target"]);
    f.seed().unwrap();
    let pointer = seed_pointer(&f.store, &f.identity).unwrap();
    let old: Seed = policy::read(&pointer).unwrap().unwrap();
    let pending = pointer.parent().unwrap().join(format!(
        "{}.pending.json",
        policy::key(&f.identity).unwrap()
    ));
    policy::save(&pending, &old).unwrap();
    // A crash after publication but before pending-file removal must not delete
    // the published seed before the next refresh has its replacement ready.
    let next = "12345678-1234-4234-8234-123456789abe";
    seed(&f.store, &f.root, &f.identity, next, None).unwrap();
    let published: Seed = policy::read(&pointer).unwrap().unwrap();
    assert!(published.path.join("0/file").exists());
    assert!(!pending.exists());
    f.approve(&["assets"]);
    let tree = f.tree();
    let outcome = transaction::prepare(&f.store, &tree, false).unwrap();
    assert!(!outcome.warm);
    assert!(outcome.reason.contains("current approval"));
    assert!(!tree.path.join("target").exists());
    assert!(!tree.path.join("assets").exists());
}

#[test]
fn approval_can_be_revoked_when_another_approved_directory_is_missing() {
    let f = Fixture::new();
    f.approve(&["assets", "target"]);
    fs::remove_dir_all(f.root.join("target")).unwrap();
    assert_eq!(f.approve(&["target"]).paths, vec!["target"]);
    assert!(f.approve(&[]).paths.is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn a_linked_worktree_never_replaces_the_project_seed() {
    let f = Fixture::new();
    f.approve(&["assets", "target"]);
    f.seed().unwrap();
    let pointer = seed_pointer(&f.store, &f.identity).unwrap();
    let before: Seed = policy::read(&pointer).unwrap().unwrap();
    assert_eq!(before.directories.len(), 2);
    // A cold worktree whose run built only one of the approved directories.
    let tree = f.tree();
    fs::create_dir(tree.path.join("target")).unwrap();
    fs::write(tree.path.join("target/file"), "one thread's output").unwrap();
    let next = "12345678-1234-4234-8234-123456789abe";
    let outcome = seed(
        &f.store,
        &tree.path,
        tree.identity.as_ref().unwrap(),
        next,
        None,
    )
    .unwrap();
    assert_eq!(outcome.reason, MAIN_ONLY);
    let after: Seed = policy::read(&pointer).unwrap().unwrap();
    assert_eq!(after.path, before.path);
    assert_eq!(after.directories, before.directories);
    assert_eq!(
        fs::read(after.path.join("1/file")).unwrap(),
        b"target source"
    );
}
