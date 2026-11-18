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
    let command = sandbox_command(&args)?;
    // Keep the writer lease in the guardian, never expose it to approved commands.
    unsafe {
        libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::fcntl(4, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    guard(command, "sandboxed", io::stdin(), Stdio::null())
}

/// Runs a command as the user, without Seatbelt, for a project whose owner
/// chose the trusted command policy. Lifetime rules match the sandboxed path.
pub fn run_trusted(args: Vec<String>) -> io::Result<()> {
    if args.len() != 2 {
        return Err(io::Error::other("trusted-command expects checkout script"));
    }
    let mut command = trusted_command(&args)?;
    private_environment(&mut command)?;
    // The command has full user access, so no inherited descriptor may reach it:
    // not the leases, and not a host-private channel a parent failed to close.
    for fd in 3..inherited_descriptor_limit() {
        unsafe {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
    }
    guard(command, "trusted", io::stdin(), Stdio::null())
}

pub(crate) fn inherited_descriptor_limit() -> i32 {
    let limit = unsafe { libc::getdtablesize() };
    limit.clamp(5, 65_536)
}

pub(crate) fn guard(
    mut command: Command,
    label: &str,
    mut lifetime: impl Read + Send + 'static,
    stdin: Stdio,
) -> io::Result<()> {
    let mut child = command.stdin(stdin).process_group(0).spawn()?;
    let group = child.id() as i32;
    // A pipe is a lifetime lease: even SIGKILL of Node closes it and stops tools.
    thread::spawn(move || {
        loop {
            match lifetime.read(&mut [0u8; 1]) {
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
            "{label} command failed or stopped: {status}"
        )))
    }
}

#[cfg(target_os = "macos")]
fn trusted_command(args: &[String]) -> io::Result<Command> {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", &args[1]]).current_dir(&args[0]);
    Ok(command)
}

// Trusted mode is not a way around the missing Linux containment decision.
#[cfg(not(target_os = "macos"))]
fn trusted_command(_: &[String]) -> io::Result<Command> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "command containment is not enabled on this platform",
    ))
}

#[cfg(target_os = "macos")]
fn sandbox_command(args: &[String]) -> io::Result<Command> {
    let mut command = if std::env::var_os("PLAIT_COMMAND_ENV_FD").is_some() {
        let mut command = crate::mcp::sandbox_command(&args[1], "/bin/sh")?;
        command.args(["-c", &args[2]]);
        command
    } else {
        let mut command = with_profile(&args[1]);
        command.args(["/bin/sh", "-c", &args[2]]);
        command
    };
    command.current_dir(&args[0]);
    Ok(command)
}

/// Both command execution and recording enter Seatbelt through the host.
#[cfg(target_os = "macos")]
pub(crate) fn with_profile(profile: &str) -> Command {
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command.args(["-p", profile]);
    command
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

    #[test]
    fn unsupported_platform_denies_trusted_commands_too() {
        let error = run_trusted(vec!["/nonexistent".into(), "exit 0".into()]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }
}

fn private_environment(command: &mut Command) -> io::Result<()> {
    if std::env::var_os("PLAIT_COMMAND_ENV_FD").is_some() {
        command.env_clear().envs(crate::mcp::environment()?);
    }
    Ok(())
}
