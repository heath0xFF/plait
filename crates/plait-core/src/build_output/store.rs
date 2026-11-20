//! Host-private approvals, immutable seeds and recoverable worktree preparation.
pub mod policy;
mod transaction;
use crate::{
    build_output::{self, Counts, Outcome},
    git::{
        self,
        worktree::{Identity, ManagedWorktree},
    },
    lease::{self, LeaseClass},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Info {
        store: PathBuf,
        root: PathBuf,
    },
    Set {
        store: PathBuf,
        root: PathBuf,
        identity: Identity,
        revision: u64,
        paths: Vec<String>,
    },
    Seed {
        store: PathBuf,
        root: PathBuf,
        identity: Identity,
        generation: String,
        source_cache: Option<String>,
    },
    Clone {
        store: PathBuf,
        worktree: ManagedWorktree,
        private_target: bool,
    },
    Recover {
        store: PathBuf,
        worktree: ManagedWorktree,
    },
}
#[derive(Clone, Serialize, Deserialize)]
struct Seed {
    revision: u64,
    path: PathBuf,
    identity: Identity,
    directories: Vec<String>,
    counts: Counts,
}
pub fn run() -> io::Result<()> {
    // This helper starts no descendants. EOF means its host died: stop promptly
    // and let the next host recover its durable transaction before admission.
    unsafe {
        libc::umask(0o077);
    }
    if std::env::var("PLAIT_BUILD_LIFETIME_FD").as_deref() == Ok("4") {
        std::thread::spawn(|| {
            use std::os::fd::FromRawFd;
            let mut lifetime = unsafe { fs::File::from_raw_fd(4) };
            let mut byte = [0];
            let _ = lifetime.read(&mut byte);
            std::process::exit(75);
        });
    }
    let mut bytes = Vec::new();
    io::stdin().take(65537).read_to_end(&mut bytes)?;
    let result = if bytes.len() > 65536 {
        Err(io::Error::other("Build output request is too large."))
    } else {
        serde_json::from_slice::<Request>(&bytes)
            .map_err(io::Error::other)
            .and_then(execute)
    };
    emit(&match result {
        Ok(value) => json!({"value":value}),
        Err(e) => json!({"error":build_output::cold_reason(&e)}),
    })
}
fn emit(value: &Value) -> io::Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()
}
fn progress(counts: &Counts) {
    let _ = emit(&json!({"progress":counts}));
}
fn execute(request: Request) -> io::Result<Value> {
    let value = match request {
        Request::Info { store, root } => {
            let (main, identity) = policy::project(&root)?;
            let policy = policy::load(&store, &main, &identity)?;
            let suggestions = git::build_output::suggestions(&main).map_err(io::Error::other)?;
            json!({"schema":1,"worktree_build_output_v1":true,"policy":policy,"suggestions":suggestions})
        }
        Request::Set {
            store,
            root,
            identity,
            revision,
            paths,
        } => {
            let policy = policy::set(&store, &root, &identity, revision, paths)?;
            json!({"schema":1,"worktree_build_output_v1":true,"policy":policy,"suggestions":[]})
        }
        Request::Seed {
            store,
            root,
            identity,
            generation,
            source_cache,
        } => serde_json::to_value(seed(
            &store,
            &root,
            &identity,
            &generation,
            source_cache.as_deref(),
        )?)?,
        Request::Clone {
            store,
            worktree,
            private_target,
        } => serde_json::to_value(transaction::prepare(&store, &worktree, private_target)?)?,
        Request::Recover { store, worktree } => {
            serde_json::to_value(transaction::recover(&store, &worktree)?)?
        }
    };
    Ok(value)
}
fn uuid(id: &str) -> io::Result<()> {
    if id.len() != 36
        || !id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
            }
        })
    {
        return Err(io::Error::other("Invalid build output identity."));
    }
    Ok(())
}
/// Opens an owner-only lock file. Without `create`, a missing file is `None`.
fn lock_file(path: &Path, create: bool) -> io::Result<Option<fs::File>> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if !create && e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.nlink() != 1
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::other("Unsafe build seed lock."));
    }
    Ok(Some(file))
}
fn flock(file: &fs::File, operation: libc::c_int) -> io::Result<bool> {
    loop {
        // SAFETY: an open lock file descriptor.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::EWOULDBLOCK) => return Ok(false),
            _ => return Err(error),
        }
    }
}
/// One seed refresh per project at a time. Creation never takes this lock.
fn seeder_lock(store: &Path, identity: &Identity) -> io::Result<fs::File> {
    let folder = policy::folder(store, "build-output-seeds")?;
    let file = lock_file(
        &folder.join(format!("{}.seeding.lock", policy::key(identity)?)),
        true,
    )?
    .unwrap();
    if !flock(&file, libc::LOCK_EX | libc::LOCK_NB)? {
        return Err(io::Error::other(
            "Another build seed refresh is active for this project.",
        ));
    }
    Ok(file)
}
/// Each seed has its own lock: a creation holds it shared while cloning, and
/// deleting a retired seed waits for it exclusively. A refresh never blocks a
/// creation from the published seed, and a deletion never removes one in use.
fn generation_lock(seed: &Path) -> PathBuf {
    let mut name = seed.file_name().unwrap_or_default().to_owned();
    name.push(".lock");
    seed.with_file_name(name)
}
/// The published seed, held shared. A pointer that moves while it is being
/// locked is read again, so a concurrent refresh or deletion cannot make
/// creation start cold while a seed is published.
fn published_seed(store: &Path, identity: &Identity) -> io::Result<Option<(Seed, fs::File)>> {
    let pointer = seed_pointer(store, identity)?;
    for _ in 0..8 {
        let Some(seed) = policy::read::<Seed>(&pointer)? else {
            return Ok(None);
        };
        let Some(lock) = lock_file(&generation_lock(&seed.path), false)? else {
            if policy::read::<Seed>(&pointer)?.is_some_and(|now| now.path == seed.path) {
                return Ok(None);
            }
            continue;
        };
        if flock(&lock, libc::LOCK_SH | libc::LOCK_NB)?
            && policy::read::<Seed>(&pointer)?.is_some_and(|now| now.path == seed.path)
        {
            return Ok(Some((seed, lock)));
        }
    }
    Ok(None)
}
fn seed_pointer(store: &Path, identity: &Identity) -> io::Result<PathBuf> {
    Ok(policy::folder(store, "build-output-seeds")?
        .join(format!("{}.json", policy::key(identity)?)))
}
fn private_cache(store: &Path, thread: &str) -> io::Result<PathBuf> {
    uuid(thread)?;
    let parent = policy::folder(store, "build-cache")?;
    let root = parent.join(thread);
    policy::private_directory(&root, true)?;
    Ok(root)
}
const MAIN_ONLY: &str = "Build seeds are taken only from the project's main checkout.";
fn seed(
    store: &Path,
    root: &Path,
    identity: &Identity,
    generation: &str,
    source_cache: Option<&str>,
) -> io::Result<Outcome> {
    uuid(generation)?;
    let (main, main_identity) = policy::project(root)?;
    // A linked worktree's output is one thread's: it must not replace a fuller
    // seed, nor become every later thread's starting cache.
    if main != root {
        return Ok(Outcome::cold(MAIN_ONLY));
    }
    let _seeder = seeder_lock(store, &main_identity)?;
    let capture = lease::seed::capture(
        root,
        identity.lease().map_err(io::Error::other)?,
        std::env::var("PLAIT_BUILD_LIFETIME_FD").as_deref() == Ok("4"),
    )
    .map_err(io::Error::other)?;
    let approved = policy::load(store, &main, &main_identity)?;
    if approved.paths.is_empty() {
        return Ok(Outcome::cold(
            "No build directories are approved for this project.",
        ));
    }
    let pointer = seed_pointer(store, &main_identity)?;
    let previous: Option<Seed> = policy::read(&pointer)?;
    let folder = pointer.parent().unwrap();
    let key = policy::key(&main_identity)?;
    let pending = folder.join(format!("{key}.pending.json"));
    let retired = folder.join(format!("{key}.retired.json"));
    // An abandoned capture joins the retired list; it is deleted after this
    // refresh publishes, so a creation waiting for a seed never waits for it.
    if let Some(old) = policy::read::<Seed>(&pending)? {
        if previous
            .as_ref()
            .is_none_or(|committed| committed.path != old.path)
        {
            retire(&retired, old)?;
        }
        fs::remove_file(&pending)?;
    }
    let path = folder.join(format!("{key}-{generation}"));
    fs::create_dir(&path)?;
    lock_file(&generation_lock(&path), true)?;
    let mut seed = Seed {
        revision: approved.revision,
        identity: Identity::read(&path).map_err(io::Error::other)?,
        path,
        directories: Vec::new(),
        counts: Counts::default(),
    };
    policy::save(&pending, &seed)?;
    let mut published = false;
    let result = (|| {
        for (index, directory) in approved.paths.iter().enumerate() {
            git::build_output::ignored(root, directory).map_err(io::Error::other)?;
            let source_root = if directory == "target" {
                source_cache
                    .map(|id| private_cache(store, id))
                    .transpose()?
                    .unwrap_or(root.into())
            } else {
                root.into()
            };
            let source_identity = if source_root == root {
                identity.clone()
            } else {
                Identity::read(&source_root).map_err(io::Error::other)?
            };
            let source = match build_output::directory_owned(
                &source_root,
                Path::new(directory),
                identity_pair(&source_identity)?,
            ) {
                Ok(source) => source,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    seed.counts
                        .note(format!("{directory}: no build output yet"));
                    continue;
                }
                Err(e) => return Err(e),
            };
            let destination = seed.path.join(index.to_string());
            fs::create_dir(&destination)?;
            let output = build_output::directory(&destination, Path::new(""))?;
            let counts =
                build_output::clone_tree(&source.0, &output.0, capture.cancelled(), &mut progress)?;
            merge(&mut seed.counts, &counts);
            seed.directories.push(directory.clone());
        }
        identity.checked(root).map_err(io::Error::other)?;
        // A policy edit during the scan cannot publish a widened or stale seed.
        if policy::load(store, &main, &main_identity)?.revision != approved.revision {
            return Err(io::Error::other(
                "Build output approval changed during seeding.",
            ));
        }
        if let Some(previous) = &previous {
            retire(&retired, previous.clone())?;
        }
        policy::save(&pointer, &seed)?;
        published = true;
        fs::remove_file(&pending)?;
        Ok(Outcome {
            warm: !seed.directories.is_empty(),
            reason: "An idle checkout supplied an immutable build seed.".into(),
            directories: seed.directories.clone(),
            counts: seed.counts.clone(),
        })
    })();
    drop(capture);
    if result.is_err() && !published {
        remove_seed(folder, &seed)?;
        let _ = fs::remove_file(&pending);
    }
    // Deletion holds no source lease and no lock a creation needs; it waits
    // only for a creation still cloning from the seed being retired.
    match (result, delete_retired(folder, &retired)) {
        (Ok(mut outcome), Err(error)) => {
            outcome
                .counts
                .note(format!("An earlier build seed is not deleted yet: {error}"));
            Ok(outcome)
        }
        (result, _) => result,
    }
}
/// Seeds waiting to be deleted. A list, so an interrupted deletion is resumed
/// by the next refresh and no abandoned seed is forgotten.
fn retire(list: &Path, seed: Seed) -> io::Result<()> {
    let mut seeds: Vec<Seed> = policy::read(list)?.unwrap_or_default();
    if !seeds.iter().any(|s| s.path == seed.path) {
        seeds.push(seed);
    }
    policy::save(list, &seeds)
}
fn delete_retired(folder: &Path, list: &Path) -> io::Result<()> {
    let mut seeds: Vec<Seed> = policy::read(list)?.unwrap_or_default();
    while let Some(seed) = seeds.first() {
        remove_seed(folder, seed)?;
        seeds.remove(0);
        if seeds.is_empty() {
            fs::remove_file(list)?;
        } else {
            policy::save(list, &seeds)?;
        }
    }
    Ok(())
}
fn remove_seed(folder: &Path, seed: &Seed) -> io::Result<()> {
    if seed.path.parent() != Some(folder) {
        return Err(io::Error::other("Invalid build seed path."));
    }
    let lock_path = generation_lock(&seed.path);
    let lock = lock_file(&lock_path, false)?;
    if let Some(lock) = &lock {
        flock(lock, libc::LOCK_EX)?;
    }
    build_output::remove_owned(
        folder,
        Path::new(seed.path.file_name().unwrap()),
        identity_pair(&seed.identity)?,
    )?;
    if lock.is_some() {
        fs::remove_file(lock_path)?;
    }
    Ok(())
}
fn identity_pair(identity: &Identity) -> io::Result<(u64, u64)> {
    let id = identity.lease().map_err(io::Error::other)?;
    Ok((id.dev, id.ino))
}
fn merge(total: &mut Counts, next: &Counts) {
    total.files += next.files;
    total.links += next.links;
    total.skipped += next.skipped;
    total.bytes += next.bytes;
    total.elapsed_ms += next.elapsed_ms;
    total.notes.extend(
        next.notes
            .iter()
            .take(20usize.saturating_sub(total.notes.len()))
            .cloned(),
    );
}

#[cfg(test)]
mod tests;
