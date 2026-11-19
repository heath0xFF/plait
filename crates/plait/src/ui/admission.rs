//! Placement is negotiated off-thread before creation; replies retain their owner.
use super::{Plait, host::Created, theme::*};
use gpui::{prelude::*, *};
use plait_core::host::{
    self, Client,
    admission::{self, Placement, Request, Stage},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct State {
    pub origin: Option<(String, Option<String>)>,
    pub preparing: Option<(String, Option<admission::Phase>)>,
    pub capabilities: Option<(u64, PathBuf, bool)>,
    pub continuation: Option<String>,
    pub pending: Option<Request>,
    pub choice: bool,
    pub source: Option<String>,
    pub settings_key: Option<(u64, PathBuf)>,
    pub info: Option<admission::Info>,
    pub settings_busy: bool,
    pub note: Option<String>,
}
impl Plait {
    pub(super) fn sync_admission_context(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || !self.host.connection.connected {
            return;
        }
        let Some(snapshot) = &self.host.thread.snapshot else {
            return;
        };
        if !self
            .admission
            .capabilities
            .as_ref()
            .is_some_and(|(epoch, root, yes)| {
                *yes && *epoch == self.host.connection.epoch && root == &snapshot.checkout
            })
        {
            return;
        }
        let id = snapshot.id.clone();
        if !snapshot.status.busy() {
            self.admission.preparing = None;
        } else if self
            .admission
            .preparing
            .as_ref()
            .is_none_or(|(thread, _)| thread != &id)
        {
            self.admission.preparing = Some((id.clone(), None));
            self.poll_preparation(id.clone(), false, cx);
        }
        if self
            .admission
            .origin
            .as_ref()
            .is_some_and(|(thread, _)| thread == &id)
        {
            return;
        }
        self.admission.origin = Some((id.clone(), None));
        let epoch = self.host.connection.epoch;
        let thread = id.clone();
        let task = cx
            .background_executor()
            .spawn(async move { admission::origin(&Client::from_environment()?, thread) });
        cx.spawn(async move |this, cx| {
            if let Ok(origin) = task.await {
                let _ = this.update(cx, |this, cx| {
                    if this.host.connection.epoch == epoch
                        && this.host.thread.selected.as_ref() == Some(&id)
                    {
                        this.admission.origin = Some((id, origin.and_then(|o| o.source_thread_id)));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }
    fn poll_preparation(&mut self, id: String, delay: bool, cx: &mut Context<Self>) {
        let epoch = self.host.connection.epoch;
        let thread = id.clone();
        let task = cx.background_executor().spawn(async move {
            if delay {
                std::thread::sleep(Duration::from_millis(300));
            }
            admission::preparation(&Client::from_environment()?, thread)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.host.connection.epoch != epoch
                    || this.host.thread.selected.as_ref() != Some(&id)
                    || this
                        .admission
                        .preparing
                        .as_ref()
                        .is_none_or(|(thread, _)| thread != &id)
                {
                    return;
                }
                let phase = result.ok().flatten();
                this.admission.preparing = Some((id.clone(), phase));
                if matches!(
                    phase,
                    Some(admission::Phase::Preparing | admission::Phase::ResourceWait)
                ) {
                    this.poll_preparation(id, true, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn preparation_label(&self) -> Option<&'static str> {
        let snapshot = self.host.thread.snapshot.as_ref()?;
        let (id, phase) = self.admission.preparing.as_ref()?;
        if id != &snapshot.id || !snapshot.status.busy() {
            return None;
        }
        match phase {
            Some(admission::Phase::Preparing) => Some("Preparing checkout…"),
            Some(admission::Phase::ResourceWait) => Some("Waiting for preparation…"),
            _ => None,
        }
    }
    pub(super) fn admission_context(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let selected = self.host.thread.selected.as_ref()?;
        let mut content = column().gap_1();
        let mut shown = false;
        if let Some((id, Some(source))) = &self.admission.origin
            && id == selected
        {
            let source = source.clone();
            shown = true;
            content = content.child(
                button("linked-origin", "Original thread")
                    .aria_description(
                        "Open the original thread. Its history and draft remain there.",
                    )
                    .on_activate(cx, move |this, window, cx| {
                        this.select_host_thread(source.clone(), window, cx)
                    }),
            );
        }
        if let Some((id, Some(phase))) = &self.admission.preparing
            && id == selected
        {
            let label = match phase {
                admission::Phase::Preparing => {
                    Some("Preparing checkout… The agent turn has not started.")
                }
                admission::Phase::ResourceWait => Some(
                    "Waiting for this filesystem's preparation scan. The agent turn has not started; Stop cancels preparation.",
                ),
                admission::Phase::Running => None,
            };
            if let Some(label) = label {
                shown = true;
                content = content.child(tag("admission-preparation-detail", label, colors().muted));
            }
        }
        shown.then(|| content.into_any_element())
    }
    pub(super) fn sync_admission_capability(&mut self, cx: &mut Context<Self>) {
        if cfg!(test) || !self.host.connection.connected {
            return;
        }
        let epoch = self.host.connection.epoch;
        let root = self.host.creation.checkout.clone();
        if self
            .admission
            .capabilities
            .as_ref()
            .is_some_and(|(e, p, _)| *e == epoch && p == &root)
        {
            return;
        }
        self.admission.capabilities = Some((epoch, root.clone(), false));
        let target = root.clone();
        let task = cx
            .background_executor()
            .spawn(async move { admission::info(&Client::from_environment()?, target) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.host.connection.epoch == epoch && this.host.creation.checkout == root {
                    this.admission.capabilities =
                        Some((epoch, root, result.is_ok_and(|info| info.is_some())));
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(super) fn continuation_control(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let snapshot = self.host.thread.snapshot.as_ref()?;
        if !self
            .admission
            .capabilities
            .as_ref()
            .is_some_and(|(epoch, root, yes)| {
                *yes && *epoch == self.host.connection.epoch && root == &snapshot.checkout
            })
            || snapshot.status.busy()
            || self.host.creation.pending_create.is_some()
        {
            return None;
        }
        let busy = self
            .host
            .connection
            .threads
            .iter()
            .any(|thread| thread.checkout == snapshot.checkout && thread.status.busy());
        if !busy {
            return None;
        }
        if self.admission.continuation.as_ref() == Some(&snapshot.id) {
            return Some(column().gap_2().child(tag("admission-continuation-detail", "Starts a linked thread from HEAD. Uncommitted files are not copied; this thread's history and draft stay here.",colors().muted))
                .child(row().gap_2().child(button("continue-worktree-confirm","Own worktree").on_activate(cx,|this,window,cx| this.start_continuation(window,cx)))
                    .child(button("continue-worktree-cancel","Cancel").on_activate(cx,|this,_,cx| {this.admission.continuation=None;cx.notify();})))
                .into_any_element());
        }
        Some(row().gap_2().child(button("continue-worktree","Continue separately")
            .tooltip(|_,cx| cx.new(|_| super::thread_controls::TextTooltip("Create a linked thread in a new worktree from HEAD. This thread and its draft stay here.".into())).into())
            .on_activate(cx,|this,_,cx| {this.admission.continuation=this.host.thread.selected.clone();cx.notify();}))
            .child(button("continue-wait","Wait").disabled(true).aria_description("Not yet available. Nothing is queued."))
            .into_any_element())
    }
    fn start_continuation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.host.thread.snapshot.as_ref() else {
            return;
        };
        if self.admission.continuation.as_ref() != Some(&snapshot.id)
            || snapshot.status.busy()
            || self.host.creation.pending_create.is_some()
        {
            return;
        }
        let snapshot = snapshot.clone();
        let harness = super::harnesses::harness_of(&snapshot);
        let mode = snapshot.harness.as_ref().and_then(|h| h.mode.clone());
        let request_id = plait_core::host_state::request_id();
        self.admission.source = Some(snapshot.id.clone());
        self.admission.continuation = None;
        self.admission.pending = Some(Request::CreateAdmittedThread {
            request_id: request_id.clone(),
            checkout: snapshot.checkout.clone(),
            placement: Placement::Worktree,
            harness,
            provider: snapshot.model.provider.clone(),
            model_id: snapshot.model.model_id.clone(),
            approval: self.next_approval_mode().unwrap_or(host::ApprovalMode::Ask),
            mode: mode.clone(),
            thinking: snapshot.thinking_level,
            source_thread_id: Some(snapshot.id.clone()),
        });
        self.host.creation.pending_create = Some((request_id, snapshot.model.clone()));
        self.host.creation.pending_thinking = snapshot.thinking_level;
        self.set_pending_harness(harness, mode);
        self.new_thread_open = true;
        self.create_host_thread(window, cx);
    }
    pub(super) fn create_with_admission(
        &mut self,
        created: Created,
        legacy: host::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request =
            self.admission
                .pending
                .clone()
                .unwrap_or_else(|| Request::CreateAdmittedThread {
                    request_id: created.request_id.clone(),
                    checkout: self.host.creation.checkout.clone(),
                    placement: if self.admission.source.is_some() {
                        Placement::Worktree
                    } else {
                        Placement::Project
                    },
                    harness: created.harness,
                    provider: created.choice.provider.clone(),
                    model_id: created.choice.model_id.clone(),
                    approval: self.draft_approval(),
                    mode: self.pending_mode(),
                    thinking: self.host.creation.pending_thinking,
                    source_thread_id: self.admission.source.clone(),
                });
        self.admission.pending = Some(request.clone());
        self.admission.choice = false;
        let epoch = self.host.connection.epoch;
        let expected = self.state.checkout_key().cloned();
        let task = cx.background_executor().spawn(async move {
            if let Some(expected) = expected {
                expected.verify().map_err(host::Error::Protocol)?;
            }
            let client = Client::from_environment()?;
            let Request::CreateAdmittedThread {
                checkout,
                source_thread_id,
                harness,
                provider,
                model_id,
                ..
            } = &request
            else {
                unreachable!()
            };
            if admission::info(&client, checkout.clone())?.is_none() {
                if source_thread_id.is_some() {
                    return Err(host::Error::Protocol(
                        "Linked worktree threads need a newer host.".into(),
                    ));
                }
                let reply: host::ThreadReply = client.request(&legacy)?;
                return Ok((None, Some(reply)));
            }
            let receipt = await_admission(&client, &request)?;
            let reply = if receipt.state == Stage::Ready {
                let thread = receipt
                    .thread_id
                    .clone()
                    .ok_or_else(|| host::Error::Protocol("Admission omitted its thread.".into()))?;
                let reply: host::ThreadReply = client.request(&host::Request::Watch {
                    thread_id: thread.clone(),
                    protocol_version: Some(host::PROTOCOL_VERSION),
                })?;
                if reply.thread.id != thread
                    || reply.thread.checkout != receipt.checkout
                    || super::harnesses::harness_of(&reply.thread) != *harness
                    || reply.thread.model.provider != *provider
                    || reply.thread.model.model_id != *model_id
                {
                    return Err(host::Error::Protocol(
                        "Admission returned a different thread, checkout, model or harness.".into(),
                    ));
                }
                Some(reply)
            } else {
                None
            };
            Ok::<_, host::Error>((Some(receipt), reply))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.host.connection.epoch != epoch || this.host.creation.pending_create.as_ref().map(|(id,_)| id) != Some(&created.request_id) { return; }
                match result {
                    Ok((Some(receipt), _)) if receipt.state == Stage::Choice => {
                        this.host.creation.creating = false;
                        this.admission.choice = true;
                        this.sync_composer(cx);
                    }
                    Ok((receipt, Some(reply))) => {
                        let mut created = created;
                        if let Some(receipt) = receipt
                            && let Some(tree) = receipt.worktree {
                                if let Some(identity) = &tree.identity {
                                    this.tabs.identity.checkouts.insert(tree.path.clone(), plait_core::tabs::DirectoryIdentity {
                                        device: identity.dev.parse().unwrap(), inode: identity.ino.parse().unwrap(),
                                    });
                                }
                                this.notice = Some(format!("Worktree: {} · {}. Starts from HEAD; original uncommitted files and build output were not copied.", tree.path.display(), tree.branch));
                                created.checkout = Some(tree.path.clone());
                                // The Stage 7 receipt is the deletion authority, never the admission ID.
                                this.refresh_host_worktrees(cx);
                        }
                        if let Some(source) = this.admission.source.clone() {
                            this.admission.origin = Some((reply.thread.id.clone(), Some(source)));
                        }
                        let policy = match &this.admission.pending {
                            Some(Request::CreateAdmittedThread { harness: host::harness::HarnessId::Pi, source_thread_id: Some(_), approval, .. }) => Some(*approval),
                            _ => None,
                        };
                        let id=reply.thread.id.clone();
                        this.admission.pending = None; this.admission.source = None;
                        this.finish_create(created, Ok(reply), window, cx);
                        if let Some(policy)=policy {
                            if !this.workspace.composers.contains_key(&id) {
                                let composer=cx.new(|cx| super::composer::Composer::new(window,cx));
                                this.attach_composer(id.clone(),composer,window,cx);
                            }
                            this.set_thread_approval(id,Some(policy),cx);
                        }
                    }
                    Ok((Some(receipt), None)) => {
                        let error = receipt.error.map(host::Error::Rejected).unwrap_or_else(|| host::Error::Protocol("Creation is still preparing. Retry with the same request.".into()));
                        if matches!(&error, host::Error::Rejected(_)) { this.admission.pending = None; this.admission.source = None; }
                        this.finish_create(created, Err(error), window, cx);
                    }
                    Err(error) => {
                        if matches!(&error, host::Error::Rejected(_)) { this.admission.pending = None; this.admission.source = None; }
                        this.finish_create(created, Err(error), window, cx);
                    }
                    _ => {}
                }
                cx.notify();
            });
        }).detach();
    }
    /// The draft's admission the host may already have made, for the saved draft.
    pub(super) fn draft_creation(&self) -> Option<plait_core::session::DraftCreation> {
        let sending = self.draft.sending.as_ref()?;
        // A choice records nothing on the host; there is nothing to retransmit.
        if self.admission.choice || !self.draft_pending() {
            return None;
        }
        let request = self.admission.pending.clone()?;
        matches!(&request, Request::CreateAdmittedThread { request_id, .. } if *request_id == sending.request_id)
            .then(|| plait_core::session::DraftCreation {
                request,
                approval: sending.approval,
                choice: sending.choice,
            })
    }
    /// A reopened draft waits on the creation it was sent with, so sending it
    /// again reaches the thread and worktree the host may already have made.
    pub(super) fn restore_draft_creation(
        &mut self,
        creation: plait_core::session::DraftCreation,
        draft: &plait_core::session::Draft,
    ) {
        let Request::CreateAdmittedThread {
            request_id,
            checkout,
            harness,
            provider,
            model_id,
            mode,
            thinking,
            source_thread_id: None,
            ..
        } = &creation.request
        else {
            return;
        };
        if self.host.creation.pending_create.is_some()
            || self.admission.pending.is_some()
            || *checkout != self.host.creation.checkout
        {
            return;
        }
        self.host.creation.pending_create = Some((
            request_id.clone(),
            host::ModelChoice {
                provider: provider.clone(),
                model_id: model_id.clone(),
            },
        ));
        self.host.creation.pending_thinking = *thinking;
        self.set_pending_harness(*harness, mode.clone());
        self.draft.sending = Some(super::new_thread::Sending {
            generation: self.draft.generation,
            request_id: request_id.clone(),
            text: draft.text.clone(),
            images: draft.images.clone(),
            revision: draft.revision,
            approval: creation.approval,
            choice: creation.choice,
        });
        self.admission.pending = Some(creation.request);
    }
    pub(super) fn accept_isolation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.admission.choice {
            return;
        }
        if let Some(Request::CreateAdmittedThread { placement, .. }) = &mut self.admission.pending {
            *placement = Placement::Worktree;
        }
        self.admission.choice = false;
        // The draft stays editable during the choice; send what is on screen now.
        self.retry_create(window, cx);
    }
    pub(super) fn cancel_isolation(&mut self, cx: &mut Context<Self>) {
        if !self.admission.choice {
            return;
        }
        self.admission.choice = false;
        self.admission.pending = None;
        self.admission.source = None;
        self.host.creation.pending_create = None;
        self.draft_rejected("Cancelled. Your draft is kept.".into());
        self.sync_composer(cx);
        cx.notify();
    }
    pub(super) fn isolation_choice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.admission.choice {
            return None;
        }
        Some(column().w_full().max_w(px(CONTENT_WIDTH)).mx_auto().gap_2().p_3()
            .child(div().text_size(px(13.)).child(a11y_text("isolation-choice-heading", "Project busy")))
            .child(div().text_size(px(12.)).text_color(rgb(colors().muted)).child(a11y_text("isolation-choice-detail", "Start from HEAD in a new worktree. Uncommitted files and build output are not copied. The busy thread stays in its checkout. Your message starts the new thread.")))
            .child(row().gap_2()
                .child(button("isolation-worktree", "Own worktree").tooltip(|_,cx| cx.new(|_| super::thread_controls::TextTooltip("Create a separate branch and checkout from HEAD, then send this draft there.".into())).into()).aria_description("Create a separate branch and checkout from HEAD, then send this draft there.").on_activate(cx, |this, window, cx| this.accept_isolation(window,cx)))
                .child(button("isolation-wait", "Wait").disabled(true).tooltip(|_,cx| cx.new(|_| super::thread_controls::TextTooltip("Not yet available. No run is queued.".into())).into()).aria_description("Not yet available. No run is queued."))
                .child(button("isolation-cancel", "Cancel").tooltip(|_,cx| cx.new(|_| super::thread_controls::TextTooltip("Keep this draft without starting a thread.".into())).into()).on_activate(cx, |this,_,cx| this.cancel_isolation(cx))))
            .child(div().text_size(px(11.)).text_color(rgb(colors().muted)).child(a11y_text("isolation-choice-wait-detail", "Wait is not yet available. Nothing is queued.")))
            .into_any_element())
    }
}
fn await_admission(
    client: &Client,
    request: &Request,
) -> Result<admission::Admission, host::Error> {
    let Request::CreateAdmittedThread { request_id, .. } = request else {
        unreachable!()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut receipt = admission::admission(client, request)?;
    loop {
        receipt.verify_request(request)?;
        if receipt.state != Stage::Preparing || Instant::now() >= deadline {
            return Ok(receipt);
        }
        std::thread::sleep(Duration::from_millis(150));
        receipt = admission::admission(
            client,
            &Request::AdmissionStatus {
                request_id: request_id.clone(),
            },
        )?;
    }
}
