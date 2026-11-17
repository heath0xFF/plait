//! Client side of version 4 harness choice: which agent harnesses can run a
//! thread here, and which one runs each thread. docs/HOST.md "Agent harnesses"
//! is the contract.
//!
//! New harnesses, states, and fields may be added, so unknown ones decode as
//! `Other` or are ignored rather than failing the reply.

use super::Model;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The agent harness that runs a thread. Pi is built into the host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessId {
    #[default]
    Pi,
    Codex,
    /// A harness from a newer host. It can be shown, never chosen.
    #[serde(other, skip_serializing)]
    Other,
}

impl HarnessId {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pi => "Pi",
            Self::Codex => "Codex",
            Self::Other => "Another harness",
        }
    }
}

/// Which harness ran a thread, and the version of its latest run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct ThreadHarness {
    pub id: HarnessId,
    /// `None` before the first run, and for Pi, which ships with the host.
    pub version: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Hosts from before version 4 reject it as `invalid_request`.
    Harnesses { protocol_version: u32 },
    /// Run this harness from `path` instead of the login shell's PATH; `None` returns
    /// to PATH. The reply is the whole list, checked again.
    SetHarnessPath {
        harness: HarnessId,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<PathBuf>,
        protocol_version: u32,
    },
}

impl Request {
    pub fn harnesses() -> Self {
        Self::Harnesses {
            protocol_version: 4,
        }
    }

    pub fn set_harness_path(harness: HarnessId, path: Option<PathBuf>) -> Self {
        Self::SetHarnessPath {
            harness,
            path,
            protocol_version: 4,
        }
    }
}

/// Whether a harness can start a thread now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessState {
    Ready,
    NotInstalled,
    /// Older than Plait supports. A newer one is `Ready` with a note.
    UnsupportedVersion,
    SignedOut,
    /// Installed, but it could not be started or asked; `fix` says why.
    Unavailable,
    #[serde(other)]
    Other,
}

impl HarnessState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::NotInstalled => "Not installed",
            Self::UnsupportedVersion => "Version not supported",
            Self::SignedOut => "Signed out",
            Self::Unavailable => "Unavailable",
            Self::Other => "Unknown",
        }
    }
}

/// What a harness's threads can do in Plait.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Capabilities {
    /// A message sent during a run joins it; otherwise it waits for the next run.
    pub steer: bool,
    /// Each edit and command can be a Plait approval.
    pub approvals: bool,
    pub thinking: bool,
    pub usage: bool,
}

/// The versions Plait checks a harness against.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Supported {
    /// The oldest version Plait runs.
    pub min: String,
    /// The version Plait's message types come from; newer ones run untested.
    pub tested: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Harness {
    pub id: HarnessId,
    pub name: String,
    pub state: HarnessState,
    /// What to do when the state is not `Ready`, or a caveat when it is.
    pub note: Option<String>,
    pub version: Option<String>,
    /// `None` for Pi, which ships with the host.
    pub supported: Option<Supported>,
    /// The executable used, when the harness is the person's own install.
    pub path: Option<PathBuf>,
    pub capabilities: Capabilities,
    /// What protects the checkout in this harness's threads, in one line.
    pub protection: String,
    /// `None` for Pi, whose models are in the status catalog.
    pub models: Option<Vec<Model>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Harnesses {
    pub harnesses: Vec<Harness>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_is_version_4() {
        assert_eq!(
            serde_json::to_value(Request::harnesses()).unwrap(),
            serde_json::json!({"op": "harnesses", "protocol_version": 4})
        );
        assert_eq!(
            serde_json::to_value(Request::set_harness_path(
                HarnessId::Codex,
                Some("/opt/codex".into())
            ))
            .unwrap(),
            serde_json::json!({"op": "set_harness_path", "harness": "codex", "path": "/opt/codex", "protocol_version": 4})
        );
        assert_eq!(
            serde_json::to_value(Request::set_harness_path(HarnessId::Codex, None)).unwrap(),
            serde_json::json!({"op": "set_harness_path", "harness": "codex", "protocol_version": 4})
        );
    }

    #[test]
    fn a_reply_decodes_and_tolerates_later_additions() {
        let reply: Harnesses = serde_json::from_str(
            r#"{"harnesses":[
                {"id":"pi","name":"Pi","state":"ready","note":null,"version":null,"supported":null,"path":null,
                 "capabilities":{"steer":true,"approvals":true,"thinking":true,"usage":true},
                 "protection":"Plait's sandbox and approvals","models":null},
                {"id":"codex","name":"Codex","state":"ready","note":"Newer than tested.","version":"0.170.0",
                 "supported":{"min":"0.169.0","tested":"0.169.0"},"path":"/bin/codex",
                 "capabilities":{"steer":true,"approvals":true,"thinking":true,"usage":true,"later":1},
                 "protection":"Codex's sandbox","models":[{"provider":"codex","model_id":"m","name":"M","authenticated":true}]},
                {"id":"someday","name":"Someday","state":"warming_up","note":null,"version":null,"supported":null,
                 "path":null,"capabilities":{"steer":false,"approvals":false,"thinking":false,"usage":false},
                 "protection":"","models":[],"later":true}],
              "later":true}"#,
        )
        .unwrap();
        let [pi, codex, other] = &reply.harnesses[..] else {
            panic!("three harnesses")
        };
        assert_eq!((pi.id, pi.state), (HarnessId::Pi, HarnessState::Ready));
        assert!(pi.models.is_none());
        assert_eq!(codex.supported.as_ref().unwrap().tested, "0.169.0");
        assert_eq!(codex.models.as_ref().unwrap()[0].model_id, "m");
        assert_eq!(
            (other.id, other.state),
            (HarnessId::Other, HarnessState::Other)
        );
    }
}
