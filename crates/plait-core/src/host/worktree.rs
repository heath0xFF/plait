//! Additive host-owned worktree operations. Blocking I/O belongs off the UI thread.
use super::{ApprovalMode, Client, Error, HostError, ThinkingLevel, harness::HarnessId};
use crate::git::worktree::{Identity, ManagedWorktree, Parent};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const OLDER_HOST: &str = "Host-owned worktrees need a newer host.";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    WorktreeInfo {
        checkout: PathBuf,
    },
    CreateWorktreeThread {
        request_id: String,
        parent: Box<Parent>,
        harness: HarnessId,
        provider: String,
        model_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<ThinkingLevel>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<String>,
        approval: ApprovalMode,
    },
    WorktreeStatus {
        request_id: String,
    },
    ListWorktrees,
    RemoveWorktree {
        request_id: String,
        creation_id: String,
        identity: Identity,
        allow_unpushed: bool,
    },
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    pub schema: u32,
    pub worktree_threads_v1: bool,
    pub parent: Parent,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Preparing,
    Ready,
    Failed,
    Removing,
    Removed,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub request_id: String,
    pub thread_id: String,
    pub state: Stage,
    pub worktree: Option<ManagedWorktree>,
    pub error: Option<HostError>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    schema: u32,
    operation: Operation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listing {
    schema: u32,
    operations: Vec<Operation>,
}
fn invalid() -> Error {
    Error::Protocol("Invalid worktree reply. Reconnect before retrying.".into())
}
fn bounded_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn path(path: &std::path::Path) -> bool {
    path.is_absolute()
        && path.as_os_str().len() <= 4096
        && path.to_str().is_some_and(|s| {
            !s.chars().any(char::is_control)
                && !s.contains("//")
                && !s.split('/').any(|part| part == "." || part == "..")
        })
}
fn identity(identity: &Identity) -> bool {
    [&identity.dev, &identity.ino].into_iter().all(|value| {
        value
            .parse::<u64>()
            .is_ok_and(|number| number.to_string() == *value)
    })
}
fn parent(parent: &Parent) -> bool {
    [&parent.root, &parent.main, &parent.common]
        .into_iter()
        .all(|p| path(p))
        && [
            &parent.identity,
            &parent.main_identity,
            &parent.common_identity,
        ]
        .into_iter()
        .all(identity)
        && matches!(parent.base.len(), 40 | 64)
        && parent
            .base
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
impl Operation {
    pub fn verified(self) -> Result<Self, Error> {
        let hex = self.thread_id.replace('-', "");
        if !bounded_id(&self.request_id)
            || self.thread_id.len() != 36
            || !self.thread_id.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
                }
            })
        {
            return Err(invalid());
        }
        if let Some(tree) = &self.worktree {
            let suffix = tree.branch.strip_prefix("plait/").ok_or_else(invalid)?;
            if tree.thread_id != self.thread_id
                || !parent(&tree.parent)
                || !path(&tree.path)
                || !matches!(suffix.len(), 12 | 16 | 20 | 24 | 28 | 32)
                || !hex.starts_with(suffix)
                || tree.path.file_name().and_then(|p| p.to_str()) != Some(suffix)
                || tree.identity.as_ref().is_some_and(|id| !identity(id))
                || tree.error.as_ref().is_some_and(|e| e.len() > 8192)
            {
                return Err(invalid());
            }
            if matches!(self.state, Stage::Ready)
                && tree.stage != crate::git::worktree::ManagedStage::Ready
                || matches!(self.state, Stage::Removed)
                    && tree.stage != crate::git::worktree::ManagedStage::Removed
            {
                return Err(invalid());
            }
            let mut name = tree.parent.main.file_name().ok_or_else(invalid)?.to_owned();
            name.push(".worktrees");
            if tree.path.parent()
                != Some(
                    tree.parent
                        .main
                        .parent()
                        .ok_or_else(invalid)?
                        .join(name)
                        .as_path(),
                )
            {
                return Err(invalid());
            }
        }
        if matches!(self.state, Stage::Ready | Stage::Removed | Stage::Removing)
            && self
                .worktree
                .as_ref()
                .is_none_or(|tree| tree.identity.is_none())
        {
            return Err(invalid());
        }
        Ok(self)
    }
}
fn compatibility(error: Error) -> Error {
    if matches!(&error, Error::Rejected(reason) if reason.code == "invalid_request") {
        Error::Protocol(OLDER_HOST.into())
    } else {
        error
    }
}
pub fn info(client: &Client, checkout: PathBuf) -> Result<Info, Error> {
    let info: Info = client
        .send(&Request::WorktreeInfo { checkout })
        .map_err(compatibility)?;
    if info.schema != 1 || !info.worktree_threads_v1 || !parent(&info.parent) {
        return Err(invalid());
    }
    Ok(info)
}
pub fn operation(client: &Client, request: &Request) -> Result<Operation, Error> {
    let reply: Reply = client.send(request)?;
    if reply.schema != 1 {
        return Err(invalid());
    }
    reply.operation.verified()
}
pub fn list(client: &Client) -> Result<Vec<Operation>, Error> {
    let reply: Listing = client
        .send(&Request::ListWorktrees)
        .map_err(compatibility)?;
    if reply.schema != 1 || reply.operations.len() > 256 {
        return Err(invalid());
    }
    reply
        .operations
        .into_iter()
        .map(Operation::verified)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn ready() -> serde_json::Value {
        let identity = json!({"dev":"1","ino":"2"});
        json!({"request_id":"request","thread_id":"12345678-1234-4abc-8abc-123456789abc","state":"ready","error":null,
            "worktree":{"thread_id":"12345678-1234-4abc-8abc-123456789abc","parent":{"root":"/repo","main":"/repo","common":"/repo/.git","identity":identity,"main_identity":identity,"common_identity":identity,"base":"a".repeat(40)},"path":"/repo.worktrees/123456781234","branch":"plait/123456781234","identity":identity,"stage":"ready","error":null}})
    }
    #[test]
    fn strict_worktree_replies_reject_unknown_states_fields_and_forged_paths() {
        let good = ready();
        assert!(
            serde_json::from_value::<Operation>(good.clone())
                .unwrap()
                .verified()
                .is_ok()
        );
        for (field, value) in [("state", json!("future")), ("extra", json!(true))] {
            let mut bad = good.clone();
            bad[field] = value;
            assert!(serde_json::from_value::<Operation>(bad).is_err());
        }
        for (field, value) in [
            ("path", json!("/elsewhere/123456781234")),
            ("branch", json!("plait/other")),
            ("identity", json!({"dev":"1","ino":"18446744073709551616"})),
            ("thread_id", json!("different")),
        ] {
            let mut bad = good.clone();
            bad["worktree"][field] = value;
            assert!(
                serde_json::from_value::<Operation>(bad)
                    .unwrap()
                    .verified()
                    .is_err()
            );
        }
        let mut bad = good;
        bad["worktree"] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<Operation>(bad)
                .unwrap()
                .verified()
                .is_err()
        );
    }
    #[test]
    fn additive_worktree_requests_omit_version_and_reject_unrecognized_fields() {
        let request = Request::WorktreeStatus {
            request_id: "same-id".into(),
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({"op":"worktree_status","request_id":"same-id"})
        );
        assert!(
            serde_json::from_value::<Request>(
                json!({"op":"worktree_status","request_id":"id","protocol_version":6})
            )
            .is_err()
        );
    }
    #[test]
    fn worktree_reply_requires_canonical_ids_and_consistent_stages() {
        let good = ready();
        let mut bad = good.clone();
        bad["thread_id"] = json!("123456781234-4abc-8abc-123456789abc-");
        let mut variants = vec![bad];
        for (field, value) in [
            ("stage", json!("removed")),
            ("identity", json!({"dev":"+1","ino":"2"})),
            ("identity", json!({"dev":"1","ino":"02"})),
            ("path", json!("/repo.worktrees//123456781234")),
        ] {
            let mut bad = good.clone();
            bad["worktree"][field] = value;
            variants.push(bad);
        }
        let mut bad = good;
        bad["worktree"]["branch"] = json!("plait/1234567812344");
        bad["worktree"]["path"] = json!("/repo.worktrees/1234567812344");
        variants.push(bad);
        for bad in variants {
            assert!(
                serde_json::from_value::<Operation>(bad)
                    .unwrap()
                    .verified()
                    .is_err()
            );
        }
    }
}
