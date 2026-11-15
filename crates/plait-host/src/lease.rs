use std::{
    fs::File,
    io,
    os::fd::{AsRawFd, FromRawFd},
};

pub fn acquire() -> io::Result<()> {
    // flock follows the shared open description, which Node retains after we exit.
    let file = unsafe { File::from_raw_fd(3) };
    let meta = file.metadata()?;
    if !meta.is_dir() {
        return Err(io::Error::other("invalid checkout lease"));
    }
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::other("checkout already has a writer"));
    }
    Ok(())
}
