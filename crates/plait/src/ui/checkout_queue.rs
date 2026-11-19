//! Checkout queue controls use the shared catalog; they own no watch or timer.
use super::{Plait, theme::*, thread_controls::TextTooltip};
use gpui::*;
use plait_core::host::{Request, checkout_queue};
impl Plait {
    pub(super) fn wait_in_checkout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.admission.queue || !self.can_submit() {
            return;
        }
        let Some(id) = self.host.thread.selected.clone() else {
            return;
        };
        let Some(composer) = self.workspace.composers.get(&id) else {
            return;
        };
        let composer = composer.read(cx);
        let (text, revision, _) = composer.saved_draft();
        let images = composer.saved_images().to_vec();
        match self.host.submissions.begin_images(
            &id,
            text.into(),
            self.next_approval_mode(),
            None,
            Some(revision),
            images,
        ) {
            Ok(pending) => {
                self.host
                    .submissions
                    .wait_for_checkout(&id, &pending.request_id);
                self.retry_submission_id(&id, &pending.request_id, window, cx);
            }
            Err(error) => self.set_thread_notice(id, error, true),
        }
    }
    pub(super) fn checkout_wait_control(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.host.thread.selected.as_ref()?;
        let thread = self
            .host
            .connection
            .threads
            .iter()
            .find(|thread| &thread.id == id)?;
        let queue = thread.checkout_queue.as_ref()?;
        let cancel = Request::CancelCheckoutWait {
            thread_id: id.clone(),
            request_id: queue.request_id.clone(),
        };
        let resume = Request::ResumeCheckoutQueue {
            thread_id: id.clone(),
        };
        let detail = queue.detail().to_owned();
        let enabled = self.admission.queue && self.host.connection.connected;
        let mut controls = row()
            .w_full()
            .max_w(px(CONTENT_WIDTH))
            .mx_auto()
            .gap_2()
            .child(
                button("checkout-wait-state", queue.label())
                    .text_color(rgb(if queue.state == checkout_queue::State::Held {
                        colors().amber
                    } else {
                        colors().blue
                    }))
                    .tooltip(move |_, cx| cx.new(|_| TextTooltip(detail.clone())).into()),
            );
        if queue.state == checkout_queue::State::Held {
            controls = controls.child(
                button("checkout-wait-resume", "Resume")
                    .disabled(!enabled)
                    .aria_description(
                        "Resume all held messages for this checkout in their original order.",
                    )
                    .on_activate(cx, move |this, window, cx| {
                        this.change_checkout_wait(resume.clone(), window, cx)
                    }),
            );
        }
        Some(
            controls
                .child(
                    button("checkout-wait-cancel", "Cancel")
                        .disabled(!enabled)
                        .aria_description(
                            "Cancel this waiting message. If it just started, stop its run.",
                        )
                        .on_activate(cx, move |this, window, cx| {
                            this.change_checkout_wait(cancel.clone(), window, cx)
                        }),
                )
                .into_any_element(),
        )
    }
    fn change_checkout_wait(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (id, cancelled) = match &request {
            Request::CancelCheckoutWait {
                thread_id,
                request_id,
            } => (thread_id.clone(), Some(request_id.clone())),
            Request::ResumeCheckoutQueue { thread_id } => (thread_id.clone(), None),
            _ => return,
        };
        let Some(checkout) = self
            .host
            .connection
            .threads
            .iter()
            .find(|thread| thread.id == id)
            .map(|thread| thread.checkout.clone())
        else {
            return;
        };
        self.host_request::<checkout_queue::Reply>(
            request,
            window,
            cx,
            move |this, result, _, cx| {
                match result {
                    Ok(reply) if reply.verified(&id, &checkout) => {
                        if let Some(request_id) = cancelled {
                            let Some(waiter) = reply.waiters.iter().find(|waiter| waiter.request_id == request_id) else {
                                this.set_thread_notice(id, "Queue change was not confirmed. Reconnect to inspect it.".into(), true);
                                cx.notify();
                                return;
                            };
                            let message = match waiter.state {
                                checkout_queue::State::Started => "Started. If this message was still running, it was asked to stop.".into(),
                                checkout_queue::State::Cancelled => "Cancelled. Your waiting message will not run.".into(),
                                checkout_queue::State::Failed => format!("Not run. {}", waiter.error.as_ref().unwrap().message),
                                _ => {
                                    this.set_thread_notice(id, "Queue change was not confirmed. Reconnect to inspect it.".into(), true);
                                    cx.notify();
                                    return;
                                }
                            };
                            this.host.submissions.forget_accepted(&id, &request_id);
                            this.set_thread_notice(id, message, false);
                            this.capture_session(cx);
                        }
                    }
                    Ok(_) => this.set_thread_notice(
                        id,
                        "Queue change was not confirmed. Reconnect to inspect it.".into(),
                        true,
                    ),
                    Err(error) => this.set_thread_notice(id, error.to_string(), true),
                }
                cx.notify();
            },
        );
    }
}
