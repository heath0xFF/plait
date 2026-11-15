use super::{Plait, theme::*};
use gpui::{prelude::*, *};

impl Plait {
    fn host_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        column().gap_3()
            .child(row().justify_between()
                .child(tag(if self.host.connecting { "Connecting to local host…" } else if self.host.connected { "Local host connected · protocol 1" } else { "Local host disconnected" }, if self.host.connected { MINT } else { AMBER }))
                .child(button("reconnect-host", "Reconnect")
                    .disabled(self.host.connecting)
                    .when(!self.host.connecting, |el| el.on_activate(cx, |this, window, cx| this.reconnect_host(window, cx)))))
            .when_some(self.host.error.clone(), |el, error| el.child(div().text_color(rgb(RED)).child(text!(error))))
            .when(!self.host.connected, |el| el.child(div().text_color(rgb(MUTED)).child("Start the independent host with plait-host start, then Reconnect. Closing Plait does not stop running threads.")))
    }

    fn model_picker(&self, settings: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(status) = &self.host.status else {
            return empty("Model catalog unavailable", "Connect to the local host to load model and authentication status. No credentials are stored in the desktop.").into_any_element();
        };
        if status.models.is_empty() {
            return empty("No models available", "The host returned an empty catalog. Configure Pi authentication on the host, then Reconnect.").into_any_element();
        }
        uniform_list(
            if settings {
                "settings-models"
            } else {
                "new-thread-models"
            },
            status.models.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| this.model_choice(index, settings, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    fn model_choice(&self, index: usize, settings: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(model) = self
            .host
            .status
            .as_ref()
            .and_then(|status| status.models.get(index))
        else {
            return div().into_any_element();
        };
        let choice = model.choice();
        let selected = if settings {
            &self.host.settings_model
        } else {
            &self.host.new_model
        };
        let chosen = selected.as_ref() == Some(&choice);
        let locked = if settings {
            self.host.saving
        } else {
            self.host.pending_create.is_some()
        };
        let enabled = self.host.connected && model.authenticated && !locked;
        button(
            (
                if settings {
                    "default-model"
                } else {
                    "thread-model"
                },
                index,
            ),
            "",
        )
        .h(px(48.))
        .w_full()
        .min_w_0()
        .role(Role::RadioButton)
        .aria_toggled(if chosen {
            Toggled::True
        } else {
            Toggled::False
        })
        .disabled(!enabled)
        .aria_label(format!(
            "{} / {}. {}. {}",
            model.provider,
            model.model_id,
            model.name,
            if model.authenticated {
                "Authenticated"
            } else {
                "Not authenticated"
            }
        ))
        .when(chosen, |el| el.bg(rgb(PANEL)).border_color(rgb(MINT)))
        .child(
            column()
                .min_w_0()
                .flex_1()
                .gap_1()
                .child(div().truncate().text_color(rgb(TEXT)).child(format!(
                    "{}{}",
                    if chosen { "Selected: " } else { "" },
                    model.name
                )))
                .child(
                    div()
                        .truncate()
                        .text_color(rgb(MUTED))
                        .text_size(px(11.))
                        .child(format!("{} / {}", model.provider, model.model_id)),
                ),
        )
        .child(tag(
            if model.authenticated {
                "Authenticated"
            } else {
                "Not authenticated"
            },
            if model.authenticated { MINT } else { AMBER },
        ))
        .when(enabled, |el| {
            el.on_activate(cx, move |this, _, cx| {
                if !this.host.available(Some(&choice))
                    || (settings && this.host.saving)
                    || (!settings && this.host.pending_create.is_some())
                {
                    return;
                }
                if settings {
                    this.host.settings_model = Some(choice.clone());
                    this.host.save_message = Some("Unsaved default model change.".into());
                } else {
                    this.host.new_model = Some(choice.clone());
                }
                cx.notify();
            })
        })
        .into_any_element()
    }

    pub(super) fn settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = self
            .host
            .status
            .as_ref()
            .and_then(|status| status.default_model.as_ref())
            .map_or_else(
                || "No default model saved".into(),
                |model| format!("Saved default: {} / {}", model.provider, model.model_id),
            );
        let can_save = !self.host.saving && self.host.available(self.host.settings_model.as_ref());
        column().flex_1().min_h_0().p_6().gap_4()
            .child(row().justify_between()
                .child(div().text_size(px(18.)).child("Local host settings"))
                .child(button("close-settings", "Done").on_activate(cx, |this, _, cx| { this.settings_open = false; cx.notify(); })))
            .child(self.host_status(cx))
            .child(div().child("Authentication stays on the host. This page shows status only; it never reads or saves credentials."))
            .child(column().gap_1().child(div().font_weight(FontWeight::MEDIUM).child("Default model for new threads"))
                .child(tag(saved, MUTED))
                .child(tag("Existing threads always use their exact saved provider and model.", MUTED)))
            .child(self.model_picker(true, cx))
            .child(row().flex_shrink_0().gap_3()
                .child(button("save-default-model", if self.host.saving { "Saving…" } else { "Save default model" })
                    .bg(rgb(PANEL)).disabled(!can_save)
                    .when(can_save, |el| el.on_activate(cx, |this, window, cx| this.save_default_model(window, cx))))
                .when_some(self.host.save_message.clone(), |el, message| el.child(div().min_w_0().child(text!(message)))))
    }

    pub(super) fn new_thread(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_create = self.host.connected
            && !self.host.creating
            && (self.host.pending_create.is_some()
                || self.host.available(self.host.new_model.as_ref()));
        column()
            .flex_1()
            .min_h_0()
            .p_6()
            .gap_4()
            .child(div().text_size(px(18.)).child("Choose a model"))
            .child(div().child(
                "A new thread belongs to this local checkout. Creating it does not run the model.",
            ))
            .child(tag(self.host.checkout.display().to_string(), MUTED))
            .child(self.host_status(cx))
            .child(self.model_picker(false, cx))
            .child(
                row()
                    .flex_shrink_0()
                    .child(
                        button(
                            "create-thread",
                            if self.host.creating {
                                "Creating…"
                            } else if self.host.pending_create.is_some() {
                                "Retry thread creation"
                            } else {
                                "Create local thread"
                            },
                        )
                        .bg(rgb(PANEL))
                        .text_color(rgb(MINT))
                        .disabled(!can_create)
                        .when(can_create, |el| {
                            el.on_activate(cx, |this, window, cx| {
                                this.create_host_thread(window, cx)
                            })
                        }),
                    )
                    .child(
                        button("cancel-new-thread", "Cancel").on_activate(cx, |this, _, cx| {
                            this.new_thread_open = false;
                            cx.notify();
                        }),
                    ),
            )
    }

    pub(super) fn open_new_thread(&mut self, cx: &mut Context<Self>) {
        if self.host.pending_create.is_none() {
            self.host.new_model = self
                .host
                .status
                .as_ref()
                .and_then(|status| status.default_model.clone())
                .filter(|choice| self.host.available(Some(choice)));
        }
        self.host.select(None);
        self.state.select(plait::model::Selection {
            project: 0,
            machine: 0,
            thread: 0,
        });
        self.settings_open = false;
        self.new_thread_open = true;
        self.show_changes = false;
        cx.notify();
    }
}
