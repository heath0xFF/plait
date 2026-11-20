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
#[derive(Serialize, Deserialize)]
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
fn seed_lock(store: &Path, identity: &Identity, exclusive: bool) -> io::Result<fs::File> {
    let folder = policy::folder(store, "build-output-seeds")?;
    let path = folder.join(format!("{}.lock", policy::key(identity)?));
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.nlink() != 1
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::other("Unsafe build seed lock."));
    }
    if unsafe {
        libc::flock(
            file.as_raw_fd(),
            (if exclusive {
                libc::LOCK_EX
            } else {
                libc::LOCK_SH
            }) | libc::LOCK_NB,
        )
    } != 0
    {
        return Err(io::Error::other("Build seed is being prepared or used."));
    }
    Ok(file)
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
fn seed(
    store: &Path,
    root: &Path,
    identity: &Identity,
    generation: &str,
    source_cache: Option<&str>,
) -> io::Result<Outcome> {
    uuid(generation)?;
    let (main, main_identity) = policy::project(root)?;
    let _seed_lock = seed_lock(store, &main_identity, true)?;
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
    // Only this project's unfinished staging directory is recovered, never a seed in use.
    let pending = folder.join(format!("{}.pending.json", policy::key(&main_identity)?));
    let retired = folder.join(format!("{}.retired.json", policy::key(&main_identity)?));
    if let Some(old) = policy::read::<Seed>(&retired)? {
        if previous
            .as_ref()
            .is_none_or(|current| current.path != old.path)
        {
            remove_seed(folder, &old)?;
        }
        fs::remove_file(&retired)?;
    }
    if let Some(old) = policy::read::<Seed>(&pending)? {
        if previous
            .as_ref()
            .is_none_or(|committed| committed.path != old.path)
        {
            remove_seed(folder, &old)?;
        }
        fs::remove_file(&pending)?;
    }
    let path = folder.join(format!("{}-{generation}", policy::key(&main_identity)?));
    fs::create_dir(&path)?;
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
        drop(capture);
        // A policy edit during the scan cannot publish a widened or stale seed.
        if policy::load(store, &main, &main_identity)?.revision != approved.revision {
            return Err(io::Error::other(
                "Build output approval changed during seeding.",
            ));
        }
        if let Some(previous) = &previous {
            policy::save(&retired, previous)?;
        }
        policy::save(&pointer, &seed)?;
        published = true;
        if let Some(previous) = &previous {
            remove_seed(folder, previous)?;
            fs::remove_file(&retired)?;
        }
        fs::remove_file(&pending)?;
        Ok(Outcome {
            warm: !seed.directories.is_empty(),
            reason: "An idle checkout supplied an immutable build seed.".into(),
            directories: seed.directories.clone(),
            counts: seed.counts.clone(),
        })
    })();
    if result.is_err() && !published {
        remove_seed(folder, &seed)?;
        let _ = fs::remove_file(&pending);
    }
    result
}
fn remove_seed(folder: &Path, seed: &Seed) -> io::Result<()> {
    if seed.path.parent() != Some(folder) {
        return Err(io::Error::other("Invalid build seed path."));
    }
    build_output::remove_owned(
        folder,
        Path::new(seed.path.file_name().unwrap()),
        identity_pair(&seed.identity)?,
    )
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
