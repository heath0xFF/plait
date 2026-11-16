//! What the palette can run, the sources that list it, and the overlay that
//! hosts a palette above the workspace.
//!
//! To list something new: add a `Command` variant, a function that returns its
//! `Item`s, and one line in `command_items`. The palette view is unchanged.

use super::{
    Plait,
    actions::{ACTIONS, KEY_LABELS, Place},
    model_picker::{self, ModelPurpose, PickerStamp},
    palette::{Item, Options, Palette, PaletteEvent},
    theme::*,
};
use gpui::{prelude::*, *};
use plait_core::{host::ModelChoice, preferences};
use std::{collections::HashMap, time::Instant};

pub(super) enum Command {
    /// An application action, dispatched as its menu item or shortcut would be.
    Action(Box<dyn Action>),
    OpenThread(String),
    NewThread(ModelChoice),
    /// A model chosen in the model picker. The picker's purpose gives it meaning.
    Model(ModelChoice),
}

impl Clone for Command {
    fn clone(&self) -> Self {
        match self {
            Self::Action(action) => Self::Action(action.boxed_clone()),
            Self::OpenThread(id) => Self::OpenThread(id.clone()),
            Self::NewThread(choice) => Self::NewThread(choice.clone()),
            Self::Model(choice) => Self::Model(choice.clone()),
        }
    }
}

/// The palette or picker shown above the workspace. One at a time.
pub(super) struct Overlay {
    pub palette: Entity<Palette>,
    /// What a chosen model is for. `None` is the command palette.
    purpose: Option<ModelPurpose>,
    /// Where keyboard focus was, to put it back on Escape.
    restore: Option<FocusHandle>,
    /// The host state a model picker's rows were built from.
    pub stamp: Option<PickerStamp>,
    _events: Subscription,
}

/// The shortcut that runs `action` from the workspace, as the menu shows it.
fn shortcut(action: &dyn Action, window: &Window) -> String {
    let Ok(context) = KeyContext::parse("Plait") else {
        return String::new();
    };
    window
        .highest_precedence_binding_for_action_in_context(action, context)
        .map(|binding| {
            binding
                .keystrokes()
                .iter()
                .map(|keystroke| keystroke.display(KEY_LABELS).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

/// Every action in the table that has a menu item, with the shortcut it is
/// bound to now. The palette lists itself nowhere: it is already open.
fn action_items(window: &Window) -> Vec<Item> {
    ACTIONS
        .iter()
        .filter(|action| action.place != Place::Hidden && action.id != "palette.toggle")
        .map(|action| {
            let run = action.action();
            let hint = shortcut(run.as_ref(), window);
            Item::new(
                format!("action:{}", action.id),
                action.title,
                Command::Action(run),
            )
            .detail(action.category)
            .section("Commands")
            .hint(hint)
        })
        .collect()
}

impl Plait {
    /// Threads of the open checkout, by title, with the Cmd-number that reaches each.
    fn thread_items(&self) -> Vec<Item> {
        let count = self.host.threads.len();
        let target = self.preferences.value.jump_target;
        let mut numbers = HashMap::new();
        for number in 1..=9 {
            if let Some(jump) = preferences::jump(&self.state.projects, count, target, number) {
                numbers.entry(jump.thread).or_insert(number);
            }
        }
        let threads = self.host.threads.iter().enumerate();
        threads
            .map(|(index, thread)| {
                let model = model_picker::model_label(&thread.model);
                let item = Item::new(
                    format!("thread:{}", thread.id),
                    self.thread_title(&thread.id),
                    Command::OpenThread(thread.id.clone()),
                )
                .detail(format!("{} · {model}", thread.status.label()))
                .section("Threads")
                .glyph("thread", VIOLET)
                .hint(
                    numbers
                        .get(&index)
                        .map(|number| format!("⌘{number}"))
                        .unwrap_or_default(),
                );
                if self.host.selected.as_ref() == Some(&thread.id) {
                    item.badge("Open", SUBTLE)
                } else {
                    item
                }
            })
            .collect()
    }

    /// "New thread with <model>" for each signed-in model.
    fn new_thread_items(&self) -> Vec<Item> {
        let Some(status) = self.host.status.as_ref().filter(|_| self.host.connected) else {
            return Vec::new();
        };
        model_picker::usable_models(&status.models, &self.preferences.value)
            .into_iter()
            .map(|model| {
                let choice = model.choice();
                let label = model_picker::model_label(&choice);
                let default = status.default_model.as_ref() == Some(&choice);
                let item = Item::new(
                    format!("new-thread:{label}"),
                    format!("New thread with {}", model.name),
                    Command::NewThread(choice),
                )
                .detail(label)
                .section("New thread")
                .glyph("plus", MINT);
                if default {
                    item.badge("Default", MINT)
                } else {
                    item
                }
            })
            .collect()
    }

    /// Everything the command palette lists, in the order shown before typing.
    fn command_items(&self, window: &Window) -> Vec<Item> {
        let mut items = self.thread_items();
        items.extend(action_items(window));
        items.extend(self.new_thread_items());
        items
    }

    /// Cmd-K opens the palette, and closes it when it is already open.
    pub(super) fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = self
            .overlay
            .as_ref()
            .is_some_and(|overlay| overlay.purpose.is_none());
        self.close_overlay(true, window, cx);
        if open {
            return;
        }
        let items = self.command_items(window);
        let options = Options {
            name: "Command palette".into(),
            placeholder: "Search threads, commands, and models…".into(),
            empty: "Nothing to run yet.".into(),
            initial: None,
            mark: None,
            aside: None,
            note: None,
        };
        let palette = cx.new(|cx| Palette::new(options, items, window, cx));
        self.present(palette, None, window, cx);
    }

    /// Show `palette` above the workspace and give it the keyboard.
    pub(super) fn present(
        &mut self,
        palette: Entity<Palette>,
        purpose: Option<ModelPurpose>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_overlay(true, window, cx);
        let restore = window.focused(cx);
        let events = cx.subscribe_in(&palette, window, Self::on_palette_event);
        window.focus(&palette.focus_handle(cx), cx);
        self.overlay = Some(Overlay {
            palette,
            purpose,
            restore,
            stamp: purpose.map(|_| self.picker_stamp()),
            _events: events,
        });
        cx.notify();
    }

    /// Remove the overlay. `restore` returns focus to where it was.
    pub(super) fn close_overlay(
        &mut self,
        restore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        if restore {
            // Shortcuts need a focused element inside the root view.
            window.focus(overlay.restore.as_ref().unwrap_or(&self.focus), cx);
        }
        cx.notify();
    }

    /// A remark in the open overlay's footer, such as a failed save.
    pub(super) fn overlay_note(&mut self, note: Option<SharedString>, cx: &mut Context<Self>) {
        if let Some(overlay) = &self.overlay {
            overlay
                .palette
                .update(cx, |palette, cx| palette.set_note(note, cx));
        }
    }

    fn on_palette_event(
        &mut self,
        palette: &Entity<Palette>,
        event: &PaletteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A palette that was already replaced can still report a late blur.
        let Some(overlay) = self
            .overlay
            .as_ref()
            .filter(|open| open.palette == *palette)
        else {
            return;
        };
        let purpose = overlay.purpose;
        match event {
            PaletteEvent::Dismiss { restore } => self.close_overlay(*restore, window, cx),
            PaletteEvent::Mark(Command::Model(choice)) => self.toggle_favorite_model(choice, cx),
            PaletteEvent::Mark(_) => {}
            // The picker stays open; its rows and footer report the save.
            PaletteEvent::Aside(Command::Model(choice)) => {
                self.choose_default_model(choice.clone(), window, cx)
            }
            PaletteEvent::Aside(_) => {}

            PaletteEvent::Run(command) => {
                // Restore focus first, so an action runs where the user was.
                self.close_overlay(true, window, cx);
                self.run_command(command.clone(), purpose, window, cx);
            }
        }
    }

    fn run_command(
        &mut self,
        command: Command,
        purpose: Option<ModelPurpose>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            Command::Action(action) => window.dispatch_action(action, cx),
            Command::OpenThread(id) => {
                // The list can change while the palette is open.
                if self.host.threads.iter().any(|thread| thread.id == id) {
                    self.interaction = Some(Instant::now());
                    self.select_host_thread(id, window, cx);
                    self.focus_composer(window, cx);
                }
            }
            Command::NewThread(choice) => self.new_thread(Some(choice), window, cx),
            Command::Model(choice) => match purpose {
                Some(ModelPurpose::NewThread) => self.new_thread(Some(choice), window, cx),
                Some(ModelPurpose::DefaultModel) => self.choose_default_model(choice, window, cx),
                None => {}
            },
        }
    }
}
