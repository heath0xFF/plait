use super::{
    Plait,
    composer::{Composer, ComposerEvent},
};
use gpui::{prelude::*, *};
use plait_core::{
    host::{self, Client, Request, Snapshot},
    host_state::request_id,
    transcript::{self, Update},
};
use serde::de::DeserializeOwned;

pub(super) struct WatchLease {
    pub(super) cancel: Option<host::CancelWatch>,
    pub(super) executor: BackgroundExecutor,
}
impl Drop for WatchLease {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            self.executor
                .spawn(async move {
                    drop(cancel);
                })
                .detach();
        }
    }
}

impl Plait {
    pub(super) fn host_request<T: DeserializeOwned + Send + 'static>(
        &mut self,
        mut request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
        complete: impl FnOnce(&mut Self, Result<T, host::Error>, &mut Window, &mut Context<Self>)
        + 'static,
    ) {
        let epoch = self.host.epoch;
        let persist = matches!(request, Request::Submit { .. }) && self.session.required;
        if persist {
            self.capture_session(cx);
        }
        let journal = self.session.journal.clone();
        let task = cx.background_executor().spawn(async move {
            if persist {
                let journal = journal.ok_or_else(|| {
                    std::io::Error::other(
                        "Desktop recovery is unavailable. Submission was not sent.",
                    )
                })?;
                journal.flush().map_err(std::io::Error::other)?;
            }
            if let Request::ListThreads { checkout } | Request::CreateThread { checkout, .. } =
                &mut request
            {
                *checkout = std::fs::canonicalize(&*checkout)?;
            }
            Client::from_environment()?.request(&request)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.host.epoch != epoch {
                    return;
                }
                complete(this, result, window, cx);
                this.sync_composer(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn reconnect_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.host.connecting {
            return;
        }
        self.host_watch = None;
        self.history = Default::default();
        self.navigation.watch = None;
        self.navigation.generation += 1;
        self.navigation.loading = false;
        if self.navigation.managing.take().is_some() {
            self.navigation.error =
                Some("Thread change not confirmed. Refresh to check before trying again.".into());
        }
        self.host.reconnect();
        self.sync_composer(cx);
        self.host_request(
            Request::Status {
                protocol_version: Some(host::PROTOCOL_VERSION),
            },
            window,
            cx,
            |this, result, window, cx| match result {
                Ok(status) => {
                    if this.host.apply_status(this.host.epoch, status) {
                        this.load_threads(window, cx);
                        if let Some(id) = this.host.selected.clone() {
                            this.watch_thread(id, window, cx);
                        }
                    }
                }
                Err(error) => {
                    this.host.connecting = false;
                    this.host.error = Some(error.to_string());
                }
            },
        );
        cx.notify();
    }

    pub(super) fn load_threads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.host.connected {
            return;
        }
        if self.host.protocol() >= 3 {
            self.watch_all_threads(window, cx);
            return;
        }
        let checkout = self.host.checkout.clone();
        self.host_request::<host::ThreadsReply>(
            Request::ListThreads { checkout },
            window,
            cx,
            |this, result, window, cx| match result {
                Ok(reply) => {
                    this.host.threads = reply.threads;
                    this.rebuild_projects(cx);
                    this.restore_thread_selection(window, cx);
                }
                Err(error) => this.host.error = Some(error.to_string()),
            },
        );
    }

    pub(super) fn select_host_thread(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(checkout) = self
            .host
            .threads
            .iter()
            .find(|thread| thread.id == id)
            .map(|thread| thread.checkout.clone())
        else {
            return;
        };
        if checkout != self.host.checkout {
            if self.host.creating || self.host.pending_create.is_some() {
                self.notice =
                    Some("Resolve the pending thread creation before changing checkout.".into());
                cx.notify();
                return;
            }
            self.change_checkout(checkout, window, cx);
        }
        self.mark_thread_read(&id, cx);
        self.navigation.menu = None;
        self.navigation.rename = None;
        window.focus(&self.focus, cx);
        self.host_watch = None;
        self.host.select(Some(id.clone()));
        self.history = Default::default();
        self.settings_open = false;
        self.new_thread_open = false;
        self.show_changes = false;
        self.refresh_live_list(None);
        self.formatted = Default::default();
        if !self.composers.contains_key(&id) {
            let composer = cx.new(|cx| Composer::new(window, cx));
            self.restore_composer(&id, &composer, cx);
            cx.observe(&composer, |this, _, cx| this.capture_session(cx))
                .detach();
            let thread_id = id.clone();
            cx.subscribe_in(&composer, window, move |this, _, event, window, cx| {
                if this.host.selected.as_ref() != Some(&thread_id) {
                    return;
                }
                match event {
                    ComposerEvent::Submit {
                        text,
                        revision,
                        steer,
                    } => this.submit_message(text.clone(), *revision, *steer, window, cx),
                    ComposerEvent::Stop => this.control_thread(None, window, cx),
                    other => this.composer_event(other, window, cx),
                }
            })
            .detach();
            self.composers.insert(id.clone(), composer);
        }
        self.capture_session(cx);
        self.sync_composer(cx);
        if self.host.connected {
            self.watch_thread(id, window, cx);
        }
        cx.notify();
    }

    fn watch_thread(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.host_watch = None;
        self.host.watch_epoch += 1;
        let epoch = self.host.epoch;
        let watch_epoch = self.host.watch_epoch;
        let watch_id = id.clone();
        let protocol = self.host.protocol();
        let executor = cx.background_executor().clone();
        let task = cx.background_executor().spawn(async move {
            let snapshots = Client::from_environment()?.subscribe(watch_id, protocol)?;
            let lease = WatchLease {
                cancel: Some(snapshots.cancel_handle()),
                executor,
            };
            Ok::<_, host::Error>((snapshots, lease))
        });
        cx.spawn_in(window, async move |this, cx| {
            let (mut snapshots, lease) = match task.await {
                Ok(subscription) => subscription,
                Err(error) => {
                    let _ = this.update_in(cx, |this, _, cx| {
                        this.watch_failed(epoch, watch_epoch, &id, error, cx)
                    });
                    return;
                }
            };
            let registered = this
                .update_in(cx, |this, _, _| {
                    if !this.host.accepts_watch(epoch, watch_epoch, &id) {
                        return false;
                    }
                    this.host_watch = Some(lease);
                    true
                })
                .unwrap_or(false);
            if !registered {
                cx.background_executor()
                    .spawn(async move {
                        drop(snapshots);
                    })
                    .detach();
                return;
            }
            while let Some(result) = snapshots.next().await {
                let keep = this
                    .update_in(cx, |this, window, cx| {
                        if !this.host.accepts_watch(epoch, watch_epoch, &id) {
                            return false;
                        }
                        match result {
                            Ok(snapshot) => {
                                if snapshot.id != id {
                                    this.watch_failed(
                                        epoch,
                                        watch_epoch,
                                        &id,
                                        host::Error::Protocol(
                                            "Host returned the wrong thread.".into(),
                                        ),
                                        cx,
                                    );
                                    return false;
                                }
                                let previous = this.host.snapshot.clone();
                                if !this.host.apply_snapshot(epoch, watch_epoch, snapshot) {
                                    this.host_watch = None;
                                    this.sync_composer(cx);
                                    cx.notify();
                                    return false;
                                }
                                this.conversation_changed(window, cx);
                                this.record_snapshot(cx);
                                if previous
                                    .as_ref()
                                    .is_some_and(|snapshot| snapshot.status.busy())
                                    && this
                                        .host
                                        .snapshot
                                        .as_ref()
                                        .is_some_and(|snapshot| !snapshot.status.busy())
                                {
                                    this.state.repository.refresh();
                                    this.pump(cx);
                                }
                                this.sync_composer(cx);
                                cx.notify();
                                true
                            }
                            Err(error) => {
                                this.watch_failed(epoch, watch_epoch, &id, error, cx);
                                false
                            }
                        }
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
            cx.background_executor()
                .spawn(async move {
                    drop(snapshots);
                })
                .detach();
        })
        .detach();
    }

    fn watch_failed(
        &mut self,
        epoch: u64,
        watch_epoch: u64,
        id: &str,
        error: host::Error,
        cx: &mut Context<Self>,
    ) {
        if !self.host.accepts_watch(epoch, watch_epoch, id) {
            return;
        }
        self.host_watch = None;
        self.host.connected = false;
        self.host.error = Some(error.to_string());
        self.sync_composer(cx);
        cx.notify();
    }

    pub(super) fn refresh_live_list(&mut self, previous: Option<&Snapshot>) {
        self.history.display = self.host.snapshot.clone();
        self.reconcile_live_list(previous);
    }

    pub(super) fn reconcile_live_list(&mut self, previous: Option<&Snapshot>) {
        let next = self.history.display.as_deref();
        match transcript::changes(previous, next) {
            Update::Reset(count) => {
                self.live_scroll.reset(count);
                self.expanded_live_tools.clear();
                self.history.expanded.clear();
            }
            Update::Patch { splices, remeasure } => {
                for splice in splices {
                    self.live_scroll.splice(splice.range, splice.count);
                }
                for range in remeasure {
                    self.live_scroll.remeasure_items(range);
                }
                self.history.expanded.retain(|id| {
                    next.is_some_and(|snapshot| {
                        snapshot.messages.iter().any(|message| message.id == *id)
                    })
                });
                self.expanded_live_tools.retain(|id| {
                    next.is_some_and(|snapshot| snapshot.tools.iter().any(|tool| tool.id == *id))
                });
            }
        }
        // Cached once per snapshot; rendering resolves visible rows by position.
        self.live_layout = next.map(transcript::layout).unwrap_or_default();
        self.live_count = self.live_layout.len();
        debug_assert_eq!(self.live_scroll.item_count(), self.live_count);
    }

    pub(super) fn create_host_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.host.connected || self.host.creating {
            return;
        }
        if self.host.pending_create.is_none() {
            let Some(choice) = self.host.new_model.clone() else {
                return;
            };
            if !self.host.available(Some(&choice)) {
                return;
            }
            self.host.pending_create = Some((request_id(), choice));
        }
        let (id, choice) = self.host.pending_create.clone().unwrap();
        self.host.creating = true;
        self.host.error = None;
        let selection_epoch = self.host.watch_epoch;
        self.host_request::<host::ThreadReply>(Request::CreateThread {
            checkout: self.host.checkout.clone(), provider: choice.provider.clone(), model_id: choice.model_id.clone(), request_id: id,
        }, window, cx, move |this, result, window, cx| {
            this.host.creating = false;
            match result {
                Ok(reply) if reply.thread.model == choice => {
                    this.host.pending_create = None;
                    if !this.host.threads.iter().any(|thread| thread.id == reply.thread.id) {
                        this.host.threads.insert(0, host::ThreadSummary::from(&reply.thread));
                    }
                    if this.new_thread_open && this.host.watch_epoch == selection_epoch {
                        this.select_host_thread(reply.thread.id, window, cx);
                        this.focus_composer(window, cx);
                    }
                    this.record_recent_model(&choice, cx);
                    this.load_threads(window, cx);
                }
                Ok(_) => this.host.error = Some("Host returned a different model. Creation is unconfirmed; reconnect before retrying.".into()),
                Err(error) => {
                    if matches!(error, host::Error::Rejected(_)) {
                        this.host.pending_create = None;
                        this.host.error = Some(error.to_string());
                    } else {
                        this.host.error = Some(format!("{error} Retry creation to reuse the same request ID."));
                    }
                }
            }
        });
        cx.notify();
    }

    fn submit_message(
        &mut self,
        text: String,
        revision: u64,
        steer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.host.selected.clone() else {
            return;
        };
        if !self.can_submit() {
            return;
        }
        let mode = (self.host.protocol() >= 3).then_some(if steer {
            host::QueueMode::Steer
        } else {
            host::QueueMode::FollowUp
        });
        match self.host.submissions.begin_with(
            &id,
            text,
            self.next_approval_mode(),
            mode,
            Some(revision),
        ) {
            Ok(pending) => self.retry_submission_id(&id, &pending.request_id, window, cx),
            Err(error) => {
                self.host.error = Some(error);
                cx.notify();
            }
        }
    }

    pub(super) fn retry_submission(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.host.selected.clone() else {
            return;
        };
        let Some(pending) = self.host.submissions.pending(&id).cloned() else {
            return;
        };
        self.retry_submission_id(&id, &pending.request_id, window, cx);
    }

    pub(super) fn retry_submission_id(
        &mut self,
        id: &str,
        request_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.host.connected || !self.session.ready() || !self.session_accepts_thread(id) {
            return;
        }
        let Some(pending) = self.host.submissions.get(id, request_id).cloned() else {
            return;
        };
        if (pending.when_busy.is_some() && self.host.protocol() < 3)
            || (pending.approval.is_some() && self.host.protocol() < 2)
        {
            self.host.error =
                Some("This message needs a newer host. Update before retrying.".into());
            return;
        }
        if !self.host.submissions.start_send(id, request_id) {
            return;
        }
        let id = id.to_owned();
        let composer = self.composers.get(&id).cloned();
        self.host.submitting = true;
        self.host.error = None;
        self.sync_composer(cx);
        self.host_request::<host::SubmitReply>(
            pending.request(&id),
            window,
            cx,
            move |this, result, _, cx| {
                match result {
                    Ok(reply)
                        if reply.accepted
                            && reply.thread_id == id
                            && reply.request_id == pending.request_id =>
                    {
                        this.host.submissions.confirm(&id, &pending.request_id);
                        if let Some(composer) = composer {
                            composer.update(cx, |composer, cx| {
                                composer.acknowledge(&pending.text, pending.draft_revision, cx);
                            });
                        }
                    }
                    Ok(_) => this.host.submissions.uncertain(
                        &pending.request_id,
                        "Host acceptance was not confirmed. Retry uses the same request ID.".into(),
                    ),
                    Err(error) => {
                        if matches!(error, host::Error::Rejected(_)) {
                            this.host.submissions.finish(&id, &pending.request_id);
                            this.set_thread_notice(id.clone(), error.to_string(), true);
                        } else {
                            this.host
                                .submissions
                                .uncertain(&pending.request_id, error.to_string());
                        }
                    }
                }
                this.host.submitting = this.host.selected.as_ref().is_some_and(|selected| {
                    this.host
                        .submissions
                        .all(selected)
                        .iter()
                        .any(|p| this.host.submissions.is_sending(&p.request_id))
                });
                this.capture_session(cx);
            },
        );
        cx.notify();
    }

    pub(super) fn control_thread(
        &mut self,
        approval: Option<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.host.connected || self.host.controlling {
            return;
        }
        let Some(snapshot) = self.host.snapshot.as_ref() else {
            return;
        };
        let id = snapshot.id.clone();
        let request = if let Some((approval_id, allow)) = approval {
            if self.host.decided_approvals.contains(&approval_id)
                || !snapshot.pending_approval.as_ref().is_some_and(|pending| {
                    pending.id == approval_id
                        && !pending.details.is_empty()
                        && pending.details.len() <= 16384
                })
            {
                return;
            }
            self.host.decided_approvals.insert(approval_id.clone());
            Request::Approve {
                thread_id: id.clone(),
                approval_id,
                allow,
            }
        } else {
            if !snapshot.status.busy() {
                return;
            }
            Request::Stop {
                thread_id: id.clone(),
            }
        };
        self.host.controlling = true;
        let watch_epoch = self.host.watch_epoch;
        self.host_request::<host::ThreadReply>(request, window, cx, move |this, result, _, _| {
            if !this.host.accepts_watch(this.host.epoch, watch_epoch, &id) {
                return;
            }
            this.host.controlling = false;
            match result {
                // Watch owns snapshot order; a command reply can arrive after a newer event.
                Ok(reply) if reply.thread.id == id => {}
                Ok(_) => {
                    this.host.error =
                        Some("Host returned the wrong thread for this control request.".into())
                }
                Err(error) => this.host.error = Some(error.to_string()),
            }
        });
        self.sync_composer(cx);
        cx.notify();
    }

    pub(super) fn save_default_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.host.saving || !self.host.available(self.host.settings_model.as_ref()) {
            return;
        }
        let choice = self.host.settings_model.clone().unwrap();
        self.host.saving = true;
        self.host.save_message = Some("Saving default model…".into());
        self.host_request::<host::DefaultReply>(
            Request::SetDefaultModel {
                provider: choice.provider.clone(),
                model_id: choice.model_id.clone(),
            },
            window,
            cx,
            move |this, result, _, _| {
                let result = match result {
                    Ok(reply) if reply.default_model == choice => Ok(reply.default_model),
                    Ok(_) => Err(
                        "Host returned a different model; reconnect to check the saved default."
                            .into(),
                    ),
                    Err(error) => Err(error.to_string()),
                };
                this.host.finish_save(result);
            },
        );
        cx.notify();
    }

    fn can_submit(&self) -> bool {
        self.session.ready()
            && self.navigation.available.get(&self.host.checkout) != Some(&false)
            && self.host.connected
            && !self.host.controlling
            && !self.thread_configuration_pending()
            && self.host.snapshot.as_ref().is_some_and(|snapshot| {
                (self.host.protocol() >= 3
                    || (!snapshot.status.busy()
                        && self.host.submissions.pending(&snapshot.id).is_none()))
                    && self.session_accepts_thread(&snapshot.id)
                    && self.host.available(Some(&snapshot.model))
                    && snapshot.queued.len() + self.host.submissions.all(&snapshot.id).len() < 16
            })
    }

    pub(super) fn sync_composer(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.host.selected.as_ref() else {
            return;
        };
        let Some(composer) = self.composers.get(id) else {
            return;
        };
        let busy = self
            .host
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.status.busy());
        let reason = if self.navigation.available.get(&self.host.checkout) == Some(&false) {
            Some("Checkout folder unavailable. Open its current location before sending.".into())
        } else if !self.session.ready() {
            Some("Restoring saved work…".into())
        } else if !self.session_accepts_thread(id) {
            Some("Thread checkout differs from the saved draft. Original work is preserved.".into())
        } else if !self.host.connected {
            Some("Host disconnected. Reconnect to send.".into())
        } else if self.host.snapshot.is_none() {
            Some("Loading saved thread…".into())
        } else if self.host.protocol() < 3
            && (self.host.submitting || self.host.submissions.pending(id).is_some())
        {
            Some("Message unconfirmed · Retry safely above.".into())
        } else if !self
            .host
            .available(self.host.snapshot.as_ref().map(|snapshot| &snapshot.model))
        {
            Some(
                if self.host.protocol() >= 3 {
                    "Model unavailable · choose another model below."
                } else {
                    "Model unavailable · check Settings."
                }
                .into(),
            )
        } else {
            None
        };
        let send_blocked = if self.thread_configuration_pending() {
            Some("Saving model settings…".into())
        } else if self
            .host
            .snapshot
            .as_ref()
            .is_some_and(|s| s.queued.len() + self.host.submissions.all(id).len() >= 16)
        {
            Some("Queue full · cancel a message to send another.".into())
        } else {
            None
        };
        composer.update(cx, |composer, cx| {
            composer.set_send_blocked(send_blocked, cx);
            composer.set_enabled(reason.is_none() && !self.host.controlling, reason, cx);
            composer.set_busy(busy, cx);
            composer.set_queue_enabled(
                self.host.protocol() >= 3,
                self.host.connected && !self.host.controlling,
                cx,
            );
        });
        self.sync_prompt_history(cx);
    }
}
