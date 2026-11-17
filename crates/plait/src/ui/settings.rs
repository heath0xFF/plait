//! The Settings page: host status first, then Models, Providers, Approvals, Agent,
//! Keyboard, View, and About and updates.

use super::{
    Plait,
    commands::Command,
    jump::PreferenceGroup,
    model_picker::{ModelPurpose, model_label},
    palette::{Item, Options, Palette},
    theme::*,
};
use gpui::{prelude::*, *};
use std::collections::HashSet;

/// Centered reading column for settings-style pages.
fn page(id: &'static str) -> Stateful<Div> {
    column()
        .id(id)
        .overflow_y_scroll()
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
        .text_color(rgb(colors().muted))
        .child(text.into())
}

/// One titled group of settings, ruled off from the group above it.
pub(super) fn section(title: &'static str) -> Div {
    column()
        .gap_3()
        .flex_shrink_0()
        .pt_4()
        .border_t_1()
        .border_color(rgb(colors().border))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                // Several section headings can appear in one accessibility tree.
                .child(text!(id = title, title)),
        )
}

/// A setting: what it is and what it means on the left, its control on the right.
pub(super) fn setting(
    label: &'static str,
    detail: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    row()
        .items_start()
        .justify_between()
        .gap_4()
        .child(
            column()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(colors().muted))
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(17.))
                        .text_color(rgb(colors().subtle))
                        .child(detail.into()),
                ),
        )
        .child(control)
}

/// A state in words beside its dot, so the color is never the only signal.
fn state_line(text: impl Into<SharedString>, tint: u32) -> Div {
    row()
        .items_start()
        .child(dot(tint).mt(px(5.)))
        // Long host errors wrap; they are never cut off.
        .child(tag(text, tint).flex_1().min_w_0().line_height(px(17.)))
}

impl Plait {
    fn host_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, tint) = if self.host.connecting {
            ("Connecting to local host…".to_owned(), colors().muted)
        } else if self.host.connected {
            (
                format!("Local host connected · protocol {}", self.host.protocol()),
                colors().mint,
            )
        } else {
            ("Local host disconnected".to_owned(), colors().amber)
        };
        column()
            .gap_2()
            .flex_shrink_0()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(rgb(colors().border))
            .bg(rgb(colors().rail))
            .child(
                row()
                    .justify_between()
                    .child(row().child(dot(tint)).child(tag(label, tint)))
                    .child(
                        row()
                            .gap_1()
                            .child(
                                button("open-setup-guide", "Setup and recovery guide").on_activate(
                                    cx,
                                    |_, _, cx| {
                                        cx.open_url(
                                            "https://github.com/heath0xFF/plait/blob/main/docs/MACOS.md",
                                        );
                                    },
                                ),
                            )
                            .child(
                                toned_button("reconnect-host", "Reconnect", Tone::Filled)
                                    .disabled(self.host.connecting)
                                    .when(!self.host.connecting, |el| {
                                        el.on_activate(cx, |this, window, cx| {
                                            this.reconnect_host(window, cx)
                                        })
                                    }),
                            ),
                    ),
            )
            .when_some(self.host.error.clone(), |el, error| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(colors().red))
                        .child(text!(error)),
                )
            })
            .when(!self.host.connected, |el| {
                el.child(note(
                    "The installed host normally starts at login. Reconnect does not restart it. Closing Plait does not stop accepted work.",
                ))
            })
    }

    /// How much of the host's catalog can be used, counted from its reply.
    fn catalog_summary(&self) -> Option<String> {
        let models = &self.host.status.as_ref()?.models;
        if models.is_empty() {
            return Some(
                "The host returned no models. The setup guide explains how to configure it.".into(),
            );
        }
        let signed_in = models.iter().filter(|model| model.authenticated);
        let usable = signed_in.clone().count();
        if usable == 0 {
            return Some("No model is signed in. Open Providers to sign in; its models become available as soon as the host confirms.".into());
        }
        let providers = |models: &mut dyn Iterator<Item = &plait_core::host::Model>| {
            models
                .map(|model| model.provider.as_str())
                .collect::<HashSet<_>>()
                .len()
        };
        Some(format!(
            "{usable} of {} models are signed in, from {} of {} providers.",
            models.len(),
            providers(&mut signed_in.clone()),
            providers(&mut models.iter()),
        ))
    }

    /// The default model at the top of its section. Choosing in the picker
    /// saves at once; the line beneath reports what the host answered.
    fn model_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = self
            .host
            .status
            .as_ref()
            .and_then(|status| status.default_model.as_ref());
        let name = saved.map_or_else(
            || "No default model".to_owned(),
            |choice| self.model_name(choice),
        );
        let usable = saved.is_none_or(|choice| self.host.available(Some(choice)));
        let can_choose = self.host.connected && !self.host.saving;
        let control = toned_button("choose-default-model", "", Tone::Filled)
            .aria_label(format!("Default model: {name}. Choose another…"))
            .gap_2()
            .max_w(px(320.))
            .child(div().min_w_0().truncate().child(name))
            .child(icon("down").text_color(rgb(colors().muted)))
            .disabled(!can_choose)
            .when(can_choose, |el| {
                el.on_activate(cx, |this, window, cx| {
                    this.open_model_picker(ModelPurpose::DefaultModel, window, cx)
                })
            });
        section("Models")
            .child(setting(
                "Default model for new threads",
                "New threads start with this model. Choosing another saves it on the host at once. Existing threads are unchanged; their own model control changes them between runs.",
                control,
            ))
            .when_some(saved, |el, choice| {
                el.child(tag(model_label(choice), colors().muted))
            })
            .when(!usable && self.host.connected, |el| {
                el.child(state_line(
                    "The saved default is not signed in on this host. New threads ask for a model until it is.",
                    colors().amber,
                ))
            })
            .when_some(self.default_model_state(), |el, (state, tint)| {
                el.child(state_line(state, tint))
            })
            .child(self.default_thinking_setting(cx))
            .when_some(self.catalog_summary(), |el, summary| {
                el.child(tag(summary, colors().subtle))
            })
            .child(tag(
                "Authentication stays on the host. Keys and sign-in answers entered here are sent once and never saved in desktop preferences.",
                colors().subtle,
            ))
            .when_some(
                self.preference_note(Some(PreferenceGroup::Models)),
                |el, note| el.child(tag(format!("Favorites and recent models: {note}"), colors().subtle)),
            )
    }

    /// Saved on this desktop and sent with each new thread. The host picks the
    /// nearest level the thread's model offers, so one setting fits all models.
    fn default_thinking_setting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let level = self.preferences.value.default_thinking();
        let detail = if self.host.connected && self.host.protocol() < 3 {
            "This host starts every thread with thinking off. Update the host to use this setting."
        } else {
            "New threads start at this level, or the closest one their model offers. A thread's own level is under its message box."
        };
        let control = toned_button("choose-default-thinking", "", Tone::Filled)
            .aria_label(format!(
                "Default thinking: {}. Choose another…",
                level.label()
            ))
            .gap_2()
            .child(level.label())
            .child(icon("down").text_color(rgb(colors().muted)))
            .on_activate(cx, |this, window, cx| {
                this.open_default_thinking(window, cx)
            });
        setting("Default thinking for new threads", detail, control)
    }

    fn open_default_thinking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.preferences.value.default_thinking();
        let items = plait_core::host::ThinkingLevel::ALL
            .iter()
            .map(|level| {
                Item::new(
                    format!("default-thinking-{level:?}"),
                    level.label(),
                    Command::DefaultThinking(*level),
                )
            })
            .collect();
        let palette = cx.new(|cx| {
            Palette::new(
                Options {
                    name: "Default thinking level".into(),
                    placeholder: "Choose the level new threads start at…".into(),
                    empty: "No levels match.".into(),
                    // Start on the saved level, so Return keeps it.
                    initial: Some(format!("default-thinking-{current:?}").into()),
                    mark: None,
                    aside: None,
                    note: Some("Existing threads keep their own level.".into()),
                },
                items,
                window,
                cx,
            )
        });
        self.present(palette, None, window, cx);
    }

    pub(super) fn settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_1().min_h_0().px_6().py_5().child(
            column()
                .flex_1()
                .min_h_0()
                .w_full()
                .max_w(px(CONTENT_WIDTH))
                .mx_auto()
                .gap_3()
                .child(
                    row()
                        .flex_shrink_0()
                        .justify_between()
                        .child(heading("Settings"))
                        .child(
                            toned_button("close-settings", "Done", Tone::Filled)
                                .on_activate(cx, |this, window, cx| {
                                    this.show_settings(false, window, cx)
                                }),
                        ),
                )
                .when_some(self.preference_note(None), |el, note| {
                    el.child(state_line(
                        format!("Desktop preferences: {note}"),
                        colors().amber,
                    ))
                })
                .child(self.host_status(cx))
                .child({
                    let mut tabs = row().gap_1().flex_wrap();
                    for (index, label) in [
                        "Models",
                        "Providers",
                        "Approvals",
                        "Agent",
                        "Keyboard",
                        "View",
                        "About",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        tabs = tabs.child(
                            toned_button(
                                ("settings-section", index),
                                label,
                                if self.settings_section == index {
                                    Tone::Selected
                                } else {
                                    Tone::Ghost
                                },
                            )
                            .role(Role::Tab)
                            .aria_selected(self.settings_section == index)
                            .on_activate(cx, move |this, _, cx| {
                                this.cancel_recording(cx);
                                if index != 1 {
                                    this.close_provider_form(cx);
                                }
                                this.settings_section = index;
                                if index == 3 {
                                    this.refresh_agent(cx);
                                }
                                this.settings_scroll.set_offset(point(px(0.), px(0.)));
                                cx.notify();
                            }),
                        );
                    }
                    tabs
                })
                .child(
                    page("settings-page")
                        .track_scroll(&self.settings_scroll)
                        .child(match self.settings_section {
                            1 => self.provider_settings(cx).into_any_element(),
                            2 => column()
                                .gap_4()
                                .when(!self.policy.confirm, |el| {
                                    el.child(self.approval_settings(cx))
                                })
                                .child(self.policy_settings(cx))
                                .into_any_element(),
                            3 => self.agent_settings(cx).into_any_element(),
                            4 => self.keyboard_settings(cx).into_any_element(),
                            5 => self.view_settings(cx).into_any_element(),
                            6 => self.build_details(cx).into_any_element(),
                            _ => self.model_settings(cx).into_any_element(),
                        }),
                ),
        )
    }

    /// Identifies the running build, so an installed update can be confirmed.
    fn build_details(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let revision = option_env!("PLAIT_BUILD_REVISION").unwrap_or("source build");
        let built = option_env!("PLAIT_BUILD_TIME")
            .map(|time| format!(" · built {time}"))
            .unwrap_or_default();
        let host = if self.host.connected {
            format!("Host protocol {}", self.host.protocol())
        } else {
            "Host not connected".into()
        };
        section("About and updates")
            .child(
                column()
                    .gap_1()
                    .child(tag(
                        format!("Desktop {} · {revision}{built}", env!("CARGO_PKG_VERSION")),
                        colors().muted,
                    ))
                    .child(tag(host, colors().subtle)),
            )
            .child(self.update_controls(cx))
    }

    fn view_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hidden = self.preferences.value.hide_changed_files;
        let mut choices = segmented();
        for (index, (label, value)) in [("Show", false), ("Hide", true)].into_iter().enumerate() {
            choices = choices.child(
                segment(("changed-files", index), label, value == hidden)
                    .role(Role::RadioButton)
                    .aria_toggled(if value == hidden {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .aria_label(format!("Changed files above the message box: {label}"))
                    .on_activate(cx, move |this, _, cx| {
                        this.set_changed_files_hidden(value, cx)
                    }),
            );
        }
        section("View")
            .child(self.theme_settings(cx))
            .child(setting(
                "Changed files above the message box",
                "Files the thread wrote that are not committed yet, with lines added and removed. Click one to see it in Changes.",
                choices,
            ))
            .when_some(
                self.preference_note(Some(PreferenceGroup::View)),
                |el, note| el.child(tag(note, colors().subtle)),
            )
    }

    pub(super) fn set_changed_files_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        let mut value = self.preferences.value.clone();
        value.hide_changed_files = hidden;
        self.set_preferences(value, PreferenceGroup::View, cx);
        // Lists were not fetched while hidden.
        if !hidden {
            self.thread_files.reset();
            self.thread_files_snapshot(cx);
        }
    }

    fn approval_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ask = self.preferences.value.ask_before_actions;
        let mut choices = segmented();
        for (index, (label, value)) in [("Automatic", false), ("Ask each time", true)]
            .into_iter()
            .enumerate()
        {
            choices = choices.child(
                segment(("approval-mode", index), label, value == ask)
                    .role(Role::RadioButton)
                    .aria_toggled(if value == ask {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .aria_label(format!("Edits and commands: {label}"))
                    .on_activate(cx, move |this, _, cx| {
                        let mut preferences = this.preferences.value.clone();
                        preferences.ask_before_actions = value;
                        this.set_preferences(preferences, PreferenceGroup::Approvals, cx);
                    }),
            );
        }
        let detail = if self.host.connected && self.host.protocol() < 2 {
            "This host asks before every edit and command. Update the host to use Automatic."
        } else {
            plait_core::host::policy::approval_detail(ask, self.command_policy())
        };
        section("Approvals")
            .child(setting("Default for edits and commands", detail, choices))
            .child(tag(
                "A thread can choose its own from the run settings under its message box.",
                colors().subtle,
            ))
            .when_some(
                self.preference_note(Some(PreferenceGroup::Approvals)),
                |el, note| el.child(tag(note, colors().subtle)),
            )
    }
}
