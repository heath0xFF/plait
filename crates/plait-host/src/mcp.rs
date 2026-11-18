//! MCP keeps stdin for JSON-RPC and uses a separate pipe as its lifetime lease.
use std::{fs::File, io, os::fd::FromRawFd, process::Stdio};

pub fn run(args: Vec<String>) -> io::Result<()> {
    if args.len() < 3 {
        return Err(io::Error::other(
            "mcp-server expects checkout profile program [arguments]",
        ));
    }
    let command = prepare(&args)?;
    // The guardian retains both writer leases. No private descriptor reaches a server.
    for fd in 3..crate::command::inherited_descriptor_limit() {
        unsafe {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    if unsafe { libc::fcntl(5, libc::F_GETFD) } < 0 {
        return Err(io::Error::other("MCP lifetime lease is missing"));
    }
    let lifetime = unsafe { File::from_raw_fd(5) };
    crate::command::guard(command, "MCP", lifetime, Stdio::inherit())
}

#[cfg(target_os = "macos")]
fn prepare(args: &[String]) -> io::Result<std::process::Command> {
    let mut command = if args[1] == "trusted" {
        std::process::Command::new(&args[2])
    } else {
        let mut command = crate::command::with_profile(&args[1]);
        command.arg(&args[2]);
        command
    };
    command.args(&args[3..]).current_dir(&args[0]);
    Ok(command)
}

#[cfg(not(target_os = "macos"))]
fn prepare(_: &[String]) -> io::Result<std::process::Command> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "MCP processes require the macOS command policy",
    ))
}
