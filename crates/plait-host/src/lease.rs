use plait_core::lease::{
    CHECKOUT_BUSY, COMMON_METADATA_BUSY, DirIdentity, LIFECYCLE_BUSY, MAX_COMMON_METADATA_WAIT,
    WRITER_BUSY, checkout_lock_name, common_lock_name, default_lease_dir, flock_fd_wait,
    open_lease_dir, open_lock_file, open_verified_dir, prepare_checkout_lock, prepare_common_lock,
    try_flock_fd, verify_dir_still,
};
use serde_json::json;
use std::{
    fs::File,
    io::{self, Write},
    mem::MaybeUninit,
    os::fd::AsRawFd,
    path::Path,
    time::Duration,
};

/// Prepare and verify a lock file path for the runtime sidecar (`checkout` or `common`).
pub fn prepare(args: Vec<String>) -> io::Result<()> {
    if args.len() != 2 {
        return Err(io::Error::other(
            "lease-prepare expects <checkout|common> <path>",
        ));
    }
    let prepared = match args[0].as_str() {
        "checkout" => prepare_checkout_lock(Path::new(&args[1])),
        "common" => prepare_common_lock(Path::new(&args[1])),
        _ => return Err(io::Error::other("unknown lease-prepare kind")),
    }
    .map_err(|error| io::Error::other(error.to_string()))?;
    let mut out = io::stdout().lock();
    serde_json::to_writer(
        &mut out,
        &json!({
            "root": prepared.root,
            "dev": prepared.identity.dev.to_string(),
            "ino": prepared.identity.ino.to_string(),
            "lock_path": prepared.lock_path,
            "lock_dev": prepared.lock_identity.dev.to_string(),
            "lock_ino": prepared.lock_identity.ino.to_string(),
        }),
    )?;
    out.write_all(b"\n")
}

/// Acquire `flock` on open file descriptions inherited from the runtime sidecar.
///
/// With no arguments, preserves the legacy single-fd writer lock on fd 3.
/// With `<reader|writer|exclusive|common> <canonical_path> [timeout_ms]`,
/// verifies every inherited descriptor and acquires locks in strict Level 1 -> Level 2 order.
pub fn acquire(args: Vec<String>) -> io::Result<()> {
    if args.is_empty() {
        return acquire_legacy();
    }
    if args.len() < 2 || args.len() > 3 {
        return Err(io::Error::other(
            "lock-fd expects <reader|writer|exclusive|common> <path> [timeout_ms]",
        ));
    }
    let mode = args[0].as_str();
    let target = Path::new(&args[1]);
    let timeout_ms = args
        .get(2)
        .map(|raw| {
            raw.parse::<u64>()
                .map_err(|_| io::Error::other("invalid lease timeout"))
        })
        .transpose()?
        .unwrap_or(0);

    let (canonical, _dir, id) =
        open_verified_dir(target).map_err(|error| io::Error::other(error.to_string()))?;
    let lease_dir = default_lease_dir().map_err(|error| io::Error::other(error.to_string()))?;
    let (_lease_dir_path, lease_dir_fd) =
        open_lease_dir(&lease_dir).map_err(|error| io::Error::other(error.to_string()))?;
    let expected_name = if mode == "common" {
        common_lock_name(id)
    } else {
        checkout_lock_name(id)
    };
    let (_expected_file, expected_lock_id) = open_lock_file(&lease_dir_fd, &expected_name)
        .map_err(|error| io::Error::other(error.to_string()))?;
    verify_inherited_lock_fd(3, expected_lock_id)?;

    match mode {
        "reader" => {
            if !try_flock_fd(3, libc::LOCK_SH).map_err(|e| io::Error::other(e.to_string()))? {
                return Err(io::Error::other(format!("busy: {LIFECYCLE_BUSY}")));
            }
        }
        "writer" => {
            verify_inherited_dir_fd(4, id)?;
            if !try_flock_fd(3, libc::LOCK_SH).map_err(|e| io::Error::other(e.to_string()))? {
                return Err(io::Error::other(format!("busy: {LIFECYCLE_BUSY}")));
            }
            if !try_flock_fd(4, libc::LOCK_EX).map_err(|e| io::Error::other(e.to_string()))? {
                unsafe {
                    libc::flock(3, libc::LOCK_UN);
                }
                return Err(io::Error::other(format!("busy: {WRITER_BUSY}")));
            }
        }
        "exclusive" => {
            verify_inherited_dir_fd(4, id)?;
            if !try_flock_fd(3, libc::LOCK_EX).map_err(|e| io::Error::other(e.to_string()))? {
                return Err(io::Error::other(format!("busy: {CHECKOUT_BUSY}")));
            }
            if !try_flock_fd(4, libc::LOCK_EX).map_err(|e| io::Error::other(e.to_string()))? {
                unsafe {
                    libc::flock(3, libc::LOCK_UN);
                }
                return Err(io::Error::other(format!("busy: {CHECKOUT_BUSY}")));
            }
        }
        "common" => {
            let wait = Duration::from_millis(timeout_ms).min(MAX_COMMON_METADATA_WAIT);
            if !flock_fd_wait(3, libc::LOCK_EX, wait)
                .map_err(|e| io::Error::other(e.to_string()))?
            {
                return Err(io::Error::other(format!("busy: {COMMON_METADATA_BUSY}")));
            }
        }
        _ => return Err(io::Error::other("unknown lock-fd mode")),
    }

    if let Err(error) = verify_dir_still(&canonical, id) {
        if matches!(mode, "writer" | "exclusive") {
            unsafe {
                libc::flock(4, libc::LOCK_UN);
            }
        }
        unsafe {
            libc::flock(3, libc::LOCK_UN);
        }
        return Err(io::Error::other(format!("identity: {error}")));
    }
    Ok(())
}

fn acquire_legacy() -> io::Result<()> {
    // flock follows the shared open description, which Node retains after we exit.
    let stat = fstat_fd(3)?;
    #[allow(clippy::unnecessary_cast)]
    let file_type = (stat.st_mode as u32) & (libc::S_IFMT as u32);
    #[allow(clippy::unnecessary_cast)]
    if file_type != (libc::S_IFDIR as u32) {
        return Err(io::Error::other("invalid checkout lease"));
    }
    if !try_flock_fd(3, libc::LOCK_EX).map_err(|e| io::Error::other(e.to_string()))? {
        return Err(io::Error::other("checkout already has a writer"));
    }
    Ok(())
}

/// Guard held by `plait-host undo` across working-tree restoration.
/// Releases only descriptors it opened itself; inherited runtime leases stay held by the runtime.
pub struct UndoLeaseGuard {
    owned_writer: Option<File>,
    owned_lifecycle: Option<File>,
}

impl Drop for UndoLeaseGuard {
    fn drop(&mut self) {
        if let Some(writer) = self.owned_writer.take() {
            unsafe {
                libc::flock(writer.as_raw_fd(), libc::LOCK_UN);
            }
        }
        if let Some(lifecycle) = self.owned_lifecycle.take() {
            unsafe {
                libc::flock(lifecycle.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

/// Enforce Level 1 (`lifecycle EX`) and Level 2 (`writer EX`) for `plait-host undo`.
/// Reuses inherited fd 7 (lifecycle) and fd 4 (writer) when provided by the runtime,
/// and acquires any missing lease standalone.
pub fn acquire_or_verify_exclusive(
    root: &Path,
    expected: DirIdentity,
) -> io::Result<UndoLeaseGuard> {
    let (canonical, dir_file, actual) =
        open_verified_dir(root).map_err(|_| io::Error::other("Checkout identity changed"))?;
    if actual != expected {
        return Err(io::Error::other("Checkout identity changed"));
    }
    let lease_dir = default_lease_dir().map_err(|e| io::Error::other(e.to_string()))?;
    let (_lease_dir_path, lease_dir_fd) =
        open_lease_dir(&lease_dir).map_err(|e| io::Error::other(e.to_string()))?;
    let (lifecycle_file, expected_lock_id) =
        open_lock_file(&lease_dir_fd, &checkout_lock_name(expected))
            .map_err(|e| io::Error::other(e.to_string()))?;

    let mut guard = UndoLeaseGuard {
        owned_writer: None,
        owned_lifecycle: None,
    };

    // Level 1: lifecycle exclusive (reuse inherited fd 7 if it matches the lock file).
    if verify_inherited_lock_fd(7, expected_lock_id).is_ok() {
        if !inherited_exclusive(7, &lifecycle_file)? {
            return Err(io::Error::other(format!("busy: {CHECKOUT_BUSY}")));
        }
    } else {
        if !try_flock_fd(lifecycle_file.as_raw_fd(), libc::LOCK_EX)
            .map_err(|e| io::Error::other(e.to_string()))?
        {
            return Err(io::Error::other(format!("busy: {CHECKOUT_BUSY}")));
        }
        guard.owned_lifecycle = Some(lifecycle_file);
    }

    // Level 2: checkout directory inode writer exclusive (reuse inherited fd 4 if it matches).
    if verify_inherited_dir_fd(4, expected).is_ok() {
        if !try_flock_fd(4, libc::LOCK_EX).map_err(|e| io::Error::other(e.to_string()))? {
            return Err(io::Error::other("busy: checkout already has a writer"));
        }
    } else {
        if !try_flock_fd(dir_file.as_raw_fd(), libc::LOCK_EX)
            .map_err(|e| io::Error::other(e.to_string()))?
        {
            return Err(io::Error::other("busy: checkout already has a writer"));
        }
        guard.owned_writer = Some(dir_file);
    }

    verify_dir_still(&canonical, expected)
        .map_err(|_| io::Error::other("Checkout identity changed"))?;
    Ok(guard)
}

/// Whether the inherited lifecycle descriptor `fd` already holds `LOCK_EX`.
///
/// The inherited description belongs to the runtime, so it is never converted:
/// a shared lock upgraded here would stay exclusive after this helper exits, and
/// on Linux a refused conversion removes the shared lock first. `probe` is a
/// fresh description of the same lock file. It can take a shared lock only when
/// nobody holds an exclusive one; when it cannot, re-asserting `LOCK_EX` on `fd`
/// is a no-op if `fd` is that holder and a refusal otherwise.
fn inherited_exclusive(fd: i32, probe: &File) -> io::Result<bool> {
    let lock =
        |fd, operation| try_flock_fd(fd, operation).map_err(|e| io::Error::other(e.to_string()));
    if lock(probe.as_raw_fd(), libc::LOCK_SH)? {
        unsafe {
            libc::flock(probe.as_raw_fd(), libc::LOCK_UN);
        }
        return Ok(false);
    }
    lock(fd, libc::LOCK_EX)
}

fn fstat_fd(fd: i32) -> io::Result<libc::stat> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { stat.assume_init() })
}

fn verify_inherited_lock_fd(fd: i32, expected: DirIdentity) -> io::Result<()> {
    let stat = fstat_fd(fd)?;
    let uid = unsafe { libc::geteuid() };
    #[allow(clippy::unnecessary_cast)]
    let file_type = (stat.st_mode as u32) & (libc::S_IFMT as u32);
    #[allow(clippy::unnecessary_cast)]
    let perms = (stat.st_mode as u32) & 0o077;
    #[allow(clippy::unnecessary_cast)]
    let dev = stat.st_dev as u64;
    #[allow(clippy::unnecessary_cast)]
    let ino = stat.st_ino as u64;
    #[allow(clippy::unnecessary_cast)]
    let nlink = stat.st_nlink as u64;
    #[allow(clippy::unnecessary_cast)]
    if file_type != (libc::S_IFREG as u32)
        || nlink != 1
        || stat.st_uid != uid
        || perms != 0
        || dev != expected.dev
        || ino != expected.ino
    {
        return Err(io::Error::other("invalid checkout lifecycle lease"));
    }
    Ok(())
}

fn verify_inherited_dir_fd(fd: i32, expected: DirIdentity) -> io::Result<()> {
    let stat = fstat_fd(fd)?;
    #[allow(clippy::unnecessary_cast)]
    let file_type = (stat.st_mode as u32) & (libc::S_IFMT as u32);
    #[allow(clippy::unnecessary_cast)]
    let dev = stat.st_dev as u64;
    #[allow(clippy::unnecessary_cast)]
    let ino = stat.st_ino as u64;
    #[allow(clippy::unnecessary_cast)]
    if file_type != (libc::S_IFDIR as u32) || dev != expected.dev || ino != expected.ino {
        return Err(io::Error::other("invalid checkout writer lease"));
    }
    Ok(())
}
