//! Blocking local-host transport. Call these operations on the background executor.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

pub const REQUEST_LIMIT: usize = 64 * 1024;
/// Newest protocol this client speaks. Version 2 adds ordered tools and approval modes.
pub const PROTOCOL_VERSION: u32 = 2;
const REPLY_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelChoice {
    pub provider: String,
    pub model_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub provider: String,
    pub model_id: String,
    pub name: String,
    pub authenticated: bool,
}

impl Model {
    pub fn choice(&self) -> ModelChoice {
        ModelChoice {
            provider: self.provider.clone(),
            model_id: self.model_id.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub protocol_version: u32,
    pub default_model: Option<ModelChoice>,
    pub models: Vec<Model>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    Idle,
    Running,
    AwaitingApproval,
    Stopped,
    Interrupted,
    Error,
}

impl ThreadStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::Running => "Running",
            Self::AwaitingApproval => "Needs approval",
            Self::Stopped => "Stopped",
            Self::Interrupted => "Interrupted",
            Self::Error => "Error",
        }
    }
    pub fn busy(self) -> bool {
        matches!(self, Self::Running | Self::AwaitingApproval)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadSummary {
    pub id: String,
    pub checkout: PathBuf,
    pub model: ModelChoice,
    pub status: ThreadStatus,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub id: String,
    pub checkout: PathBuf,
    pub model: ModelChoice,
    pub status: ThreadStatus,
    pub messages: Vec<Message>,
    pub live_text: String,
    pub tools: Vec<Tool>,
    pub pending_approval: Option<Approval>,
    pub error: Option<HostError>,
    pub truncated: bool,
    /// Absent from version 1 hosts, which always ask.
    #[serde(default)]
    pub approval_mode: Option<ApprovalMode>,
}

/// How a run's edits and commands are authorized. Sandbox and file checks apply to both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    Ask,
    Auto,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub text: String,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub status: ToolStatus,
    pub input: String,
    pub output: String,
    /// `None`: a version 1 host sent no order. `Some(None)`: before any message.
    #[serde(default, deserialize_with = "present")]
    pub after: Option<Option<String>>,
}

// Distinguishes a missing field from an explicit null.
fn present<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Running,
    Done,
    Error,
    Interrupted,
}
impl ToolStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Done => "Done",
            Self::Error => "Error",
            Self::Interrupted => "Interrupted",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub id: String,
    pub tool_id: String,
    pub kind: ApprovalKind,
    pub summary: String,
    pub details: String,
    pub dirty_paths: Vec<String>,
    pub risk: String,
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    Edit,
    Command,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub enum Error {
    Transport(String),
    Protocol(String),
    Rejected(HostError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(message) | Self::Protocol(message) => f.write_str(message),
            Self::Rejected(error) => write!(f, "{}: {}", error.code, error.message),
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Transport(format!(
            "Local host connection failed: {error}. Start the host, then Reconnect."
        ))
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Status {
        #[serde(skip_serializing_if = "Option::is_none")]
        protocol_version: Option<u32>,
    },
    SetDefaultModel {
        provider: String,
        model_id: String,
    },
    ListThreads {
        checkout: PathBuf,
    },
    CreateThread {
        checkout: PathBuf,
        provider: String,
        model_id: String,
        request_id: String,
    },
    Submit {
        thread_id: String,
        request_id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        protocol_version: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        approval: Option<ApprovalMode>,
    },
    Stop {
        thread_id: String,
    },
    Approve {
        thread_id: String,
        approval_id: String,
        allow: bool,
    },
    Watch {
        thread_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        protocol_version: Option<u32>,
    },
}

impl Request {
    /// Version 1 hosts reject unknown fields, so only a negotiated version 2 sends any.
    pub fn submit(
        thread_id: String,
        request_id: String,
        text: String,
        approval: Option<ApprovalMode>,
    ) -> Self {
        Self::Submit {
            thread_id,
            request_id,
            text,
            protocol_version: approval.map(|_| 2),
            approval,
        }
    }

    /// The largest form of a submission, for size checks before negotiation.
    pub fn submit_bound(thread_id: &str, request_id: &str, text: &str) -> Self {
        Self::submit(
            thread_id.into(),
            request_id.into(),
            text.into(),
            Some(ApprovalMode::Auto),
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadsReply {
    pub threads: Vec<ThreadSummary>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadReply {
    pub thread: Snapshot,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultReply {
    pub default_model: ModelChoice,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitReply {
    pub thread_id: String,
    pub request_id: String,
    pub accepted: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
    error: Option<HostError>,
}

fn decode<T: DeserializeOwned>(line: &[u8]) -> Result<T, Error> {
    let envelope: Envelope<T> = serde_json::from_slice(line).map_err(|_| {
        Error::Protocol("Invalid host reply. Update the host and desktop together.".into())
    })?;
    match (envelope.ok, envelope.result, envelope.error) {
        (true, Some(value), None) => Ok(value),
        (false, None, Some(error)) => Err(Error::Rejected(error)),
        _ => Err(Error::Protocol(
            "Inconsistent host reply; no action was confirmed.".into(),
        )),
    }
}

#[derive(Clone)]
pub struct Client {
    path: PathBuf,
}
impl Client {
    pub fn from_environment() -> Result<Self, Error> {
        let path = if let Some(path) = std::env::var_os("PLAIT_HOST_STORE") {
            PathBuf::from(path).join("host.sock")
        } else {
            PathBuf::from(std::env::var_os("HOME").ok_or_else(|| {
                Error::Transport("HOME is not set; cannot locate the local host.".into())
            })?)
            .join(".local/share/plait/host.sock")
        };
        if !path.is_absolute() {
            return Err(Error::Transport(
                "PLAIT_HOST_STORE must be an absolute path.".into(),
            ));
        }
        Ok(Self { path })
    }

    fn connect(&self, request: &Request) -> Result<Frames, Error> {
        let bytes = serde_json::to_vec(request)
            .map_err(|_| Error::Protocol("Cannot encode host request.".into()))?;
        if bytes.len() + 1 > REQUEST_LIMIT {
            return Err(Error::Protocol(
                "Message exceeds the host's 64 KiB request limit.".into(),
            ));
        }
        let mut socket = crate::local_socket::connect(&self.path)?;
        socket.set_write_timeout(Some(Duration::from_secs(5)))?;
        socket.write_all(&bytes)?;
        socket.write_all(b"\n")?;
        Ok(Frames {
            reader: BufReader::new(socket),
            pending: Vec::new(),
            partial_since: None,
        })
    }

    /// One request and reply. A versioned status request is the negotiation:
    /// a version 1 host rejects the field as unknown, so ask again without it.
    pub fn request<T: DeserializeOwned>(&self, request: &Request) -> Result<T, Error> {
        let result = self.exchange(request);
        if let (
            Request::Status {
                protocol_version: Some(_),
            },
            Err(Error::Rejected(error)),
        ) = (request, &result)
            && error.code == "invalid_request"
        {
            return self.exchange(&Request::Status {
                protocol_version: None,
            });
        }
        result
    }

    fn exchange<T: DeserializeOwned>(&self, request: &Request) -> Result<T, Error> {
        let mut frames = self.connect(request)?;
        decode(&frames.next(Some(Instant::now() + Duration::from_secs(15)))?)
    }

    pub fn watch(&self, thread_id: String, protocol: u32) -> Result<Watch, Error> {
        Ok(Watch {
            frames: self.connect(&Request::Watch {
                thread_id: thread_id.clone(),
                protocol_version: (protocol >= 2).then_some(2),
            })?,
            thread_id,
            identity: None,
            first: true,
            initial_deadline: Instant::now() + Duration::from_secs(15),
        })
    }
}

struct Frames {
    reader: BufReader<UnixStream>,
    pending: Vec<u8>,
    partial_since: Option<Instant>,
}
impl Frames {
    fn next(&mut self, deadline: Option<Instant>) -> Result<Vec<u8>, Error> {
        loop {
            let deadline = deadline.or_else(|| {
                self.partial_since
                    .map(|start| start + Duration::from_secs(15))
            });
            let timeout = if let Some(deadline) = deadline {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(Error::Transport("Host frame timed out. The request may have been accepted; reconnect before retrying.".into()));
                }
                Some(remaining)
            } else {
                None
            };
            self.reader.get_ref().set_read_timeout(timeout)?;
            let available = match self.reader.fill_buf() {
                Ok(bytes) => bytes,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(Error::Transport("Host frame timed out. Reconnect before retrying; the request may have been accepted.".into()));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            };
            if available.is_empty() {
                return Err(Error::Transport(
                    "Host disconnected. Reconnect to resume this thread.".into(),
                ));
            }
            let end = available.iter().position(|byte| *byte == b'\n');
            let count = end.map_or(available.len(), |index| index + 1);
            if self.pending.len() + count > REPLY_LIMIT {
                return Err(Error::Protocol(
                    "Host reply exceeds 1 MiB; connection closed.".into(),
                ));
            }
            self.partial_since.get_or_insert_with(Instant::now);
            self.pending.extend_from_slice(&available[..count]);
            self.reader.consume(count);
            if end.is_some() {
                self.partial_since = None;
                return Ok(std::mem::take(&mut self.pending));
            }
        }
    }
}

mod watch;
pub use watch::{CancelWatch, Snapshots};

pub struct Watch {
    frames: Frames,
    thread_id: String,
    identity: Option<(PathBuf, ModelChoice)>,
    first: bool,
    initial_deadline: Instant,
}
impl Watch {
    pub fn next_snapshot(&mut self) -> Result<Snapshot, Error> {
        let line = self
            .frames
            .next(self.first.then_some(self.initial_deadline))?;
        let snapshot = if self.first {
            let reply: ThreadReply = decode(&line)?;
            self.first = false;
            reply.thread
        } else {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Event {
                event: String,
                thread: Snapshot,
            }
            let event: Event = serde_json::from_slice(&line).map_err(|_| {
                Error::Protocol("Invalid host event; reconnect to load current state.".into())
            })?;
            if event.event != "snapshot" {
                return Err(Error::Protocol("Unsupported host event.".into()));
            }
            event.thread
        };
        // Validate before coalescing so a later good frame cannot hide an identity violation.
        if snapshot.id != self.thread_id {
            return Err(Error::Protocol("Host returned the wrong thread.".into()));
        }
        if let Some((checkout, model)) = &self.identity {
            if *checkout != snapshot.checkout || *model != snapshot.model {
                return Err(Error::Protocol(
                    "Host changed this thread's saved model or checkout.".into(),
                ));
            }
        } else {
            self.identity = Some((snapshot.checkout.clone(), snapshot.model.clone()));
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_round_trip_checks_socket_ownership_and_permissions() {
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};
        let directory = std::env::temp_dir().join(format!("plait-client-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("host.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(socket.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["op"], "status");
            socket.write_all(b"{\"ok\":true,\"result\":{\"protocol_version\":1,\"default_model\":null,\"models\":[]}}\n").unwrap();
        });
        let client = Client { path: path.clone() };
        let reply: Status = client
            .request(&Request::Status {
                protocol_version: None,
            })
            .unwrap();
        assert_eq!(reply.protocol_version, 1);
        server.join().unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(matches!(client.request::<Status>(&Request::Status {
                protocol_version: None
            }), Err(Error::Transport(message)) if message.contains("owner-only")));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_inconsistent_or_unknown_replies() {
        assert!(decode::<DefaultReply>(br#"{"ok":true,"result":{"default_model":{"provider":"p","model_id":"m"}},"error":{"code":"internal","message":"bad"}}"#).is_err());
        assert!(decode::<DefaultReply>(br#"{"ok":true,"result":{"default_model":{"provider":"p","model_id":"m"}},"extra":true}"#).is_err());
        assert!(matches!(
            decode::<DefaultReply>(br#"{"ok":false,"error":{"code":"busy","message":"Running"}}"#),
            Err(Error::Rejected(_))
        ));
    }

    #[test]
    fn drip_fed_frames_obey_absolute_deadline() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            for _ in 0..100 {
                if writer.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let mut frames = Frames {
            reader: BufReader::new(reader),
            pending: Vec::new(),
            partial_since: None,
        };
        let deadline = Instant::now() + Duration::from_millis(20);
        assert!(matches!(
            frames.next(Some(deadline)),
            Err(Error::Transport(_))
        ));
        assert!(!frames.pending.is_empty());
        drop(frames);
        writer.join().unwrap();
    }

    #[test]
    fn frames_keep_partial_data_and_bound_size() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_millis(5)))
            .unwrap();
        let mut frames = Frames {
            reader: BufReader::new(reader),
            pending: Vec::new(),
            partial_since: None,
        };
        writer.write_all(b"partial").unwrap();
        assert!(
            frames
                .next(Some(Instant::now() + Duration::from_millis(5)))
                .is_err()
        );
        writer.write_all(b"\nnext\n").unwrap();
        assert_eq!(frames.next(None).unwrap(), b"partial\n");
        assert_eq!(frames.next(None).unwrap(), b"next\n");
        frames.pending = vec![b'x'; REPLY_LIMIT];
        writer.write_all(b"x").unwrap();
        assert!(matches!(frames.next(None), Err(Error::Protocol(_))));
    }
}
