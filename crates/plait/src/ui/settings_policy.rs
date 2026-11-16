//! The host owns command policy; an unread policy is never called sandboxed.
use super::{Plait, settings, theme::*};
use gpui::{prelude::*, *};
use plait_core::host::{
    policy::{self, CommandPolicy, PolicyReply, Request},
    providers,
};
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct PolicyState {
    pub key: Option<(u64, PathBuf)>,
    pub value: Option<CommandPolicy>,
    pub busy: bool,
    pub confirm: bool,
    pub note: Option<String>,
}

impl Plait {
    pub(super) fn command_policy(&self) -> Option<CommandPolicy> {
        (self.host.connected
            && self.policy.key.as_ref() == Some(&(self.host.epoch, self.host.checkout.clone())))
        .then_some(self.policy.value)
        .flatten()
    }

    pub(super) fn sync_policy(&mut self, cx: &mut Context<Self>) {
        if !self.host.connected || self.state.repository.snapshot.ready().is_none() || cfg!(test) {
            return;
        }
        let key = (self.host.epoch, self.host.checkout.clone());
        if self.policy.key.as_ref() == Some(&key) {
            return;
        }
        self.policy = PolicyState {
            key: Some(key),
            ..Default::default()
        };
        self.request_policy(None, cx);
    }

    fn request_policy(&mut self, value: Option<CommandPolicy>, cx: &mut Context<Self>) {
        if self.policy.busy || !self.host.connected {
            return;
        }
        self.policy.busy = true;
        self.policy.confirm = false;
        self.policy.note = None;
        let key = (self.host.epoch, self.host.checkout.clone());
        let checkout = self
            .state
            .repository
            .snapshot
            .ready()
            .map_or_else(|| key.1.clone(), |snapshot| snapshot.root.clone());
        let task = cx.background_executor().spawn(async move {
            let canonical = std::fs::canonicalize(&checkout)?;
            let request = match value {
                Some(policy) => Request::SetCommandPolicy {
                    checkout: canonical.clone(),
                    policy,
                },
                None => Request::CommandPolicy {
                    checkout: canonical.clone(),
                },
            };
            let reply: PolicyReply =
                plait_core::host::Client::from_environment()?.send(&request)?;
            if reply.checkout != canonical {
                return Err(plait_core::host::Error::Protocol(
                    "Host returned another checkout's policy.".into(),
                ));
            }
            Ok(reply)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.policy.key.as_ref() != Some(&key)
                    || this.host.epoch != key.0
                    || this.host.checkout != key.1
                {
                    return;
                }
                this.policy.busy = false;
                match result {
                    Ok(reply) => {
                        this.policy.value = Some(reply.policy);
                        this.policy.note =
                            value.map(|_| "Saved. Active runs keep their original policy.".into());
                    }
                    Err(error) => {
                        this.policy.note = Some(if providers::needs_host_update(&error) {
                            "Update the host to manage command policy.".into()
                        } else {
                            providers::refusal(&error)
                        })
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn policy_settings(&self, cx: &mut Context<Self>) -> Div {
        let current = self.command_policy();
        let enabled = self.host.connected && !self.policy.busy;
        let mut choices = segmented();
        for (index, (label, policy)) in [
            ("Sandboxed", CommandPolicy::Sandboxed),
            ("Trusted", CommandPolicy::Trusted),
        ]
        .into_iter()
        .enumerate()
        {
            choices = choices.child(
                segment(("command-policy", index), label, current == Some(policy))
                    .disabled(!enabled)
                    .when(enabled, |el| {
                        el.on_activate(cx, move |this, _, cx| {
                            if policy == CommandPolicy::Trusted
                                && this.command_policy() != Some(policy)
                            {
                                this.policy.confirm = true;
                                cx.notify();
                            } else {
                                this.request_policy(Some(policy), cx);
                            }
                        })
                    }),
            );
        }
        settings::section("Command policy")
            .child(settings::setting(
                "This checkout",
                current.map_or(
                    "Policy has not loaded from the host.",
                    CommandPolicy::meaning,
                ),
                choices,
            ))
            .child(tag(
                self.host.checkout.to_string_lossy().into_owned(),
                SUBTLE,
            ))
            .when(self.policy.busy, |el| {
                el.child(tag("Loading command policy…", MUTED))
            })
            .when_some(self.policy.note.clone(), |el, note| {
                el.child(tag(note, AMBER))
            })
            .when(current.is_none() && enabled, |el| {
                el.child(
                    button("retry-policy", "Retry")
                        .on_activate(cx, |this, _, cx| this.request_policy(None, cx)),
                )
            })
            .when(self.policy.confirm, |el| {
                el.child(
                    column()
                        .gap_2()
                        .p_3()
                        .border_1()
                        .border_color(rgb(AMBER))
                        .rounded_md()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Trust commands in this checkout?"),
                        )
                        .children(
                            policy::TRUSTED_CONSEQUENCES
                                .iter()
                                .map(|line| tag(*line, MUTED).line_height(px(18.))),
                        )
                        .child(tag(policy::TRUSTED_SCOPE, SUBTLE))
                        .child(
                            row()
                                .gap_2()
                                .child(button("cancel-trusted", "Keep current policy").on_activate(
                                    cx,
                                    |this, _, cx| {
                                        this.policy.confirm = false;
                                        cx.notify();
                                    },
                                ))
                                .child(
                                    toned_button(
                                        "confirm-trusted",
                                        "Trust this checkout",
                                        Tone::Filled,
                                    )
                                    .on_activate(
                                        cx,
                                        |this, _, cx| {
                                            this.request_policy(Some(CommandPolicy::Trusted), cx)
                                        },
                                    ),
                                ),
                        ),
                )
            })
    }
}
