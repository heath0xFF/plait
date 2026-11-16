//! Desktop recovery, separate from host conversations and model credentials.
//!
//! Open and flush only on a background executor. Queueing replaces an in-memory
//! snapshot; disk writes are serialized so an older save cannot replace newer work.

use crate::host::ModelChoice;
use crate::host_state::PendingSubmit;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const FILE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Draft {
    pub text: String,
    pub revision: u64,
    pub submitted: Option<(String, u64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Thread {
    pub checkout: PathBuf,
    pub model: ModelChoice,
    pub title: String,
    pub draft: Draft,
    pub pending: Option<PendingSubmit>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub schema_version: u32,
    pub host_store: PathBuf,
    pub checkout: Option<PathBuf>,
    pub selected: Option<String>,
    pub threads: BTreeMap<String, Thread>,
}

impl Session {
    pub fn saved_thread(&self, summary: &crate::host::ThreadSummary) -> Option<&Thread> {
        self.threads
            .get(&summary.id)
            .filter(|saved| saved.checkout == summary.checkout && saved.model == summary.model)
    }

    pub fn accepts(&self, summary: &crate::host::ThreadSummary) -> bool {
        !self.threads.contains_key(&summary.id) || self.saved_thread(summary).is_some()
    }

    pub fn update_thread(&mut self, id: String, thread: Thread) -> Result<(), String> {
        if self
            .threads
            .get(&id)
            .is_some_and(|saved| saved.checkout != thread.checkout || saved.model != thread.model)
        {
            return Err("Thread identity conflicts with saved work. Its draft and submission were not changed. Start a new thread or check the host store.".into());
        }
        self.threads.insert(id, thread);
        Ok(())
    }

    pub fn empty(host_store: PathBuf) -> Self {
        Self {
            schema_version: 1,
            host_store,
            checkout: None,
            selected: None,
            threads: BTreeMap::new(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Saved workspace uses an unsupported version. Update Plait; the file was not changed.".into());
        }
        if !self.host_store.is_absolute()
            || self
                .checkout
                .as_ref()
                .is_some_and(|path| !path.is_absolute())
            || self.threads.len() > 512
        {
            return Err("Saved workspace has invalid paths or exceeds its thread limit.".into());
        }
        for (id, thread) in &self.threads {
            if !identifier(id)
                || !thread.checkout.is_absolute()
                || thread.title.len() > 1024
                || thread.draft.text.len() > 256 * 1024
                || thread
                    .draft
                    .submitted
                    .as_ref()
                    .is_some_and(|(text, _)| text.len() > crate::host::REQUEST_LIMIT)
            {
                return Err("Saved thread or draft exceeds its limits.".into());
            }
            if let Some(pending) = &thread.pending {
                let request = crate::host::Request::Submit {
                    thread_id: id.clone(),
                    request_id: pending.request_id.clone(),
                    text: pending.text.clone(),
                };
                if !identifier(&pending.request_id)
                    || pending.text.trim().is_empty()
                    || serde_json::to_vec(&request)
                        .map_err(|e| e.to_string())?
                        .len()
                        + 1
                        > crate::host::REQUEST_LIMIT
                {
                    return Err("Saved submission is invalid. It was not sent.".into());
                }
            }
        }
        Ok(())
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

pub fn locations() -> Result<(PathBuf, PathBuf), String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("No absolute home directory is available.")?;
    let directory = match std::env::var_os("PLAIT_DESKTOP_STATE_DIR") {
        Some(path) => PathBuf::from(path),
        None => crate::preferences::default_path()
            .and_then(|path| path.parent().map(|p| p.join("session")))
            .ok_or("No desktop state directory is available.")?,
    };
    let host = std::env::var_os("PLAIT_HOST_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share/plait"));
    if !directory.is_absolute() || !host.is_absolute() {
        return Err("Desktop state and host store paths must be absolute.".into());
    }
    Ok((directory, host))
}

struct Lease(File);

impl Drop for Lease {
    fn drop(&mut self) {
        // A forked child can keep the file description alive until exec. Only the
        // journal owns this lease, unlike host leases shared with command guardians.
        if let Err(error) = file_lock(&self.0, libc::LOCK_UN) {
            eprintln!("Plait workspace lock release failed: {error}");
        }
    }
}

fn file_lock(file: &File, operation: libc::c_int) -> io::Result<()> {
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

pub struct Journal {
    directory: PathBuf,
    _lease: Lease,
    queued: Mutex<(u64, Arc<Session>)>,
    written: Mutex<u64>,
}

impl Journal {
    pub fn open(directory: &Path, host_store: PathBuf) -> Result<(Arc<Self>, Session), String> {
        let io = |e: std::io::Error| format!("Cannot open saved workspace: {e}");
        fs::create_dir_all(directory).map_err(io)?;
        let metadata = fs::symlink_metadata(directory).map_err(io)?;
        if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err("Desktop state must be an owned directory, not a symlink.".into());
        }
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(io)?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(directory.join("desktop.lock"))
            .map_err(io)?;
        let metadata = lease.metadata().map_err(io)?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err("Invalid desktop recovery lock.".into());
        }
        file_lock(&lease, libc::LOCK_EX | libc::LOCK_NB).map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                "Another Plait client owns this saved workspace. Close it before sending work here."
                    .into()
            } else {
                format!("Cannot lock saved workspace: {error}")
            }
        })?;
        // Acquire the guard before reading so failed recovery also releases ownership.
        let lease = Lease(lease);
        let path = directory.join("desktop.json");
        let session = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => {
                let metadata = file.metadata().map_err(io)?;
                if !metadata.is_file()
                    || metadata.nlink() != 1
                    || metadata.uid() != unsafe { libc::geteuid() }
                    || metadata.mode() & 0o077 != 0
                    || metadata.len() > FILE_LIMIT as u64
                {
                    return Err(
                        "Saved workspace must be a private regular file within its size limit."
                            .into(),
                    );
                }
                let mut bytes = Vec::new();
                file.take(FILE_LIMIT as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(io)?;
                if bytes.len() > FILE_LIMIT {
                    return Err("Saved workspace exceeds its size limit.".into());
                }
                serde_json::from_slice::<Session>(&bytes).map_err(|e| {
                    format!("Saved workspace is invalid ({e}); the file was not changed.")
                })?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Session::empty(host_store.clone())
            }
            Err(e) => return Err(io(e)),
        };
        session.validate()?;
        if session.host_store != host_store {
            return Err("Saved workspace belongs to another host store. Use a separate PLAIT_DESKTOP_STATE_DIR; nothing was sent or overwritten.".into());
        }
        let journal = Self {
            directory: directory.to_owned(),
            _lease: lease,
            queued: Mutex::new((0, Arc::new(session.clone()))),
            written: Mutex::new(0),
        };
        Ok((Arc::new(journal), session))
    }

    /// No file access. Call after a state transition, never from render.
    pub fn queue(&self, session: Session) {
        let mut queued = self.queued.lock().unwrap();
        queued.0 += 1;
        queued.1 = Arc::new(session);
    }

    /// Flush the newest queued state. Submission callers must await success first.
    pub fn flush(&self) -> Result<(), String> {
        let mut written = self.written.lock().unwrap();
        let (revision, session) = self.queued.lock().unwrap().clone();
        if revision <= *written {
            return Ok(());
        }
        session.validate()?;
        let bytes =
            serde_json::to_vec(&*session).map_err(|e| format!("Cannot encode workspace: {e}"))?;
        if bytes.len() > FILE_LIMIT {
            return Err("Saved workspace exceeds 4 MiB. Shorten drafts before sending; nothing was discarded.".into());
        }
        let path = self.directory.join("desktop.json");
        let temporary = self
            .directory
            .join(format!(".desktop.{}.{revision}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(|e| format!("Cannot save workspace: {e}"))?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &path)?;
            File::open(&self.directory)?.sync_all()
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "Workspace save failed: {error}. Submission was not sent."
            ));
        }
        *written = revision;
        Ok(())
    }
}

pub fn title(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|character| !character.is_control())
        .take(72)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("plait-session-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }
    fn thread() -> Thread {
        Thread {
            checkout: "/repo".into(),
            model: ModelChoice {
                provider: "p".into(),
                model_id: "m".into(),
            },
            title: "Fix the parser".into(),
            draft: Draft {
                text: "unfinished 🙂".into(),
                revision: 7,
                submitted: None,
            },
            pending: Some(PendingSubmit {
                request_id: "request-1".into(),
                text: "Do the task".into(),
            }),
        }
    }

    #[test]
    fn capture_cannot_reassign_a_draft_or_pending_request_to_a_new_identity() {
        let mut saved = Session::empty("/host".into());
        let original = thread();
        saved
            .update_thread("thread".into(), original.clone())
            .unwrap();
        for model_change in [false, true] {
            let mut changed = original.clone();
            if model_change {
                changed.model.model_id = "another-model".into();
            } else {
                changed.checkout = "/another-checkout".into();
            }
            let summary = crate::host::ThreadSummary {
                id: "thread".into(),
                checkout: changed.checkout.clone(),
                model: changed.model.clone(),
                status: crate::host::ThreadStatus::Idle,
            };
            assert!(!saved.accepts(&summary));
            assert!(saved.saved_thread(&summary).is_none());
            assert!(saved.update_thread("thread".into(), changed).is_err());
            assert_eq!(saved.threads["thread"], original);
        }
        let mut edited = original;
        edited.draft.text = "A later draft".into();
        saved
            .update_thread("thread".into(), edited.clone())
            .unwrap();
        assert_eq!(saved.threads["thread"], edited);
    }

    #[test]
    fn shared_lock_permissions_are_rejected_without_replacement() {
        let root = root("shared-lock");
        fs::create_dir_all(&root).unwrap();
        let lock = root.join("desktop.lock");
        fs::write(&lock, b"existing").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Journal::open(&root, "/host".into()).is_err());
        assert_eq!(fs::read(&lock).unwrap(), b"existing");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicated_descriptor_does_not_extend_journal_ownership() {
        let root = root("inherited-descriptor");
        let (journal, mut state) = Journal::open(&root, "/host".into()).unwrap();
        state.threads.insert("thread-1".into(), thread());
        journal.queue(state.clone());
        journal.flush().unwrap();
        let owner = journal.clone();
        // dup and fork share a flock's open file description, even with close-on-exec.
        let inherited = journal._lease.0.try_clone().unwrap();
        assert_ne!(
            unsafe { libc::fcntl(inherited.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
        drop(journal);
        assert!(Journal::open(&root, "/host".into()).is_err());
        drop(owner);
        let (reopened, saved) = Journal::open(&root, "/host".into()).unwrap();
        assert_eq!(saved, state);
        drop(inherited);
        assert!(Journal::open(&root, "/host".into()).is_err());
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fork_before_exec_does_not_delay_journal_reopen() {
        use std::{os::unix::net::UnixStream, os::unix::process::CommandExt, process::Command};
        let root = root("fork-reopen");
        let (journal, _) = Journal::open(&root, "/host".into()).unwrap();
        let (mut parent, child) = UnixStream::pair().unwrap();
        let timeout = Some(std::time::Duration::from_secs(10));
        parent.set_read_timeout(timeout).unwrap();
        child.set_read_timeout(timeout).unwrap();
        let spawner = std::thread::spawn(move || {
            let fd = child.as_raw_fd();
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "exit 0"]);
            // Only async-signal-safe I/O runs between fork and exec. The handshake
            // holds the inherited descriptor without timing sleeps or a live child leak.
            unsafe {
                command.pre_exec(move || {
                    let mut byte = 0_u8;
                    if libc::write(fd, (&byte as *const u8).cast(), 1) != 1
                        || libc::read(fd, (&mut byte as *mut u8).cast(), 1) != 1
                    {
                        return Err(io::Error::from_raw_os_error(libc::EIO));
                    }
                    Ok(())
                });
            }
            command.spawn().unwrap().wait().unwrap()
        });
        parent.read_exact(&mut [0]).unwrap();
        drop(journal);
        let reopened = Journal::open(&root, "/host".into());
        let released = parent.write_all(&[1]);
        let status = spawner.join().unwrap();
        released.unwrap();
        assert!(status.success());
        drop(reopened.unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_retains_drafts_pending_ids_and_model_identity() {
        let root = root("roundtrip");
        let (journal, mut state) = Journal::open(&root, "/host".into()).unwrap();
        state.checkout = Some("/repo".into());
        state.selected = Some("thread-1".into());
        state.threads.insert("thread-1".into(), thread());
        journal.queue(state.clone());
        journal.flush().unwrap();
        drop(journal);
        let (_, saved) = Journal::open(&root, "/host".into()).unwrap();
        assert_eq!(saved, state);
        assert_eq!(
            fs::metadata(root.join("desktop.json")).unwrap().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn queued_updates_coalesce_and_old_flushes_cannot_regress_state() {
        let root = root("ordering");
        let (journal, mut state) = Journal::open(&root, "/host".into()).unwrap();
        for i in 0..50 {
            state.selected = Some(format!("thread-{i}"));
            journal.queue(state.clone());
        }
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let journal = journal.clone();
                std::thread::spawn(move || journal.flush().unwrap())
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        drop(journal);
        assert_eq!(
            Journal::open(&root, "/host".into()).unwrap().1.selected,
            Some("thread-49".into())
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn second_client_and_different_host_cannot_overwrite_saved_work() {
        let root = root("ownership");
        let (journal, state) = Journal::open(&root, "/host".into()).unwrap();
        journal.queue(state);
        journal.flush().unwrap();
        assert!(Journal::open(&root, "/host".into()).is_err());
        drop(journal);
        assert!(Journal::open(&root, "/other-host".into()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_future_and_oversized_files_are_not_reset() {
        let root = root("invalid");
        let (journal, state) = Journal::open(&root, "/host".into()).unwrap();
        journal.queue(state.clone());
        journal.flush().unwrap();
        drop(journal);
        let path = root.join("desktop.json");
        let mut future = state;
        future.schema_version = 2;
        for bytes in [
            b"not json".to_vec(),
            serde_json::to_vec(&future).unwrap(),
            vec![b' '; FILE_LIMIT + 1],
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(Journal::open(&root, "/host".into()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn symlinks_and_invalid_submissions_fail_without_replacing_state() {
        let root = root("links");
        let (journal, mut state) = Journal::open(&root, "/host".into()).unwrap();
        journal.queue(state.clone());
        journal.flush().unwrap();
        let before = fs::read(root.join("desktop.json")).unwrap();
        let mut thread = thread();
        thread.pending.as_mut().unwrap().request_id = "bad\nrequest".into();
        state.threads.insert("thread-1".into(), thread);
        journal.queue(state);
        assert!(journal.flush().is_err());
        assert_eq!(fs::read(root.join("desktop.json")).unwrap(), before);
        drop(journal);
        fs::remove_file(root.join("desktop.json")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("desktop.json")).unwrap();
        assert!(Journal::open(&root, "/host".into()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn titles_are_plain_whitespace_normalized_and_unicode_safe() {
        assert_eq!(title("  Fix\n the\tparser 🙂  "), "Fix the parser 🙂");
        assert_eq!(title(&"🙂".repeat(100)).chars().count(), 72);
    }
}
