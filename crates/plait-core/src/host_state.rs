use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::host::{ApprovalMode, ModelChoice, QueueMode, Request, Snapshot, Status, ThreadSummary};
use std::{path::PathBuf, sync::Arc};

pub struct HostState {
    pub epoch: u64,
    pub watch_epoch: u64,
    pub checkout: PathBuf,
    pub connected: bool,
    pub connecting: bool,
    pub error: Option<String>,
    pub status: Option<Status>,
    pub threads: Vec<ThreadSummary>,
    pub selected: Option<String>,
    pub snapshot: Option<Arc<Snapshot>>,
    pub new_model: Option<ModelChoice>,
    pub settings_model: Option<ModelChoice>,
    pub saving: bool,
    pub save_message: Option<String>,
    pub pending_create: Option<(String, ModelChoice)>,
    /// The starting thinking level fixed when `pending_create` was made, so a
    /// retry repeats the same request even if the default changes meanwhile.
    pub pending_thinking: Option<crate::host::ThinkingLevel>,
    pub creating: bool,
    pub submitting: bool,
    pub controlling: bool,
    pub decided_approvals: HashSet<String>,
    pub submissions: Submissions,
}

impl HostState {
    pub fn new(checkout: PathBuf) -> Self {
        Self {
            epoch: 0,
            watch_epoch: 0,
            checkout,
            connected: false,
            connecting: false,
            error: None,
            status: None,
            threads: Vec::new(),
            selected: None,
            snapshot: None,
            new_model: None,
            settings_model: None,
            saving: false,
            save_message: None,
            pending_create: None,
            pending_thinking: None,
            creating: false,
            submitting: false,
            controlling: false,
            decided_approvals: HashSet::new(),
            submissions: Submissions::default(),
        }
    }

    pub fn reconnect(&mut self) -> u64 {
        self.epoch += 1;
        self.watch_epoch += 1;
        self.connected = false;
        self.connecting = true;
        self.snapshot = None;
        self.error = None;
        self.submitting = false;
        self.submissions.disconnected();
        self.controlling = false;
        self.creating = false;
        if self.saving {
            self.save_message =
                Some("Save not confirmed. Reconnect will reload the saved default.".into());
        }
        self.saving = false;
        self.decided_approvals.clear();
        self.epoch
    }

    pub fn select(&mut self, id: Option<String>) -> u64 {
        self.watch_epoch += 1;
        self.selected = id;
        self.snapshot = None;
        self.controlling = false;
        self.watch_epoch
    }

    pub fn apply_status(&mut self, epoch: u64, mut status: Status) -> bool {
        if epoch != self.epoch {
            return false;
        }
        self.connecting = false;
        if !(1..=crate::host::PROTOCOL_VERSION).contains(&status.protocol_version) {
            self.connected = false;
            self.error =
                Some("Unsupported host protocol. Update the host and desktop together.".into());
            return false;
        }
        self.connected = true;
        if self.pending_create.is_none() && !self.model_available(self.new_model.as_ref(), &status)
        {
            self.new_model = status
                .default_model
                .clone()
                .filter(|choice| self.model_available(Some(choice), &status));
        }
        status.models.sort_by_key(|model| !model.authenticated);
        self.settings_model = status.default_model.clone();
        self.status = Some(status);
        true
    }

    /// The negotiated protocol; version 1 until a status reply says otherwise.
    pub fn protocol(&self) -> u32 {
        self.status
            .as_ref()
            .map_or(1, |status| status.protocol_version)
    }

    fn model_available(&self, choice: Option<&ModelChoice>, status: &Status) -> bool {
        choice.is_some_and(|choice| {
            status
                .models
                .iter()
                .any(|model| model.authenticated && model.choice() == *choice)
        })
    }

    pub fn available(&self, choice: Option<&ModelChoice>) -> bool {
        self.connected
            && self
                .status
                .as_ref()
                .is_some_and(|status| self.model_available(choice, status))
    }

    pub fn accepts_watch(&self, epoch: u64, watch_epoch: u64, id: &str) -> bool {
        self.epoch == epoch
            && self.watch_epoch == watch_epoch
            && self.selected.as_deref() == Some(id)
    }

    pub fn apply_snapshot(&mut self, epoch: u64, watch_epoch: u64, snapshot: Snapshot) -> bool {
        if !self.accepts_watch(epoch, watch_epoch, &snapshot.id) {
            return false;
        }
        let expected = self
            .threads
            .iter()
            .find(|thread| thread.id == snapshot.id)
            .map(|thread| (&thread.model, &thread.checkout))
            .or_else(|| {
                self.snapshot
                    .as_ref()
                    .map(|old| (&old.model, &old.checkout))
            });
        if expected.is_some_and(|(model, checkout)| {
            (self.protocol() < 3 && *model != snapshot.model) || *checkout != snapshot.checkout
        }) {
            self.connected = false;
            self.error =
                Some("Host changed this thread's identity. Reconnect to reload it.".into());
            return false;
        }
        if let Some(thread) = self
            .threads
            .iter_mut()
            .find(|thread| thread.id == snapshot.id)
        {
            thread.status = snapshot.status;
            thread.model = snapshot.model.clone();
        }
        self.snapshot = Some(Arc::new(snapshot));
        true
    }

    pub fn finish_save(&mut self, result: Result<ModelChoice, String>) {
        self.saving = false;
        match result {
            Ok(choice) => {
                if let Some(status) = &mut self.status {
                    status.default_model = Some(choice.clone());
                }
                self.settings_model = Some(choice);
                self.save_message =
                    Some("Default model saved. Existing threads keep their model.".into());
            }
            Err(error) => self.save_message = Some(format!("Save not confirmed: {error}")),
        }
    }
}

/// The exact request to retry after an uncertain response. It is journaled before I/O.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingSubmit {
    pub request_id: String,
    pub text: String,
    #[serde(default)]
    pub approval: Option<ApprovalMode>,
    #[serde(default)]
    pub when_busy: Option<QueueMode>,
    #[serde(default)]
    pub draft_revision: Option<u64>,
}

impl PendingSubmit {
    pub fn request(&self, thread_id: &str) -> Request {
        match self.when_busy {
            Some(mode) => Request::submit_when_busy(
                thread_id.into(),
                self.request_id.clone(),
                self.text.clone(),
                self.approval.unwrap_or(ApprovalMode::Ask),
                mode,
            ),
            None => Request::submit(
                thread_id.into(),
                self.request_id.clone(),
                self.text.clone(),
                self.approval,
            ),
        }
    }
}

#[derive(Default)]
pub struct Submissions {
    pending: HashMap<String, Vec<PendingSubmit>>,
    accepted: HashMap<String, Vec<PendingSubmit>>,
    /// Transient only: restored entries always require an explicit retry.
    sending: HashSet<String>,
    errors: HashMap<String, String>,
}

impl Submissions {
    /// Legacy clients allow one outstanding request per thread.
    pub fn begin(&mut self, thread_id: &str, text: String) -> Result<PendingSubmit, String> {
        if self.pending(thread_id).is_some() {
            return Err("Resolve the previous submission before sending another message.".into());
        }
        self.begin_with(thread_id, text, None, None, None)
    }

    pub fn begin_with(
        &mut self,
        thread_id: &str,
        text: String,
        approval: Option<ApprovalMode>,
        when_busy: Option<QueueMode>,
        draft_revision: Option<u64>,
    ) -> Result<PendingSubmit, String> {
        if text.trim().is_empty() {
            return Err("Enter a message before sending.".into());
        }
        if self.all(thread_id).len() >= 16 {
            return Err("Resolve a pending message before sending another.".into());
        }
        if draft_revision.is_some()
            && self
                .all(thread_id)
                .iter()
                .any(|p| p.draft_revision == draft_revision)
        {
            return Err("This draft is already awaiting acceptance. Use Retry safely.".into());
        }
        let submission = PendingSubmit {
            request_id: request_id(),
            text,
            approval,
            when_busy,
            draft_revision,
        };
        let request =
            crate::host::Request::submit_bound(thread_id, &submission.request_id, &submission.text);
        let bytes = serde_json::to_vec(&request).map_err(|_| "Cannot encode this submission.")?;
        if bytes.len() + 1 > crate::host::REQUEST_LIMIT {
            return Err("Message exceeds the host's 64 KiB request limit. Shorten the draft before sending.".into());
        }
        self.pending
            .entry(thread_id.into())
            .or_default()
            .push(submission.clone());
        Ok(submission)
    }

    pub fn restore(&mut self, thread_id: String, pending: PendingSubmit) {
        let items = self.pending.entry(thread_id).or_default();
        if !items
            .iter()
            .any(|item| item.request_id == pending.request_id)
        {
            items.push(pending);
        }
    }

    pub fn all(&self, thread_id: &str) -> &[PendingSubmit] {
        self.pending.get(thread_id).map_or(&[], Vec::as_slice)
    }

    pub fn pending(&self, thread_id: &str) -> Option<&PendingSubmit> {
        self.all(thread_id).first()
    }

    pub fn get(&self, thread_id: &str, request_id: &str) -> Option<&PendingSubmit> {
        self.all(thread_id)
            .iter()
            .find(|p| p.request_id == request_id)
    }

    pub fn start_send(&mut self, thread_id: &str, request_id: &str) -> bool {
        self.get(thread_id, request_id).is_some() && self.sending.insert(request_id.into())
    }

    pub fn is_sending(&self, request_id: &str) -> bool {
        self.sending.contains(request_id)
    }

    pub fn uncertain(&mut self, request_id: &str, error: String) {
        self.sending.remove(request_id);
        self.errors.insert(request_id.into(), error);
    }

    pub fn error(&self, request_id: &str) -> Option<&str> {
        self.errors.get(request_id).map(String::as_str)
    }

    pub fn disconnected(&mut self) {
        self.sending.clear();
    }

    pub fn finish(&mut self, thread_id: &str, request_id: &str) -> Option<PendingSubmit> {
        let pending = self.pending.get_mut(thread_id)?;
        let index = pending.iter().position(|p| p.request_id == request_id)?;
        let finished = pending.remove(index);
        self.sending.remove(request_id);
        self.errors.remove(request_id);
        Some(finished)
    }

    pub fn accepted(&self, thread_id: &str) -> &[PendingSubmit] {
        self.accepted.get(thread_id).map_or(&[], Vec::as_slice)
    }

    pub fn restore_accepted(&mut self, thread_id: String, pending: PendingSubmit) {
        let items = self.accepted.entry(thread_id).or_default();
        if !items
            .iter()
            .any(|item| item.request_id == pending.request_id)
        {
            items.push(pending);
        }
    }

    pub fn confirm(&mut self, thread_id: &str, request_id: &str) -> Option<PendingSubmit> {
        let pending = self.finish(thread_id, request_id)?;
        let items = self.accepted.entry(thread_id.into()).or_default();
        items.push(pending.clone());
        // This cache is only for recovering exact queued text. Older host-owned
        // messages remain in history, not in the desktop journal.
        if items.len() > 16 {
            items.remove(0);
        }
        Some(pending)
    }

    pub fn forget_accepted(&mut self, thread_id: &str, request_id: &str) {
        if let Some(items) = self.accepted.get_mut(thread_id) {
            items.retain(|p| p.request_id != request_id);
        }
    }

    /// Both accepted messages and still-queued inputs acknowledge the exact request.
    pub fn observe(&mut self, snapshot: &Snapshot) -> Vec<PendingSubmit> {
        let accepted = self
            .all(&snapshot.id)
            .iter()
            .filter(|pending| {
                snapshot
                    .queued
                    .iter()
                    .any(|q| q.request_id == pending.request_id)
                    || snapshot
                        .messages
                        .iter()
                        .any(|m| m.id == format!("request-{}", pending.request_id))
            })
            .map(|pending| pending.request_id.clone())
            .collect::<Vec<_>>();
        let confirmed = accepted
            .into_iter()
            .filter_map(|id| self.confirm(&snapshot.id, &id))
            .collect();
        if let Some(items) = self.accepted.get_mut(&snapshot.id) {
            items.retain(|pending| {
                !snapshot
                    .messages
                    .iter()
                    .any(|m| m.id == format!("request-{}", pending.request_id))
            });
        }
        confirmed
    }
}

pub fn request_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "desktop-{}-{time}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(default: Option<ModelChoice>, models: Vec<crate::host::Model>) -> Status {
        Status {
            protocol_version: 1,
            default_model: default,
            models,
        }
    }

    fn snapshot(id: &str) -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "id": id, "checkout": "/repo",
            "model": {"provider":"p", "model_id":"saved"},
            "status":"idle", "messages":[], "live_text":"", "tools":[],
            "pending_approval":null, "error":null, "truncated":false
        }))
        .unwrap()
    }

    #[test]
    fn negotiated_protocol_accepts_versions_one_through_four_only() {
        let mut state = HostState::new("/repo".into());
        assert_eq!(state.protocol(), 1);
        for (version, accepted) in [
            (1, true),
            (2, true),
            (3, true),
            (4, true),
            (5, true),
            (6, false),
            (0, false),
        ] {
            state.reconnect();
            let mut reply = status(None, Vec::new());
            reply.protocol_version = version;
            assert_eq!(state.apply_status(state.epoch, reply), accepted);
            assert_eq!(state.connected, accepted);
            if accepted {
                assert_eq!(state.protocol(), version);
            }
        }
        // Submissions are sized as their largest negotiated form.
        let request = crate::host::Request::submit_bound("t", "r", "hi");
        let wire = serde_json::to_value(&request).unwrap();
        assert_eq!(wire["protocol_version"], 3);
        assert_eq!(wire["approval"], "auto");
        assert_eq!(wire["when_busy"], "follow_up");
        let legacy = crate::host::Request::submit("t".into(), "r".into(), "hi".into(), None);
        assert_eq!(
            serde_json::to_value(&legacy).unwrap(),
            serde_json::json!({"op":"submit","thread_id":"t","request_id":"r","text":"hi"})
        );
    }

    #[test]
    fn snapshots_replace_state_and_keep_exact_saved_model() {
        let mut state = HostState::new("/repo".into());
        let watch = state.select(Some("a".into()));
        assert!(state.apply_snapshot(0, watch, snapshot("a")));
        assert!(!state.apply_snapshot(0, watch, snapshot("b")));
        assert_eq!(state.snapshot.as_ref().unwrap().model.model_id, "saved");
        let mut wrong_model = snapshot("a");
        wrong_model.model.model_id = "new-default".into();
        assert!(!state.apply_snapshot(0, watch, wrong_model));
        assert_eq!(state.snapshot.as_ref().unwrap().model.model_id, "saved");
        let new_watch = state.select(Some("b".into()));
        assert!(!state.apply_snapshot(0, watch, snapshot("a")));
        assert!(state.apply_snapshot(0, new_watch, snapshot("b")));
        assert_eq!(state.snapshot.as_ref().unwrap().id, "b");
    }

    #[test]
    fn reconnect_preserves_uncertain_create_and_submit_ids() {
        let mut state = HostState::new("/repo".into());
        let original = state
            .submissions
            .begin("thread", "One task".into())
            .unwrap();
        state.pending_create = Some((
            "create-once".into(),
            ModelChoice {
                provider: "p".into(),
                model_id: "saved".into(),
            },
        ));
        state.reconnect();
        state.apply_status(state.epoch, status(None, vec![]));
        assert_eq!(state.submissions.pending("thread"), Some(&original));
        assert_eq!(state.pending_create.as_ref().unwrap().0, "create-once");
        assert!(!state.submitting);
        assert!(!state.creating);
    }

    #[test]
    fn stale_replies_and_unavailable_catalog_do_not_select_a_model() {
        let mut state = HostState::new("/repo".into());
        let old = state.reconnect();
        let current = state.reconnect();
        assert!(!state.apply_status(old, status(None, vec![])));
        assert!(state.apply_status(
            current,
            status(
                Some(ModelChoice {
                    provider: "p".into(),
                    model_id: "missing".into()
                }),
                vec![]
            )
        ));
        assert!(state.new_model.is_none());
        assert!(!state.available(state.new_model.as_ref()));
        let watch = state.select(Some("old".into()));
        state.select(Some("new".into()));
        assert!(!state.accepts_watch(current, watch, "old"));
        state.reconnect();
        assert!(!state.accepts_watch(current, state.watch_epoch, "new"));
    }

    #[test]
    fn saved_defaults_require_acknowledgement_and_do_not_replace_new_choice() {
        let choice = ModelChoice {
            provider: "p".into(),
            model_id: "m".into(),
        };
        let mut state = HostState::new("/repo".into());
        state.apply_status(
            0,
            status(
                Some(choice.clone()),
                vec![crate::host::Model {
                    provider: "p".into(),
                    model_id: "m".into(),
                    name: "Model".into(),
                    authenticated: true,
                    reasoning: None,
                    thinking_levels: None,
                    context_window: None,
                    input: None,
                }],
            ),
        );
        assert_eq!(state.new_model, Some(choice.clone()));
        state.saving = true;
        state.finish_save(Err("disk full".into()));
        assert_eq!(
            state.status.as_ref().unwrap().default_model,
            Some(choice.clone())
        );
        assert!(
            state
                .save_message
                .as_ref()
                .unwrap()
                .starts_with("Save not confirmed")
        );
        let other = ModelChoice {
            provider: "p".into(),
            model_id: "other".into(),
        };
        state.finish_save(Ok(other.clone()));
        assert_eq!(state.status.as_ref().unwrap().default_model, Some(other));
        assert_eq!(state.new_model, Some(choice));
    }

    #[test]
    fn oversized_encoded_submission_is_rejected_without_pending_work() {
        let mut state = Submissions::default();
        assert!(state.begin("thread", "\\".repeat(40_000)).is_err());
        assert!(state.pending("thread").is_none());
    }

    #[test]
    fn uncertain_submission_keeps_its_id_and_text() {
        let mut state = Submissions::default();
        let first = state.begin("thread", "Run one task".into()).unwrap();
        assert!(state.begin("thread", "Run another task".into()).is_err());
        assert_eq!(state.pending("thread"), Some(&first));
        assert!(state.finish("thread", "stale-reply").is_none());
        assert_eq!(state.pending("thread"), Some(&first));
        assert_eq!(state.finish("thread", &first.request_id), Some(first));
        assert!(state.pending("thread").is_none());
    }

    #[test]
    fn switching_threads_does_not_discard_an_uncertain_submission() {
        let mut state = Submissions::default();
        let first = state.begin("a", "Message A".into()).unwrap();
        let second = state.begin("b", "Message B".into()).unwrap();
        assert_ne!(first.request_id, second.request_id);
        state.finish("b", &second.request_id);
        assert_eq!(state.pending("a"), Some(&first));
        assert!(state.begin("c", " \n".into()).is_err());
    }
    #[test]
    fn many_pending_messages_keep_original_modes_and_acknowledge_out_of_order() {
        let mut state = Submissions::default();
        let a = state
            .begin_with(
                "t",
                "first".into(),
                Some(ApprovalMode::Ask),
                Some(QueueMode::FollowUp),
                Some(1),
            )
            .unwrap();
        let b = state
            .begin_with(
                "t",
                "second".into(),
                Some(ApprovalMode::Auto),
                Some(QueueMode::Steer),
                Some(2),
            )
            .unwrap();
        assert!(state.start_send("t", &a.request_id));
        assert!(!state.start_send("t", &a.request_id));
        state.uncertain(&a.request_id, "transport closed".into());
        assert_eq!(state.confirm("t", &b.request_id), Some(b));
        assert_eq!(state.pending("t"), Some(&a));
        assert!(
            state
                .begin_with("t", "edited".into(), None, Some(QueueMode::Steer), Some(1))
                .is_err()
        );
        let wire = serde_json::to_value(a.request("t")).unwrap();
        assert_eq!(wire["request_id"], a.request_id);
        assert_eq!(wire["approval"], "ask");
        assert_eq!(wire["when_busy"], "follow_up");
        assert!(state.start_send("t", &a.request_id));
        state.disconnected();
        assert!(!state.is_sending(&a.request_id));
        assert_eq!(state.pending("t"), Some(&a));
    }

    #[test]
    fn snapshot_queue_acknowledges_pending_and_keeps_full_local_text() {
        let mut state = Submissions::default();
        let a = state
            .begin_with(
                "a",
                "full text".into(),
                Some(ApprovalMode::Auto),
                Some(QueueMode::FollowUp),
                Some(1),
            )
            .unwrap();
        let mut current = snapshot("a");
        current.queued.push(crate::host::Queued {
            id: format!("request-{}", a.request_id),
            request_id: a.request_id.clone(),
            text: "tail".into(),
            mode: QueueMode::FollowUp,
            approval: ApprovalMode::Auto,
        });
        assert_eq!(state.observe(&current), vec![a.clone()]);
        assert!(state.pending("a").is_none());
        assert_eq!(state.accepted("a"), std::slice::from_ref(&a));
        assert!(state.observe(&current).is_empty());
        current.queued.clear();
        current.messages.push(crate::host::Message {
            id: format!("request-{}", a.request_id),
            ..Default::default()
        });
        state.observe(&current);
        assert!(state.accepted("a").is_empty());
        state.restore("a".into(), a.clone());
        state.restore("a".into(), a.clone());
        assert_eq!(state.all("a"), &[a]);
    }

    #[test]
    fn v3_watch_accepts_model_change_but_never_a_checkout_change() {
        let mut state = HostState::new("/repo".into());
        let mut catalog = status(None, Vec::new());
        catalog.protocol_version = 3;
        state.apply_status(0, catalog);
        let epoch = state.select(Some("a".into()));
        assert!(state.apply_snapshot(0, epoch, snapshot("a")));
        let mut changed = snapshot("a");
        changed.model.model_id = "other".into();
        assert!(state.apply_snapshot(0, epoch, changed.clone()));
        changed.checkout = "/different".into();
        assert!(!state.apply_snapshot(0, epoch, changed));
    }
    #[test]
    fn pending_limit_is_bounded_and_cancelled_cache_is_forgotten() {
        let mut state = Submissions::default();
        for revision in 0..16 {
            state
                .begin_with(
                    "t",
                    format!("message {revision}"),
                    Some(ApprovalMode::Ask),
                    Some(QueueMode::FollowUp),
                    Some(revision),
                )
                .unwrap();
        }
        assert!(
            state
                .begin_with("t", "overflow".into(), None, None, Some(16))
                .is_err()
        );
        assert_eq!(state.all("t").len(), 16);
        let request = state.pending("t").unwrap().request_id.clone();
        state.confirm("t", &request).unwrap();
        state.forget_accepted("t", &request);
        assert!(state.accepted("t").is_empty());
        assert!(
            state
                .begin_with("t", "next".into(), None, None, Some(17))
                .is_ok()
        );
    }
}
