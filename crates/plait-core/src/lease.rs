//! Authoritative checkout lifecycle, writer, and shared-repository metadata leases.
//!
//! All Plait writers (agent runs, turn undo, desktop Git writes, and worktree
//! mutations) acquire OS-level `flock` leases in one strict order so deadlocks
//! cannot occur:
//!
//! 1. Checkout lifecycle lock (`checkout-<dev>-<ino>.lock` in a private per-user
//!    directory independent of host store): `LOCK_SH` for runs/readers, `LOCK_EX`
//!    for exclusive checkout mutations and worktree removal.
//! 2. Checkout writer lock (the canonical checkout root directory inode itself):
//!    `LOCK_EX` for writers and exclusive mutations; omitted for readers so a
//!    future strict reader can coexist with a writer while still blocking root
//!    removal and desktop Git mutations. Preserves interoperability with older
//!    hosts that only lock the directory inode.
//! 3. Common Git directory metadata lock (`common-<dev>-<ino>.lock` in the same
//!    per-user directory): `LOCK_EX` around operations that mutate shared refs,
//!    config, or the worktree registry across linked worktrees.

use std::{
    ffi::{CStr, CString, OsStr},
    fmt,
    fs::{self, DirBuilder, File},
    io,
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
    ptr, thread,
    time::{Duration, Instant},
};

/// User-facing refusal when an active run, reader, or another Plait write holds a checkout.
pub const CHECKOUT_BUSY: &str = "This checkout is in use by an active run or another Plait write. Wait until it finishes, then try again.";

/// User-facing refusal when a writer run cannot take the writer inode lock.
pub const WRITER_BUSY: &str = "This checkout already has a host writer.";

/// User-facing refusal when an exclusive checkout mutation blocks a new run or reader.
pub const LIFECYCLE_BUSY: &str =
    "This checkout is busy with another Plait operation. Wait until it finishes.";

/// User-facing refusal when shared Git metadata (refs/worktrees) stays busy past the wait budget.
pub const COMMON_METADATA_BUSY: &str = "Another Plait operation is updating this repository's shared Git metadata. Wait until it finishes, then try again.";

/// Default bounded wait when serializing shared-repository metadata across linked worktrees.
pub const COMMON_METADATA_WAIT: Duration = Duration::from_secs(5);

/// Maximum wait accepted by CLI callers for the shared metadata lock.
pub const MAX_COMMON_METADATA_WAIT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DirIdentity {
    pub dev: u64,
    pub ino: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseClass {
    /// Future strict reader: holds lifecycle-shared only.
    Reader,
    /// Writing run: holds lifecycle-shared and checkout directory writer-exclusive.
    Writer,
    /// Desktop Git write, turn undo, or worktree removal: holds lifecycle-exclusive
    /// and checkout directory writer-exclusive.
    Exclusive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseErrorKind {
    CheckoutBusy,
    CommonMetadataBusy,
    IdentityChanged,
    UnsafeLeaseStore,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseError {
    pub kind: LeaseErrorKind,
    pub message: String,
}

impl LeaseError {
    pub(crate) fn new(kind: LeaseErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn checkout_busy(message: impl Into<String>) -> Self {
        Self::new(LeaseErrorKind::CheckoutBusy, message)
    }

    pub fn common_busy() -> Self {
        Self::new(LeaseErrorKind::CommonMetadataBusy, COMMON_METADATA_BUSY)
    }

    pub fn identity(message: impl Into<String>) -> Self {
        Self::new(LeaseErrorKind::IdentityChanged, message)
    }

    pub fn unsafe_store(message: impl Into<String>) -> Self {
        Self::new(LeaseErrorKind::UnsafeLeaseStore, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new(LeaseErrorKind::Io, message)
    }
}

impl fmt::Display for LeaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LeaseError {}

impl From<LeaseError> for String {
    fn from(error: LeaseError) -> Self {
        error.message
    }
}

/// Verified lock-file metadata returned to `plait-host` callers before acquiring `flock`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedLock {
    pub root: PathBuf,
    pub identity: DirIdentity,
    pub lock_path: PathBuf,
    pub lock_identity: DirIdentity,
}

/// Held checkout lease (and optional Level 3 shared-metadata lock).
/// Dropping releases all held `flock` descriptors in reverse order.
#[derive(Debug)]
pub struct CheckoutLease {
    class: LeaseClass,
    root: PathBuf,
    identity: DirIdentity,
    common: Option<File>,
    writer: Option<File>,
    lifecycle: File,
}

impl CheckoutLease {
    pub fn class(&self) -> LeaseClass {
        self.class
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn identity(&self) -> DirIdentity {
        self.identity
    }

    /// Acquire the Level 3 shared Git common-directory metadata lock after
    /// Level 1 (and Level 2, if applicable) are already held.
    pub fn with_common_metadata(
        self,
        common_dir: &Path,
        wait: Duration,
    ) -> Result<Self, LeaseError> {
        let lease_dir = default_lease_dir()?;
        self.with_common_metadata_in(&lease_dir, common_dir, wait)
    }

    pub(crate) fn with_common_metadata_in(
        mut self,
        lease_dir: &Path,
        common_dir: &Path,
        wait: Duration,
    ) -> Result<Self, LeaseError> {
        let common = acquire_common_file_in(lease_dir, common_dir, wait)?;
        self.common = Some(common);
        Ok(self)
    }
}

fn unlock_fd(fd: i32) {
    // Explicit LOCK_UN releases the lock immediately even if a sibling thread in
    // this multithreaded process is mid-fork with a transient CLOEXEC copy.
    unsafe {
        libc::flock(fd, libc::LOCK_UN);
    }
}

impl Drop for CheckoutLease {
    fn drop(&mut self) {
        // Release in reverse acquisition order: Level 3, Level 2, Level 1.
        if let Some(common) = self.common.take() {
            unlock_fd(common.as_raw_fd());
        }
        if let Some(writer) = self.writer.take() {
            unlock_fd(writer.as_raw_fd());
        }
        unlock_fd(self.lifecycle.as_raw_fd());
    }
}

/// Standalone Level 3 common-directory metadata lease for host-side metadata operations.
#[derive(Debug)]
pub struct CommonMetadataLease {
    common_dir: PathBuf,
    identity: DirIdentity,
    file: File,
}

impl CommonMetadataLease {
    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    pub fn identity(&self) -> DirIdentity {
        self.identity
    }
}

impl Drop for CommonMetadataLease {
    fn drop(&mut self) {
        unlock_fd(self.file.as_raw_fd());
    }
}

/// Canonical checkout root and common Git directory resolved by hardened Git.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitCheckoutPaths {
    pub root: PathBuf,
    pub common_dir: PathBuf,
}

/// Per-user lease directory independent of `PLAIT_HOST_STORE` and `TMPDIR`.
pub fn default_lease_dir() -> Result<PathBuf, LeaseError> {
    if let Some(path) = std::env::var_os("PLAIT_LEASE_DIR") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(LeaseError::unsafe_store(
                "PLAIT_LEASE_DIR must be an absolute path.",
            ));
        }
        return Ok(path);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(account_home)
        .ok_or_else(|| {
            LeaseError::unsafe_store("Cannot determine a home directory for Plait checkout leases.")
        })?;
    Ok(home.join(".local/share/plait-leases"))
}

fn account_home() -> Option<PathBuf> {
    let uid = unsafe { libc::geteuid() };
    let mut record = MaybeUninit::<libc::passwd>::uninit();
    let mut found = ptr::null_mut();
    let mut buffer = vec![0u8; 65536];
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if status != 0 || found.is_null() {
        return None;
    }
    let record = unsafe { record.assume_init() };
    if record.pw_dir.is_null() {
        return None;
    }
    let dir = unsafe { CStr::from_ptr(record.pw_dir) };
    let path = PathBuf::from(OsStr::from_bytes(dir.to_bytes()));
    path.is_absolute().then_some(path)
}

pub fn checkout_lock_name(id: DirIdentity) -> String {
    format!("checkout-{:016x}-{:016x}.lock", id.dev, id.ino)
}

pub fn common_lock_name(id: DirIdentity) -> String {
    format!("common-{:016x}-{:016x}.lock", id.dev, id.ino)
}

/// Resolve a directory to its canonical path, open it without following symlinks,
/// and return its verified `(dev, ino)` identity.
pub fn open_verified_dir(path: &Path) -> Result<(PathBuf, File, DirIdentity), LeaseError> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| LeaseError::io(format!("Cannot resolve checkout: {error}")))?;
    let file = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(&canonical)
        .map_err(|error| LeaseError::identity(format!("Checkout is not a directory: {error}")))?;
    let meta = file
        .metadata()
        .map_err(|error| LeaseError::io(format!("Cannot inspect checkout: {error}")))?;
    if !meta.is_dir() {
        return Err(LeaseError::identity("Checkout is not a directory."));
    }
    let identity = DirIdentity {
        dev: meta.dev(),
        ino: meta.ino(),
    };
    verify_dir_still(&canonical, identity)?;
    Ok((canonical, file, identity))
}

/// Verify that `canonical` is still the same real directory (not replaced or turned into a symlink).
pub fn verify_dir_still(canonical: &Path, expected: DirIdentity) -> Result<(), LeaseError> {
    let meta = fs::symlink_metadata(canonical)
        .map_err(|_| LeaseError::identity("Checkout identity changed."))?;
    if !meta.is_dir()
        || meta.dev() != expected.dev
        || meta.ino() != expected.ino
        || fs::canonicalize(canonical).ok().as_deref() != Some(canonical)
    {
        return Err(LeaseError::identity("Checkout identity changed."));
    }
    Ok(())
}

/// Open and validate the private `0700` per-user lease directory.
pub fn open_lease_dir(path: &Path) -> Result<(PathBuf, File), LeaseError> {
    if !path.is_absolute() {
        return Err(LeaseError::unsafe_store(
            "Lease directory must be an absolute path.",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            LeaseError::unsafe_store(format!("Cannot create lease parent directory: {error}"))
        })?;
    }
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(LeaseError::unsafe_store(format!(
                "Cannot create lease directory: {error}"
            )));
        }
    }
    let dir = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            LeaseError::unsafe_store(format!("Lease directory must be a real directory: {error}"))
        })?;
    let meta = dir
        .metadata()
        .map_err(|error| LeaseError::unsafe_store(format!("Cannot inspect lease dir: {error}")))?;
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(LeaseError::unsafe_store(
            "Lease directory must be an owner-only (0700) directory owned by this user.",
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|error| {
        LeaseError::unsafe_store(format!("Cannot resolve lease directory: {error}"))
    })?;
    let check = fs::symlink_metadata(&canonical).map_err(|error| {
        LeaseError::unsafe_store(format!("Cannot inspect lease directory: {error}"))
    })?;
    if !check.is_dir() || check.dev() != meta.dev() || check.ino() != meta.ino() {
        return Err(LeaseError::unsafe_store(
            "Lease directory changed while opening.",
        ));
    }
    Ok((canonical, dir))
}

/// Open or create a private `0600` regular lock file inside an already-verified lease directory.
pub fn open_lock_file(lease_dir: &File, name: &str) -> Result<(File, DirIdentity), LeaseError> {
    if name.is_empty() || name.contains('/') || name.contains('\0') {
        return Err(LeaseError::unsafe_store("Invalid lock file name."));
    }
    let c_name =
        CString::new(name).map_err(|_| LeaseError::unsafe_store("Invalid lock file name."))?;
    let flags = libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mode: libc::c_uint = 0o600;
    let fd = unsafe { libc::openat(lease_dir.as_raw_fd(), c_name.as_ptr(), flags, mode) };
    if fd < 0 {
        return Err(LeaseError::unsafe_store(format!(
            "Cannot open lease lock file: {}",
            io::Error::last_os_error()
        )));
    }
    // SAFETY: `openat` returned a newly owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    let identity = verify_lock_metadata(&file)?;
    Ok((file, identity))
}

/// Verify that an open lock descriptor is an owned, private, single-link regular file.
pub fn verify_lock_metadata(file: &File) -> Result<DirIdentity, LeaseError> {
    let meta = file
        .metadata()
        .map_err(|error| LeaseError::unsafe_store(format!("Cannot inspect lock file: {error}")))?;
    let uid = unsafe { libc::geteuid() };
    if !meta.is_file() || meta.nlink() != 1 || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(LeaseError::unsafe_store(
            "Lease lock file must be a private (0600), single-link file owned by this user.",
        ));
    }
    Ok(DirIdentity {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// Non-blocking `flock` on `fd`. Returns `Ok(true)` if acquired, `Ok(false)` on contention.
pub fn try_flock_fd(fd: i32, operation: i32) -> Result<bool, LeaseError> {
    loop {
        if unsafe { libc::flock(fd, operation | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if matches!(
            error.raw_os_error(),
            Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
        ) {
            return Ok(false);
        }
        return Err(LeaseError::io(format!("Cannot lock checkout: {error}")));
    }
}

/// Bounded-wait `flock` on `fd` using non-blocking polls so timeouts are thread-safe and portable.
pub fn flock_fd_wait(fd: i32, operation: i32, wait: Duration) -> Result<bool, LeaseError> {
    if try_flock_fd(fd, operation)? {
        return Ok(true);
    }
    if wait.is_zero() {
        return Ok(false);
    }
    let deadline = Instant::now() + wait;
    let mut step = Duration::from_millis(5);
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(false);
        };
        thread::sleep(step.min(remaining));
        if try_flock_fd(fd, operation)? {
            return Ok(true);
        }
        step = (step * 2).min(Duration::from_millis(25));
    }
}

/// Prepare and verify the checkout lifecycle lock file for `path`.
pub fn prepare_checkout_lock(path: &Path) -> Result<PreparedLock, LeaseError> {
    let lease_dir = default_lease_dir()?;
    prepare_checkout_lock_in(&lease_dir, path)
}

pub(crate) fn prepare_checkout_lock_in(
    lease_dir: &Path,
    path: &Path,
) -> Result<PreparedLock, LeaseError> {
    let (root, _dir, identity) = open_verified_dir(path)?;
    let (canonical_lease_dir, lease_dir_fd) = open_lease_dir(lease_dir)?;
    let name = checkout_lock_name(identity);
    let (_file, lock_identity) = open_lock_file(&lease_dir_fd, &name)?;
    Ok(PreparedLock {
        root,
        identity,
        lock_path: canonical_lease_dir.join(name),
        lock_identity,
    })
}

/// Prepare and verify the common Git directory metadata lock file for `path`.
/// Accepts either a checkout path (resolving `--git-common-dir`) or a Git directory directly.
pub fn prepare_common_lock(path: &Path) -> Result<PreparedLock, LeaseError> {
    let lease_dir = default_lease_dir()?;
    prepare_common_lock_in(&lease_dir, path)
}

pub(crate) fn prepare_common_lock_in(
    lease_dir: &Path,
    path: &Path,
) -> Result<PreparedLock, LeaseError> {
    let common = resolve_common_dir(path)?;
    let (root, _dir, identity) = open_verified_dir(&common)?;
    let (canonical_lease_dir, lease_dir_fd) = open_lease_dir(lease_dir)?;
    let name = common_lock_name(identity);
    let (_file, lock_identity) = open_lock_file(&lease_dir_fd, &name)?;
    Ok(PreparedLock {
        root,
        identity,
        lock_path: canonical_lease_dir.join(name),
        lock_identity,
    })
}

/// Acquire a checkout lease (Level 1 lifecycle, plus Level 2 writer inode for Writer/Exclusive).
pub fn acquire_checkout(
    root: &Path,
    expected: Option<DirIdentity>,
    class: LeaseClass,
) -> Result<CheckoutLease, LeaseError> {
    let lease_dir = default_lease_dir()?;
    acquire_checkout_in(&lease_dir, root, expected, class)
}

pub(crate) fn acquire_checkout_in(
    lease_dir: &Path,
    root: &Path,
    expected: Option<DirIdentity>,
    class: LeaseClass,
) -> Result<CheckoutLease, LeaseError> {
    let (canonical, dir_file, identity) = open_verified_dir(root)?;
    if let Some(expected) = expected
        && expected != identity
    {
        return Err(LeaseError::identity("Checkout identity changed."));
    }
    let (_lease_dir_path, lease_dir_fd) = open_lease_dir(lease_dir)?;
    let name = checkout_lock_name(identity);
    let (lifecycle, _) = open_lock_file(&lease_dir_fd, &name)?;

    // Level 1: checkout lifecycle lock.
    let lifecycle_op = match class {
        LeaseClass::Reader | LeaseClass::Writer => libc::LOCK_SH,
        LeaseClass::Exclusive => libc::LOCK_EX,
    };
    if !try_flock_fd(lifecycle.as_raw_fd(), lifecycle_op)? {
        let message = match class {
            LeaseClass::Reader | LeaseClass::Writer => LIFECYCLE_BUSY,
            LeaseClass::Exclusive => CHECKOUT_BUSY,
        };
        return Err(LeaseError::checkout_busy(message));
    }

    // Level 2: checkout directory inode writer lock (omitted for readers).
    let writer = if matches!(class, LeaseClass::Writer | LeaseClass::Exclusive) {
        if !try_flock_fd(dir_file.as_raw_fd(), libc::LOCK_EX)? {
            unlock_fd(lifecycle.as_raw_fd());
            let message = match class {
                LeaseClass::Writer => WRITER_BUSY,
                LeaseClass::Exclusive => CHECKOUT_BUSY,
                LeaseClass::Reader => unreachable!(),
            };
            return Err(LeaseError::checkout_busy(message));
        }
        Some(dir_file)
    } else {
        None
    };

    let lease = CheckoutLease {
        class,
        root: canonical,
        identity,
        common: None,
        writer,
        lifecycle,
    };
    verify_dir_still(&lease.root, identity)?;
    Ok(lease)
}

/// Acquire a standalone Level 3 common-directory metadata lock.
pub fn acquire_common_metadata(
    path: &Path,
    wait: Duration,
) -> Result<CommonMetadataLease, LeaseError> {
    let lease_dir = default_lease_dir()?;
    acquire_common_metadata_in(&lease_dir, path, wait)
}

pub(crate) fn acquire_common_metadata_in(
    lease_dir: &Path,
    path: &Path,
    wait: Duration,
) -> Result<CommonMetadataLease, LeaseError> {
    let common = resolve_common_dir(path)?;
    let (canonical, _dir, identity) = open_verified_dir(&common)?;
    let file = acquire_common_file_in(lease_dir, &canonical, wait)?;
    Ok(CommonMetadataLease {
        common_dir: canonical,
        identity,
        file,
    })
}

fn acquire_common_file_in(
    lease_dir: &Path,
    common_dir: &Path,
    wait: Duration,
) -> Result<File, LeaseError> {
    let (canonical, _dir, identity) = open_verified_dir(common_dir)?;
    let (_lease_dir_path, lease_dir_fd) = open_lease_dir(lease_dir)?;
    let name = common_lock_name(identity);
    let (file, _) = open_lock_file(&lease_dir_fd, &name)?;
    if !flock_fd_wait(file.as_raw_fd(), libc::LOCK_EX, wait)? {
        return Err(LeaseError::common_busy());
    }
    if let Err(error) = verify_dir_still(&canonical, identity) {
        unlock_fd(file.as_raw_fd());
        return Err(error);
    }
    Ok(file)
}

/// Resolve a Git checkout's canonical working-tree root and common Git directory
/// using hardened read-only Git flags.
pub fn resolve_git_checkout(path: &Path) -> Result<GitCheckoutPaths, LeaseError> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| LeaseError::io(format!("Cannot resolve checkout: {error}")))?;
    let mut command = crate::git::base_command(&canonical);
    command.args([
        "-c",
        "core.hooksPath=/dev/null",
        "rev-parse",
        "--path-format=absolute",
        "--show-toplevel",
        "--git-common-dir",
    ]);
    let raw = crate::git::run_readonly(&mut command).map_err(LeaseError::io)?;
    let text = String::from_utf8(raw)
        .map_err(|_| LeaseError::io("Git returned non-UTF-8 checkout paths."))?;
    let lines: Vec<&str> = text.lines().collect();
    let [root, common_dir] = lines[..] else {
        return Err(LeaseError::io("Git did not report the checkout roots."));
    };
    let root = fs::canonicalize(Path::new(root))
        .map_err(|error| LeaseError::io(format!("Cannot resolve checkout root: {error}")))?;
    let common_dir = fs::canonicalize(Path::new(common_dir))
        .map_err(|error| LeaseError::io(format!("Cannot resolve Git common directory: {error}")))?;
    Ok(GitCheckoutPaths { root, common_dir })
}

/// Resolve `--git-common-dir` for either a working tree or a Git directory itself.
pub fn resolve_common_dir(path: &Path) -> Result<PathBuf, LeaseError> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| LeaseError::io(format!("Cannot resolve repository: {error}")))?;
    let mut command = crate::git::base_command(&canonical);
    command.args([
        "-c",
        "core.hooksPath=/dev/null",
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ]);
    let raw = crate::git::run_readonly(&mut command).map_err(LeaseError::io)?;
    let line = String::from_utf8(raw)
        .map_err(|_| LeaseError::io("Git returned a non-UTF-8 common directory path."))?;
    let common = line.trim();
    if common.is_empty() {
        return Err(LeaseError::io(
            "Git did not report the shared repository directory.",
        ));
    }
    fs::canonicalize(Path::new(common))
        .map_err(|error| LeaseError::io(format!("Cannot resolve Git common directory: {error}")))
}

/// Resolve a Git checkout to its canonical working-tree toplevel and acquire its
/// checkout lease (plus optional shared common-directory metadata lock).
pub fn acquire_git_checkout(
    path: &Path,
    expected: Option<DirIdentity>,
    class: LeaseClass,
    common_wait: Option<Duration>,
) -> Result<CheckoutLease, LeaseError> {
    let lease_dir = default_lease_dir()?;
    acquire_git_checkout_in(&lease_dir, path, expected, class, common_wait)
}

pub(crate) fn acquire_git_checkout_in(
    lease_dir: &Path,
    path: &Path,
    expected: Option<DirIdentity>,
    class: LeaseClass,
    common_wait: Option<Duration>,
) -> Result<CheckoutLease, LeaseError> {
    let paths = resolve_git_checkout(path)?;
    let lease = acquire_checkout_in(lease_dir, &paths.root, expected, class)?;
    match common_wait {
        Some(wait) => lease.with_common_metadata_in(lease_dir, &paths.common_dir, wait),
        None => Ok(lease),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "plait-lease-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.name", "Test"]);
        git(dir, &["config", "user.email", "test@example.invalid"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
        fs::write(dir.join("README.md"), "hello\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "init"]);
    }

    #[test]
    fn writer_excludes_writer_and_exclusive_but_coexists_with_reader() {
        let scratch = Scratch::new("matrix");
        let leases = scratch.0.join("leases");
        let root = scratch.0.join("repo");
        fs::create_dir(&root).unwrap();

        let writer = acquire_checkout_in(&leases, &root, None, LeaseClass::Writer).unwrap();
        // A second writer is refused.
        let second_writer =
            acquire_checkout_in(&leases, &root, None, LeaseClass::Writer).unwrap_err();
        assert_eq!(second_writer.kind, LeaseErrorKind::CheckoutBusy);
        assert_eq!(second_writer.message, WRITER_BUSY);
        // An exclusive mutation is refused while a writer holds the checkout.
        let exclusive =
            acquire_checkout_in(&leases, &root, None, LeaseClass::Exclusive).unwrap_err();
        assert_eq!(exclusive.kind, LeaseErrorKind::CheckoutBusy);
        // Fake reader class coexists with an active writer and another reader.
        let reader_a = acquire_checkout_in(&leases, &root, None, LeaseClass::Reader).unwrap();
        let reader_b = acquire_checkout_in(&leases, &root, None, LeaseClass::Reader).unwrap();

        // Dropping the writer still leaves readers blocking exclusive mutations.
        drop(writer);
        let exclusive_vs_reader =
            acquire_checkout_in(&leases, &root, None, LeaseClass::Exclusive).unwrap_err();
        assert_eq!(exclusive_vs_reader.kind, LeaseErrorKind::CheckoutBusy);

        // A new writer can still start while readers are active.
        let writer2 = acquire_checkout_in(&leases, &root, None, LeaseClass::Writer).unwrap();
        drop(writer2);
        drop(reader_a);
        drop(reader_b);

        // Once all writers and readers are gone, exclusive succeeds and blocks everyone else.
        let exclusive = acquire_checkout_in(&leases, &root, None, LeaseClass::Exclusive).unwrap();
        assert_eq!(
            acquire_checkout_in(&leases, &root, None, LeaseClass::Writer)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
        assert_eq!(
            acquire_checkout_in(&leases, &root, None, LeaseClass::Reader)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
        assert_eq!(
            acquire_checkout_in(&leases, &root, None, LeaseClass::Exclusive)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
        drop(exclusive);

        let after = acquire_checkout_in(&leases, &root, None, LeaseClass::Writer).unwrap();
        drop(after);
    }

    #[test]
    fn aliases_subdirectories_and_canonical_paths_share_one_lease() {
        let scratch = Scratch::new("aliases");
        let leases = scratch.0.join("leases");
        let repo = scratch.0.join("repo");
        init_repo(&repo);
        let subdir = repo.join("nested");
        fs::create_dir(&subdir).unwrap();
        let alias = scratch.0.join("alias");
        symlink(&repo, &alias).unwrap();

        let held = acquire_checkout_in(&leases, &repo, None, LeaseClass::Writer).unwrap();
        assert_eq!(
            acquire_checkout_in(&leases, &alias, None, LeaseClass::Writer)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
        assert_eq!(
            acquire_checkout_in(&leases, &alias, None, LeaseClass::Exclusive)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
        assert_eq!(
            acquire_git_checkout_in(&leases, &subdir, None, LeaseClass::Exclusive, None)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
        drop(held);

        let via_alias = acquire_checkout_in(&leases, &alias, None, LeaseClass::Exclusive).unwrap();
        assert_eq!(via_alias.root(), repo);
        assert_eq!(
            acquire_checkout_in(&leases, &repo, None, LeaseClass::Writer)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
    }

    #[test]
    fn replaced_directory_identity_is_rejected_without_relying_on_fresh_inode_reuse() {
        let scratch = Scratch::new("replaced");
        let leases = scratch.0.join("leases");
        let repo = scratch.0.join("repo");
        fs::create_dir(&repo).unwrap();
        let (_, _, original) = open_verified_dir(&repo).unwrap();

        // Rename away rather than remove so the original inode remains allocated on Linux too.
        let displaced = scratch.0.join("repo-old");
        fs::rename(&repo, &displaced).unwrap();
        fs::create_dir(&repo).unwrap();

        let err =
            acquire_checkout_in(&leases, &repo, Some(original), LeaseClass::Exclusive).unwrap_err();
        assert_eq!(err.kind, LeaseErrorKind::IdentityChanged);
    }

    #[test]
    fn unsafe_lease_directory_and_lock_files_are_rejected() {
        let scratch = Scratch::new("unsafe");
        let repo = scratch.0.join("repo");
        fs::create_dir(&repo).unwrap();

        // Symlink lease directory.
        let real_dir = scratch.0.join("real-leases");
        DirBuilder::new().mode(0o700).create(&real_dir).unwrap();
        let link_dir = scratch.0.join("link-leases");
        symlink(&real_dir, &link_dir).unwrap();
        assert_eq!(
            acquire_checkout_in(&link_dir, &repo, None, LeaseClass::Writer)
                .unwrap_err()
                .kind,
            LeaseErrorKind::UnsafeLeaseStore
        );

        // Group/world-accessible lease directory.
        let public_dir = scratch.0.join("public-leases");
        fs::create_dir(&public_dir).unwrap();
        fs::set_permissions(&public_dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            acquire_checkout_in(&public_dir, &repo, None, LeaseClass::Writer)
                .unwrap_err()
                .kind,
            LeaseErrorKind::UnsafeLeaseStore
        );

        // Symlink lock file inside an otherwise valid lease directory.
        let good_dir = scratch.0.join("good-leases");
        let (_, _, id) = open_verified_dir(&repo).unwrap();
        let (_, _) = open_lease_dir(&good_dir).unwrap();
        let target = scratch.0.join("outside.txt");
        fs::write(&target, "").unwrap();
        let lock_path = good_dir.join(checkout_lock_name(id));
        symlink(&target, &lock_path).unwrap();
        assert_eq!(
            acquire_checkout_in(&good_dir, &repo, None, LeaseClass::Writer)
                .unwrap_err()
                .kind,
            LeaseErrorKind::UnsafeLeaseStore
        );

        // Hard-linked lock file (`nlink > 1`). The macOS command sandbox used by
        // dogfood tests denies `link(2)` even in scratch, so skip only EPERM there;
        // `crates/plait-host/tests/record.rs` exercises hard-link rejection outside it.
        fs::remove_file(&lock_path).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        match fs::hard_link(&target, &lock_path) {
            Ok(()) => assert_eq!(
                acquire_checkout_in(&good_dir, &repo, None, LeaseClass::Writer)
                    .unwrap_err()
                    .kind,
                LeaseErrorKind::UnsafeLeaseStore
            ),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
            Err(error) => panic!("unexpected hard_link failure: {error}"),
        }
    }

    #[test]
    fn old_host_directory_flock_interoperates_with_new_leases() {
        let scratch = Scratch::new("interop");
        let leases = scratch.0.join("leases");
        let repo = scratch.0.join("repo");
        fs::create_dir(&repo).unwrap();

        // Simulate an older host holding only the directory inode flock.
        let old_dir = File::open(&repo).unwrap();
        assert!(try_flock_fd(old_dir.as_raw_fd(), libc::LOCK_EX).unwrap());
        assert_eq!(
            acquire_checkout_in(&leases, &repo, None, LeaseClass::Writer)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
        assert_eq!(
            acquire_checkout_in(&leases, &repo, None, LeaseClass::Exclusive)
                .unwrap_err()
                .kind,
            LeaseErrorKind::CheckoutBusy
        );
        unlock_fd(old_dir.as_raw_fd());
        drop(old_dir);

        // Now hold the new writer lease: an older host trying raw directory flock must fail.
        let new_writer = acquire_checkout_in(&leases, &repo, None, LeaseClass::Writer).unwrap();
        let probe = File::open(&repo).unwrap();
        assert!(!try_flock_fd(probe.as_raw_fd(), libc::LOCK_EX).unwrap());
        drop(new_writer);
        assert!(try_flock_fd(probe.as_raw_fd(), libc::LOCK_EX).unwrap());
    }

    #[test]
    fn common_metadata_lock_serializes_linked_worktrees_while_unrelated_repos_progress() {
        let scratch = Scratch::new("common");
        let leases = scratch.0.join("leases");
        let main = scratch.0.join("main");
        let unrelated = scratch.0.join("unrelated");
        init_repo(&main);
        init_repo(&unrelated);

        let wt = scratch.0.join("wt1");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "wt-branch",
                wt.to_str().unwrap(),
                "HEAD",
            ],
        );

        let held = acquire_git_checkout_in(
            &leases,
            &main,
            None,
            LeaseClass::Exclusive,
            Some(Duration::ZERO),
        )
        .unwrap();

        // Sibling worktree has its own checkout root, so checkout-level exclusive succeeds,
        // while shared common-dir metadata lock is immediately contended with zero wait.
        let wt_only =
            acquire_git_checkout_in(&leases, &wt, None, LeaseClass::Exclusive, None).unwrap();
        drop(wt_only);

        let busy = acquire_git_checkout_in(
            &leases,
            &wt,
            None,
            LeaseClass::Exclusive,
            Some(Duration::from_millis(30)),
        )
        .unwrap_err();
        assert_eq!(busy.kind, LeaseErrorKind::CommonMetadataBusy);

        // An unrelated repository makes progress immediately without contention.
        let other = acquire_git_checkout_in(
            &leases,
            &unrelated,
            None,
            LeaseClass::Exclusive,
            Some(Duration::ZERO),
        )
        .unwrap();
        drop(other);

        // Bounded wait serializes cleanly once `held` releases after a short delay.
        let leases_clone = leases.clone();
        let wt_clone = wt.clone();
        let started = Instant::now();
        let releaser = thread::spawn(move || {
            thread::sleep(Duration::from_millis(120));
            drop(held);
        });
        let acquired = acquire_git_checkout_in(
            &leases_clone,
            &wt_clone,
            None,
            LeaseClass::Exclusive,
            Some(Duration::from_secs(2)),
        )
        .unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(90),
            "sibling worktree did not wait for shared metadata lock"
        );
        drop(acquired);
        releaser.join().unwrap();
    }
}
