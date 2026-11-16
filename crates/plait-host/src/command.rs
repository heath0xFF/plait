use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    thread,
};

pub fn run(args: Vec<String>) -> io::Result<()> {
    if args.len() != 3 {
        return Err(io::Error::other("command expects checkout policy script"));
    }
    let mut command = sandbox_command(&args)?;
    // Keep the writer lease in the guardian, never expose it to approved commands.
    unsafe {
        libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(4, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    let mut child = command.stdin(Stdio::null()).process_group(0).spawn()?;
    let group = child.id() as i32;
    // A pipe is a lifetime lease: even SIGKILL of Node closes it and stops tools.
    thread::spawn(move || {
        loop {
            match io::stdin().read(&mut [0u8; 1]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                _ => {
                    unsafe {
                        libc::kill(-group, libc::SIGKILL);
                    }
                    break;
                }
            }
        }
    });
    let status = child.wait()?;
    // Shells may exit while background children still hold the output pipes.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "sandboxed command failed or stopped: {status}"
        )))
    }
}

#[cfg(target_os = "macos")]
fn sandbox_command(args: &[String]) -> io::Result<Command> {
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .args(["-p", &args[1], "/bin/sh", "-c", &args[2]])
        .current_dir(&args[0]);
    Ok(command)
}

#[cfg(not(target_os = "macos"))]
fn sandbox_command(_: &[String]) -> io::Result<Command> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "command containment is not enabled on this platform",
    ))
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_denies_before_starting_a_command() {
        let error = run(vec!["/nonexistent".into(), String::new(), "exit 0".into()]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }
}
