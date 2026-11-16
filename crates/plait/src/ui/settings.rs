use super::{Plait, theme::*};
use gpui::{prelude::*, *};

const MODEL_ROW_HEIGHT: f32 = 36.;

/// Centered reading column for settings-style pages.
fn page() -> Div {
    column()
        .flex_1()
        .min_h_0()
        .w_full()
        .max_w(px(CONTENT_WIDTH))
        .mx_auto()
        .gap_4()
}

fn heading(text: &'static str) -> Div {
    div()
        .text_size(px(15.))
        .font_weight(FontWeight::MEDIUM)
        .child(text)
}

fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .line_height(px(19.))
        .text_color(rgb(MUTED))
        .child(text.into())
}

/// Ring for an unchosen option, filled for the chosen one: shape, not just color.
fn radio(chosen: bool) -> Div {
    div()
        .size(px(12.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(rgb(if chosen { MINT } else { SUBTLE }))
        .when(chosen, |el| {
            el.child(div().size(px(6.)).rounded_full().bg(rgb(MINT)))
        })
}

impl Plait {
    fn host_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, tint) = if self.host.connecting {
            ("Connecting to local host…", MUTED)
        } else if self.host.connected {
            ("Local host connected · protocol 1", MINT)
        } else {
            ("Local host disconnected", AMBER)
        };
        column()
            .gap_2()
            .flex_shrink_0()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(RAIL))
            .child(
                row()
                    .justify_between()
                    .child(row().child(dot(tint)).child(tag(label, tint)))
                    .child(
                        button("reconnect-host", "Reconnect")
                            .disabled(self.host.connecting)
                            .when(!self.host.connecting, |el| {
                                el.on_activate(cx, |this, window, cx| {
                                    this.reconnect_host(window, cx)
                                })
                            }),
                    ),
            )
            .when_some(self.host.error.clone(), |el, error| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(RED))
                        .child(text!(error)),
                )
            })
            .when(!self.host.connected, |el| {
                el.child(note(
                    "Start the independent host with plait-host start, then Reconnect. Closing Plait does not stop running threads.",
                ))
            })
    }

    fn model_picker(&self, settings: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let list = column()
            .flex_1()
            .min_h_0()
            .rounded_lg()
            .border_1()
            .border_color(rgb(BORDER))
            .overflow_hidden();
        let Some(status) = &self.host.status else {
            return list.child(empty(
                "Model catalog unavailable",
                "Connect to the local host to load model and authentication status. No credentials are stored in the desktop.",
            ));
        };
        if status.models.is_empty() {
            return list.child(empty(
                "No models available",
                "The host returned an empty catalog. Configure Pi authentication on the host, then Reconnect.",
            ));
        }
        list.child(
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
            .min_h_0(),
        )
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
        nav_row(
            (
                if settings {
                    "default-model"
                } else {
                    "thread-model"
                },
                index,
            ),
            format!(
                "{} / {}. {}. {}",
                model.provider,
                model.model_id,
                model.name,
                if model.authenticated {
                    "Authenticated"
                } else {
                    "Not authenticated"
                }
            ),
            chosen,
        )
        .h(px(MODEL_ROW_HEIGHT))
        .rounded_none()
        .px_3()
        .gap_3()
        .role(Role::RadioButton)
        .aria_toggled(if chosen {
            Toggled::True
        } else {
            Toggled::False
        })
        .disabled(!enabled)
        .child(radio(chosen))
        .child(
            div()
                .flex_shrink_0()
                .text_color(rgb(TEXT))
                .child(model.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(rgb(SUBTLE))
                .child(format!("{} / {}", model.provider, model.model_id)),
        )
        .child(tag(
            if model.authenticated {
                "Authenticated"
            } else {
                "Not authenticated"
            },
            if model.authenticated { SUBTLE } else { AMBER },
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
        div().flex().flex_1().min_h_0().px_6().py_5().child(
            page()
                .child(
                    row()
                        .justify_between()
                        .child(heading("Local host settings"))
                        .child(toned_button("close-settings", "Done", Tone::Filled).on_activate(
                            cx,
                            |this, window, cx| this.show_settings(false, window, cx),
                        )),
                )
                .child(self.host_status(cx))
                .child(note(
                    "Authentication stays on the host. This page shows status only; it never reads or saves credentials.",
                ))
                .child(
                    column()
                        .gap_1()
                        .child(section_label("Default model for new threads"))
                        .child(tag(saved, MUTED))
                        .child(tag(
                            "Existing threads always use their exact saved provider and model.",
                            SUBTLE,
                        )),
                )
                .child(self.model_picker(true, cx))
                .child(
                    row()
                        .flex_shrink_0()
                        .gap_3()
                        .child(
                            toned_button(
                                "save-default-model",
                                if self.host.saving {
                                    "Saving…"
                                } else {
                                    "Save default model"
                                },
Tone::Primary,
)
                            .disabled(!can_save)
                            .when(can_save, |el| {
                                el.on_activate(cx, |this, window, cx| {
                                    this.save_default_model(window, cx)
                                })
                            }),
                        )
                        .when_some(self.host.save_message.clone(), |el, message| {
                            el.child(
                                div()
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .text_color(rgb(MUTED))
                                    .child(text!(message)),
                            )
                        }),
                ),
        )
    }

    pub(super) fn new_thread(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_create = self.host.connected
            && !self.host.creating
            && (self.host.pending_create.is_some()
                || self.host.available(self.host.new_model.as_ref()));
        div().flex().flex_1().min_h_0().px_6().py_5().child(
            page()
                .child(
                    column()
                        .gap_1()
                        .child(heading("Choose a model"))
                        .child(note(
                            "A new thread belongs to this local checkout. Creating it does not run the model.",
                        ))
                        .child(tag(self.host.checkout.display().to_string(), SUBTLE)),
                )
                .child(self.host_status(cx))
                .child(self.model_picker(false, cx))
                .child(
                    row()
                        .flex_shrink_0()
                        .child(
                            toned_button(
                                "create-thread",
                                if self.host.creating {
                                    "Creating…"
                                } else if self.host.pending_create.is_some() {
                                    "Retry thread creation"
                                } else {
                                    "Create local thread"
                                },
Tone::Primary,
)
                            .disabled(!can_create)
                            .when(can_create, |el| {
                                el.on_activate(cx, |this, window, cx| {
                                    this.create_host_thread(window, cx)
                                })
                            }),
                        )
                        .child(button("cancel-new-thread", "Cancel").on_activate(
                            cx,
                            |this, _, cx| {
                                this.new_thread_open = false;
                                cx.notify();
                            },
                        )),
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
        self.host_watch = None;
        self.host.select(None);
        // Cancel returns to this machine's first example thread.
        self.show_local_machine();
        self.settings_open = false;
        self.new_thread_open = true;
        self.show_changes = false;
        cx.notify();
    }
}
