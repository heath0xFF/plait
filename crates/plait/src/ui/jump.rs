//! Cmd-1 through Cmd-9, and the desktop preferences file that backs them and
//! the other choices that belong to this desktop.

use super::{Plait, settings, theme::*};
use gpui::{prelude::*, *};
use plait_core::preferences::{self, JumpTarget, Preferences};

// Bound in the action table (actions.rs), like every application shortcut.
actions!(
    plait,
    [
        Jump1, Jump2, Jump3, Jump4, Jump5, Jump6, Jump7, Jump8, Jump9
    ]
);

/// Which part of the app a preference belongs to, so its saved state is shown
/// beside the control that changed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreferenceGroup {
    Models,
    Approvals,
    Keyboard,
    /// Layout choices made outside Settings, such as hiding the Git inspector.
    View,
}

/// Saved state of the preference file, shown beside the setting.
#[derive(Default)]
pub(super) struct PreferenceState {
    pub value: Preferences,
    /// `None` when the platform gives no home directory; changes then last one session.
    path: Option<std::path::PathBuf>,
    /// Only the newest save may report its result.
    revision: u64,
    pub note: Option<String>,
    /// The group of the newest change. `None` while `note` is about loading.
    group: Option<PreferenceGroup>,
    /// `note` reports a problem, not progress.
    failed: bool,
}

impl Plait {
    pub(super) fn on_jump_actions(
        &self,
        root: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        macro_rules! jumps {
            ($root:expr, $($action:ident => $number:literal),*) => {
                $root$(.on_action(cx.listener(|this, _: &$action, window, cx| {
                    this.jump_to($number, window, cx)
                })))*
            };
        }
        jumps!(root, Jump1 => 1, Jump2 => 2, Jump3 => 3, Jump4 => 4, Jump5 => 5,
            Jump6 => 6, Jump7 => 7, Jump8 => 8, Jump9 => 9)
    }

    /// Select the Nth thread, machine, or project in sidebar order.
    pub(super) fn jump_to(&mut self, number: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(jump) = preferences::jump(
            &self.state.projects,
            self.host.threads.len(),
            self.preferences.value.jump_target,
            number,
        ) else {
            return;
        };
        self.state.collapsed_projects.remove(&jump.project);
        self.state
            .collapsed_machines
            .remove(&(jump.project, jump.machine));
        let id = self.host.threads[jump.thread].id.clone();
        self.select_host_thread(id, window, cx);
    }

    /// Read the file in the background. Until it loads, the default applies.
    pub(super) fn load_preferences(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async move {
            let path = preferences::default_path();
            let loaded = path.as_deref().map(preferences::load);
            (path, loaded)
        });
        cx.spawn(async move |this, cx| {
            let (path, loaded) = task.await;
            let _ = this.update(cx, |this, cx| {
                let state = &mut this.preferences;
                state.path = path;
                // A choice made before loading finished is newer than the file.
                if state.revision == 0 {
                    state.failed = !matches!(loaded, Some(Ok(_)));
                    match loaded {
                        Some(Ok(value)) => state.value = value,
                        Some(Err(error)) => state.note = Some(error),
                        None => {
                            state.note = Some(
                                "No home directory; preferences last until Plait quits.".into(),
                            )
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn set_jump_target(&mut self, target: JumpTarget, cx: &mut Context<Self>) {
        let mut value = self.preferences.value.clone();
        value.jump_target = target;
        self.set_preferences(value, PreferenceGroup::Keyboard, cx);
    }

    /// Apply now; report "Saved" only once the file write succeeds.
    pub(super) fn set_preferences(
        &mut self,
        value: Preferences,
        group: PreferenceGroup,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.preferences;
        if state.value == value && state.revision > 0 {
            return;
        }
        state.value = value.clone();
        state.revision += 1;
        state.group = Some(group);
        state.failed = state.path.is_none();
        let revision = state.revision;
        let Some(path) = state.path.clone() else {
            state.note =
                Some("Not saved: no preferences location. Applies until Plait quits.".into());
            cx.notify();
            return;
        };
        state.note = Some("Saving…".into());
        let task = cx
            .background_executor()
            .spawn(async move { preferences::save(&path, &value) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.preferences.revision != revision {
                    return;
                }
                this.preferences.failed = result.is_err();
                this.preferences.note = Some(match result {
                    Ok(()) => "Saved.".into(),
                    Err(error) => {
                        let note = format!("{error} The choice applies until Plait quits.");
                        // A picker is where favorites change; report it there too.
                        if group == PreferenceGroup::Models {
                            this.overlay_note(Some(note.clone().into()), cx);
                        }
                        note
                    }
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The preference file's state, for the section whose control changed it.
    /// Loading problems and changes made outside Settings belong to no section.
    pub(super) fn preference_note(&self, group: Option<PreferenceGroup>) -> Option<String> {
        let state = &self.preferences;
        let shown = match state.group {
            // Outside a section only a problem is worth a line, never "Saved."
            None | Some(PreferenceGroup::View) => state.failed.then_some(None),
            changed => Some(changed),
        };
        state.note.clone().filter(|_| shown == Some(group))
    }

    pub(super) fn keyboard_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.preferences.value.jump_target;
        let mut choices = segmented();
        for (index, target) in JumpTarget::ALL.into_iter().enumerate() {
            choices = choices.child(
                segment(("jump-target", index), target.label(), target == current)
                    .role(Role::RadioButton)
                    .aria_toggled(if target == current {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .aria_label(format!("Command 1 to 9 jump to {}", target.label()))
                    .on_activate(cx, move |this, _, cx| this.set_jump_target(target, cx)),
            );
        }
        settings::section("Keyboard")
            .child(settings::setting(
                "⌘1 – ⌘9 jump to",
                "Counted in sidebar order. A machine or project opens its first thread.",
                choices,
            ))
            .child(tag(
                "⌘K opens the command palette, which lists every command with its shortcut.",
                SUBTLE,
            ))
            .when_some(
                self.preference_note(Some(PreferenceGroup::Keyboard)),
                |el, note| el.child(tag(note, SUBTLE)),
            )
    }
}
