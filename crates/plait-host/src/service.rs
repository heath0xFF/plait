use std::{
    ffi::CStr,
    io,
    mem::MaybeUninit,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
};

pub fn user_home() -> io::Result<PathBuf> {
    // Launch services must use the account's home, not an installer's environment.
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the user host service must not run as root",
        ));
    }
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
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if found.is_null() {
        return Err(io::Error::other("current user account was not found"));
    }
    let record = unsafe { record.assume_init() };
    if record.pw_dir.is_null() {
        return Err(io::Error::other("current user has no home directory"));
    }
    // Copy before the account record's backing buffer leaves scope.
    let directory = unsafe { CStr::from_ptr(record.pw_dir) };
    home_path(directory.to_bytes())
}

fn home_path(bytes: &[u8]) -> io::Result<PathBuf> {
    let path = Path::new(std::ffi::OsStr::from_bytes(bytes));
    if !path.is_absolute() {
        return Err(io::Error::other("user home must be an absolute path"));
    }
    Ok(path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_home_comes_from_the_account_and_requires_an_absolute_path() {
        if unsafe { libc::geteuid() } == 0 {
            assert_eq!(
                user_home().unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
        } else {
            assert!(user_home().unwrap().is_absolute());
        }
        assert!(home_path(b"").is_err());
        assert!(home_path(b"relative").is_err());
        assert_eq!(
            home_path(b"/fixture/home").unwrap(),
            Path::new("/fixture/home")
        );
    }
}
