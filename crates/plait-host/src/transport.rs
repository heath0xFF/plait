use plait_core::local_socket;
use serde_json::{Value, json};
#[cfg(test)]
use std::io::BufRead;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

mod notifications;
use notifications::Notifications;

const REQUEST_LIMIT: usize = 65536;
const OUTPUT_LIMIT: usize = 1048576;

#[cfg(test)]
pub fn frame(reader: &mut impl std::io::BufRead, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > limit || bytes.last() != Some(&b'\n') {
        return Err(io::Error::other("invalid or oversized frame"));
    }
    Ok(bytes)
}

fn ready(fd: i32, events: i16, deadline: Instant) -> io::Result<()> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "frame deadline exceeded"))?;
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, remaining.as_millis().max(1) as i32) };
        if result > 0 {
            return Ok(());
        }
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(io::Error::last_os_error());
        }
    }
}

fn timed_frame(
    reader: &mut (impl Read + AsRawFd),
    limit: usize,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        ready(reader.as_raw_fd(), libc::POLLIN, deadline)?;
        let mut buffer = [0u8; 4096];
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Err(io::Error::other("connection closed before newline"));
        }
        let end = buffer[..count].iter().position(|byte| *byte == b'\n');
        bytes.extend_from_slice(&buffer[..end.map_or(count, |index| index + 1)]);
        if bytes.len() > limit {
            return Err(io::Error::other("frame exceeds bound"));
        }
        if end.is_some() {
            return Ok(bytes);
        }
    }
}

fn timed_write(
    writer: &mut (impl Write + AsRawFd),
    mut bytes: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    let fd = writer.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    while !bytes.is_empty() {
        ready(fd, libc::POLLOUT, deadline)?;
        match writer.write(bytes) {
            Ok(0) => return Err(io::Error::other("connection closed")),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub fn status(store: &Path) -> io::Result<Vec<u8>> {
    let mut socket = local_socket::connect(&store.join("host.sock"))?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.write_all(b"{\"op\":\"status\"}\n")?;
    let reply = timed_frame(
        &mut socket,
        OUTPUT_LIMIT,
        Instant::now() + Duration::from_secs(2),
    )?;
    let value: Value = serde_json::from_slice(&reply)?;
    if value["ok"] != true {
        return Err(io::Error::other("host is not ready"));
    }
    Ok(reply)
}

struct Sidecar {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
}

impl Sidecar {
    fn request(&mut self, request: &Value) -> io::Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut exchange = || -> io::Result<Value> {
            let mut bytes = serde_json::to_vec(request)?;
            bytes.push(b'\n');
            timed_write(&mut self.input, &bytes, deadline)?;
            let bytes = timed_frame(&mut self.output, OUTPUT_LIMIT, deadline)?;
            Ok(serde_json::from_slice(&bytes)?)
        };
        let result = exchange();
        if result.is_err() {
            let _ = self.child.kill();
        }
        result
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn private_store(store: &Path) -> io::Result<File> {
    fs::create_dir_all(store)?;
    let meta = fs::symlink_metadata(store)?;
    // Never follow a caller-supplied store symlink or take another user's store.
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other("store must be an owned directory"));
    }
    fs::set_permissions(store, fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(store.join("host.lock"))?;
    if !lock.metadata()?.is_file() || lock.metadata()?.nlink() != 1 {
        return Err(io::Error::other("invalid store lock"));
    }
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::other("store already has a writer"));
    }
    Ok(lock)
}

pub fn serve(
    store: &Path,
    runtime: &Path,
    node: &std::ffi::OsStr,
    home: Option<&Path>,
) -> io::Result<()> {
    // Private creation modes also apply to SQLite WALs and runtime scratch files.
    unsafe {
        libc::umask(0o077);
    }
    let lock = private_store(store)?;
    // Validate the leaf before resolving system aliases such as /tmp.
    let store = fs::canonicalize(store)?;
    let socket_path = store.join("host.sock");
    if let Ok(meta) = fs::symlink_metadata(&socket_path) {
        if !meta.file_type().is_socket() {
            return Err(io::Error::other("socket path is not a socket"));
        }
        fs::remove_file(&socket_path)?;
    }
    // The sidecar retains the writer lock if the host dies before it handles EOF.
    if unsafe { libc::fcntl(lock.as_raw_fd(), libc::F_SETFD, 0) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut startup = crate::startup::Startup::new(&store);
    startup.phase("starting_sidecar")?;
    let (notify_reader, notify_writer) = UnixStream::pair()?;
    // Keep change notifications separate from the serialized request/reply pipe.
    if unsafe { libc::fcntl(notify_writer.as_raw_fd(), libc::F_SETFD, 0) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut command = Command::new(node);
    command
        .arg("--disable-warning=ExperimentalWarning")
        .arg(runtime)
        .arg(store)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .env("PLAIT_HOST_EXE", std::env::current_exe()?)
        .env("PLAIT_LOCK_FD", lock.as_raw_fd().to_string())
        .env("PLAIT_NOTIFY_FD", notify_writer.as_raw_fd().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(home) = home {
        command.env("HOME", home).current_dir(home);
    }
    let child = command.spawn();
    drop(notify_writer);
    let mut child = child.inspect_err(|error| startup.failed(error))?;
    startup.child(child.id());
    let input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("sidecar stdin missing"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("sidecar stdout missing"))?;
    let sidecar = Arc::new(Mutex::new(Sidecar {
        child,
        input,
        output,
    }));
    startup.phase("waiting_for_runtime")?;
    let ready = sidecar
        .lock()
        .unwrap()
        .request(&json!({"op":"status"}))
        .inspect_err(|error| startup.failed(error))?;
    if ready["ok"] != true {
        let error = io::Error::other("sidecar initialization failed");
        startup.failed(&error);
        return Err(error);
    }
    startup.phase("binding_socket")?;
    let listener = UnixListener::bind(&socket_path)?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    startup.phase("ready")?;
    let notifications = Arc::new(Notifications::default());
    let relay = Arc::clone(&notifications);
    thread::spawn(move || {
        // EOF also detects sidecar death, with no periodic child-process check.
        let _ = notifications::relay(notify_reader, &relay);
        std::process::exit(1);
    });
    let clients = Arc::new(AtomicUsize::new(0));
    for socket in listener.incoming() {
        let socket = socket?;
        if clients.fetch_add(1, Ordering::Relaxed) >= 64 {
            clients.fetch_sub(1, Ordering::Relaxed);
            continue;
        }
        let sidecar = Arc::clone(&sidecar);
        let clients = Arc::clone(&clients);
        let notifications = Arc::clone(&notifications);
        thread::spawn(move || {
            let _ = connection(socket, &sidecar, &notifications);
            clients.fetch_sub(1, Ordering::Relaxed);
        });
    }
    Ok(())
}

fn send(socket: &mut UnixStream, value: &Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    if bytes.len() > OUTPUT_LIMIT {
        return Err(io::Error::other("response exceeds bound"));
    }
    timed_write(socket, &bytes, Instant::now() + Duration::from_secs(2))
}

fn connection(
    mut socket: UnixStream,
    sidecar: &Mutex<Sidecar>,
    notifications: &Arc<Notifications>,
) -> io::Result<()> {
    local_socket::require_same_user(&socket)?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    let request = timed_frame(
        &mut socket,
        REQUEST_LIMIT,
        Instant::now() + Duration::from_secs(5),
    )
    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).map_err(io::Error::other));
    let request = match request {
        Ok(request) => request,
        Err(_) => {
            return send(
                &mut socket,
                &json!({"ok":false,"error":{"code":"invalid_request","message":"Expected one bounded JSON line."}}),
            );
        }
    };
    // Register before reading state so a concurrent change cannot be missed.
    let mut subscription = if request["op"] == "watch" {
        request["thread_id"]
            .as_str()
            .map(|id| notifications.subscribe(id.to_owned()))
            .transpose()?
    } else {
        None
    };
    let reply = sidecar
        .lock()
        .map_err(|_| io::Error::other("sidecar failed"))?
        .request(&request)?;
    send(&mut socket, &reply)?;
    if reply["ok"] != true {
        return Ok(());
    }
    let Some(subscription) = subscription.as_mut() else {
        return Ok(());
    };
    let mut previous = reply["result"]["thread"].clone();
    while subscription.wait(&socket)? {
        let reply = sidecar
            .lock()
            .map_err(|_| io::Error::other("sidecar failed"))?
            .request(&request)?;
        if reply["ok"] != true {
            return send(&mut socket, &reply);
        }
        let snapshot = &reply["result"]["thread"];
        if *snapshot != previous {
            send(&mut socket, &json!({"event":"snapshot","thread":snapshot}))?;
            previous = snapshot.clone();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drip_input_cannot_extend_absolute_deadline() {
        let (mut input, mut output) = UnixStream::pair().unwrap();
        thread::spawn(move || {
            for _ in 0..5 {
                if output.write_all(b"x").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(40));
            }
        });
        let start = Instant::now();
        assert!(timed_frame(&mut input, 100, start + Duration::from_millis(90)).is_err());
        assert!(start.elapsed() < Duration::from_millis(180));
    }

    #[test]
    fn bounded_frames_require_newline() {
        assert!(frame(&mut &b"{}"[..], 3).is_err());
        assert!(frame(&mut &b"1234\n"[..], 4).is_err());
        assert_eq!(frame(&mut &b"{}\nnext"[..], 3).unwrap(), b"{}\n");
    }
}
