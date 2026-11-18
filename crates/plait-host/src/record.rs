//! `plait-host record` and `plait-host prune`: the runtime's helpers for the
//! host's own records of a checkout. Each reads one JSON request on stdin and
//! writes one JSON reply on stdout.
//!
//! On macOS the work runs in a second copy of this program under
//! `sandbox-exec`, with a profile that denies writes to the checkout and its
//! Git directories, so neither Git nor this code can change the person's
//! repository, not even a modification time. Elsewhere it runs directly.

use plait_core::git::record::{self, Located, PruneRequest, Request};
use serde_json::{Value, json};
use std::{
    io::{self, Read, Write},
    path::Path,
};

/// Requests are small; anything larger is not from the runtime.
const REQUEST_LIMIT: u64 = 1 << 20;

fn read_request() -> io::Result<Value> {
    let mut bytes = Vec::new();
    io::stdin()
        .take(REQUEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > REQUEST_LIMIT {
        return Err(io::Error::other("record request too large"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn reply(value: Value) -> io::Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &value)?;
    out.write_all(b"\n")
}

fn failure(message: String) -> io::Error {
    io::Error::other(message)
}

pub fn run() -> io::Result<()> {
    let request: Request = serde_json::from_value(read_request()?).map_err(io::Error::other)?;
    if !request.store.is_absolute() {
        return Err(failure("store must be absolute".into()));
    }
    let located = record::locate(&request.checkout).map_err(failure)?;
    let protected = record::protected_paths(&located);
    // Validate before creating anything: even mkdir or chmod would break the guarantee.
    let store = resolve_store(&request.store)?;
    record::sandbox_profile(&protected, &store).map_err(failure)?;
    let records = prepare_store(&store)?;
    contain(
        "record-contained",
        &json!({"request": request, "located": located}),
        &protected,
        &records,
    )
}

pub fn run_contained() -> io::Result<()> {
    let value = read_request()?;
    let request: Request =
        serde_json::from_value(value["request"].clone()).map_err(io::Error::other)?;
    let located: Located =
        serde_json::from_value(value["located"].clone()).map_err(io::Error::other)?;
    let made = record::record(&located, &request).map_err(failure)?;
    reply(serde_json::to_value(made)?)
}

pub fn prune() -> io::Result<()> {
    let request: PruneRequest =
        serde_json::from_value(read_request()?).map_err(io::Error::other)?;
    if !request.store.is_absolute() {
        return Err(failure("store must be absolute".into()));
    }
    let records = record::records_dir(&request.store);
    if !records.is_dir() {
        return reply(serde_json::to_value(record::Pruned::default())?);
    }
    let records = records.canonicalize()?;
    // Every checkout the store reads from: pruning reads their objects too.
    let sources = record::sources(&request.store);
    let mut protected = Vec::new();
    for source in &sources {
        for path in [&source.root, &source.git_dir, &source.common_dir] {
            if let Ok(path) = path.canonicalize()
                && !path.starts_with(&records)
            {
                protected.push(path);
            }
        }
    }
    let protected: Vec<&Path> = protected.iter().map(|p| p.as_path()).collect();
    contain(
        "prune-contained",
        &serde_json::to_value(&request).map_err(io::Error::other)?,
        &protected,
        &records,
    )
}

pub fn prune_contained() -> io::Result<()> {
    let request: PruneRequest =
        serde_json::from_value(read_request()?).map_err(io::Error::other)?;
    reply(serde_json::to_value(
        record::prune(&request).map_err(failure)?,
    )?)
}

/// Resolve missing final directories without creating them inside a protected tree.
fn resolve_store(path: &Path) -> io::Result<std::path::PathBuf> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(error)?;
            let name = path
                .file_name()
                .ok_or_else(|| io::Error::other("invalid store path"))?;
            Ok(resolve_store(parent)?.join(name))
        }
        Err(error) => Err(error),
    }
}

/// The records directory, created private and resolved, so the sandbox
/// profile names the path the kernel sees.
fn prepare_store(store: &Path) -> io::Result<std::path::PathBuf> {
    let records = record::records_dir(store);
    std::fs::create_dir_all(&records)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&records, std::fs::Permissions::from_mode(0o700))?;
    records.canonicalize()
}

/// Runs `operation` in a copy of this program that cannot write `protected`.
#[cfg(target_os = "macos")]
fn contain(operation: &str, input: &Value, protected: &[&Path], records: &Path) -> io::Result<()> {
    use std::process::Stdio;
    let profile = record::sandbox_profile(protected, records).map_err(failure)?;
    let mut child = crate::command::with_profile(&profile)
        .arg(std::env::current_exe()?)
        .arg(operation)
        // Resolved here once, so the contained copy does not look it up again.
        .env("PLAIT_GIT", record::git_binary())
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("recorder stdin missing"))?;
    stdin.write_all(&serde_json::to_vec(input)?)?;
    drop(stdin);
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        // The contained copy already said why on stderr.
        std::process::exit(status.code().unwrap_or(1))
    }
}

/// Without a sandbox, the recorder's own design is the only guard: commands
/// that write objects never see the person's object directory.
#[cfg(not(target_os = "macos"))]
fn contain(operation: &str, input: &Value, _: &[&Path], _: &Path) -> io::Result<()> {
    match operation {
        "record-contained" => {
            let request: Request =
                serde_json::from_value(input["request"].clone()).map_err(io::Error::other)?;
            let located: Located =
                serde_json::from_value(input["located"].clone()).map_err(io::Error::other)?;
            reply(serde_json::to_value(
                record::record(&located, &request).map_err(failure)?,
            )?)
        }
        _ => {
            let request: PruneRequest =
                serde_json::from_value(input.clone()).map_err(io::Error::other)?;
            reply(serde_json::to_value(
                record::prune(&request).map_err(failure)?,
            )?)
        }
    }
}
