use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, Metadata},
    io,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::PathBuf,
};

pub fn stamp(meta: &Metadata) -> Value {
    json!({
        "dev":meta.dev().to_string(), "ino":meta.ino().to_string(),
        "mode":meta.mode().to_string(), "size":meta.len().to_string(),
        "mtime":format!("{}:{}",meta.mtime(),meta.mtime_nsec()),
        "ctime":format!("{}:{}",meta.ctime(),meta.ctime_nsec()),
    })
}

#[cfg(target_os = "macos")]
fn descriptor_path(file: &File) -> io::Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut path = [0u8; libc::PATH_MAX as usize];
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, path.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let end = path
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| io::Error::other("descriptor path too long"))?;
    Ok(std::ffi::OsStr::from_bytes(&path[..end]).into())
}

#[cfg(not(target_os = "macos"))]
fn descriptor_path(file: &File) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

pub fn postimage(file: &File, root: &str, bytes: &[u8]) -> io::Result<Value> {
    let path = descriptor_path(file)?;
    let relative = path.strip_prefix(root).map_err(io::Error::other)?;
    let path = relative
        .to_str()
        .ok_or_else(|| io::Error::other("non-UTF8 descriptor path"))?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || meta.len() != bytes.len() as u64 {
        return Err(io::Error::other("file changed before postimage receipt"));
    }
    let mut result = stamp(&meta);
    result["path"] = json!(path);
    // Hash the bytes this operation read or wrote, never a later path reopen.
    result["sha256"] = json!(format!("{:x}", Sha256::digest(bytes)));
    Ok(result)
}
