use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::host::{ModelChoice, Snapshot, Status, ThreadSummary};
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
        if status.protocol_version != 1 {
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
            *model != snapshot.model || *checkout != snapshot.checkout
        }) {
            self.connected = false;
            self.error = Some("Host changed this thread's saved model or checkout. Reconnect to reload its identity.".into());
            return false;
        }
        if let Some(thread) = self
            .threads
            .iter_mut()
            .find(|thread| thread.id == snapshot.id)
        {
            thread.status = snapshot.status;
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingSubmit {
    pub request_id: String,
    pub text: String,
}

#[derive(Default)]
pub struct Submissions {
    pending: HashMap<String, PendingSubmit>,
}

impl Submissions {
    pub fn begin(&mut self, thread_id: &str, text: String) -> Result<PendingSubmit, String> {
        if text.trim().is_empty() {
            return Err("Enter a message before sending.".into());
        }
        if self.pending.contains_key(thread_id) {
            return Err("Resolve the previous submission before sending another message.".into());
        }
        let submission = PendingSubmit {
            request_id: request_id(),
            text,
        };
        let request = crate::host::Request::Submit {
            thread_id: thread_id.into(),
            request_id: submission.request_id.clone(),
            text: submission.text.clone(),
        };
        let bytes = serde_json::to_vec(&request).map_err(|_| "Cannot encode this submission.")?;
        if bytes.len() + 1 > crate::host::REQUEST_LIMIT {
            return Err("Message exceeds the host's 64 KiB request limit. Shorten the draft before sending.".into());
        }
        self.pending.insert(thread_id.into(), submission.clone());
        Ok(submission)
    }

    pub fn restore(&mut self, thread_id: String, pending: PendingSubmit) {
        // Loading disk state must not replace a newer in-memory submission.
        self.pending.entry(thread_id).or_insert(pending);
    }

    pub fn pending(&self, thread_id: &str) -> Option<&PendingSubmit> {
        self.pending.get(thread_id)
    }

    pub fn finish(&mut self, thread_id: &str, request_id: &str) -> Option<PendingSubmit> {
        if self.pending.get(thread_id)?.request_id != request_id {
            return None;
        }
        self.pending.remove(thread_id)
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
}
