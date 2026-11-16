//! Every application action in one table.
//!
//! The table is the single source for key bindings, the menu bar, the command
//! palette's entries and shortcut hints, and the keyboard table in
//! docs/MACOS.md (a test compares them). Shortcuts are bound only by `bind`,
//! which takes a map of action ID to keystroke, so a saved keymap can replace
//! any default without touching the table.
//!
//! Editing keys that belong to one control (the composer, the text field, and
//! the palette's list) are bound by those controls and are not listed here.

use super::{jump::*, *};
use std::{collections::BTreeMap, rc::Rc};

/// Where an action appears besides its shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Place {
    /// A menu item and a command palette entry.
    Menu,
    /// The same, starting a new group in its menu.
    MenuGroup,
    /// Shortcut only: movement that needs no menu item.
    Hidden,
}

pub(super) struct AppAction {
    /// Stable identifier, the key of a keymap override. Never rename or reuse one.
    pub id: &'static str,
    pub title: &'static str,
    /// The menu the action sits in, and its group in a shortcut editor.
    pub category: &'static str,
    /// Default keystroke in GPUI notation, such as `cmd-shift-n`.
    pub key: Option<&'static str>,
    pub place: Place,
    build: fn() -> Box<dyn Action>,
}

impl AppAction {
    pub fn action(&self) -> Box<dyn Action> {
        (self.build)()
    }

    /// Quit must work with no window focused; everything else needs the root view.
    fn context(&self) -> Option<&'static str> {
        (self.id != "app.quit").then_some("Plait")
    }
}

const fn entry(
    id: &'static str,
    title: &'static str,
    category: &'static str,
    key: Option<&'static str>,
    place: Place,
    build: fn() -> Box<dyn Action>,
) -> AppAction {
    AppAction {
        id,
        title,
        category,
        key,
        place,
        build,
    }
}

use Place::{Hidden, Menu as Item, MenuGroup as Group};

/// In menu order. Categories are contiguous; each becomes one menu.
#[rustfmt::skip]
pub(super) const ACTIONS: &[AppAction] = &[
    entry("app.settings", "Settings…", "Plait", Some("cmd-,"), Item, || Box::new(Settings)),
    entry("app.check_for_updates", "Check for Updates…", "Plait", None, Item, || Box::new(CheckForUpdates)),
    entry("app.quit", "Quit Plait", "Plait", Some("cmd-q"), Group, || Box::new(Quit)),
    entry("thread.new", "New Thread", "File", Some("cmd-n"), Item, || Box::new(NewThread)),
    entry("thread.new_with_model", "New Thread with Model…", "File", Some("cmd-shift-n"), Item, || Box::new(NewThreadWithModel)),
    entry("repository.open", "Open Repository…", "File", Some("cmd-o"), Group, || Box::new(OpenRepository)),
    entry("git.refresh", "Refresh Git Status", "File", Some("cmd-r"), Item, || Box::new(Refresh)),
    entry("window.close", "Close Window", "File", Some("cmd-w"), Group, || Box::new(Close)),
    entry("palette.toggle", "Command Palette…", "View", Some("cmd-k"), Item, || Box::new(CommandPalette)),
    entry("view.toggle_sidebar", "Toggle Sidebar", "View", Some("cmd-b"), Group, || Box::new(ToggleSidebar)),
    entry("view.toggle_inspector", "Toggle Git Inspector", "View", Some("cmd-alt-b"), Item, || Box::new(ToggleInspector)),
    entry("view.toggle_diagnostics", "Toggle Diagnostics", "View", Some("cmd-shift-d"), Item, || Box::new(ToggleDiagnostics)),
    entry("view.conversation", "Conversation", "View", Some("cmd-shift-1"), Item, || Box::new(Conversation)),
    entry("view.changes", "Changes", "View", Some("cmd-shift-2"), Item, || Box::new(Changes)),
    entry("thread.next", "Next Thread", "View", Some("cmd-alt-down"), Group, || Box::new(NextThread)),
    entry("thread.previous", "Previous Thread", "View", Some("cmd-alt-up"), Item, || Box::new(PreviousThread)),
    entry("git.next_file", "Next Changed File", "View", Some("alt-down"), Item, || Box::new(NextFile)),
    entry("git.previous_file", "Previous Changed File", "View", Some("alt-up"), Item, || Box::new(PreviousFile)),
    entry("window.minimize", "Minimize", "Window", Some("cmd-m"), Item, || Box::new(Minimize)),
    entry("jump.1", "Jump to Sidebar Item 1", "Navigation", Some("cmd-1"), Hidden, || Box::new(Jump1)),
    entry("jump.2", "Jump to Sidebar Item 2", "Navigation", Some("cmd-2"), Hidden, || Box::new(Jump2)),
    entry("jump.3", "Jump to Sidebar Item 3", "Navigation", Some("cmd-3"), Hidden, || Box::new(Jump3)),
    entry("jump.4", "Jump to Sidebar Item 4", "Navigation", Some("cmd-4"), Hidden, || Box::new(Jump4)),
    entry("jump.5", "Jump to Sidebar Item 5", "Navigation", Some("cmd-5"), Hidden, || Box::new(Jump5)),
    entry("jump.6", "Jump to Sidebar Item 6", "Navigation", Some("cmd-6"), Hidden, || Box::new(Jump6)),
    entry("jump.7", "Jump to Sidebar Item 7", "Navigation", Some("cmd-7"), Hidden, || Box::new(Jump7)),
    entry("jump.8", "Jump to Sidebar Item 8", "Navigation", Some("cmd-8"), Hidden, || Box::new(Jump8)),
    entry("jump.9", "Jump to Sidebar Item 9", "Navigation", Some("cmd-9"), Hidden, || Box::new(Jump9)),
    entry("focus.next", "Focus Next Control", "Navigation", Some("tab"), Hidden, || Box::new(Tab)),
    entry("focus.previous", "Focus Previous Control", "Navigation", Some("shift-tab"), Hidden, || Box::new(BackTab)),
];

/// Keystrokes that replace the defaults, by action ID. An empty keystroke
/// leaves the action unbound. Ordered, so binding is deterministic.
pub(super) type KeyOverrides = BTreeMap<String, String>;

/// The platform whose key names label shortcuts.
pub(super) const KEY_LABELS: OperatingSystem = if cfg!(target_os = "macos") {
    OperatingSystem::Mac
} else {
    OperatingSystem::Linux
};

fn parses(keystrokes: &str) -> bool {
    let mut parts = keystrokes.split_whitespace().peekable();
    parts.peek().is_some()
        && parts.all(|keystroke| Keystroke::parse_with_os(keystroke, KEY_LABELS).is_ok())
}

/// The keystroke an action is bound to: its override when that is usable,
/// otherwise its default. `None` means unbound.
pub(super) fn keystroke<'a>(action: &'a AppAction, overrides: &'a KeyOverrides) -> Option<&'a str> {
    match overrides.get(action.id).map(String::as_str) {
        Some("") => None,
        // A keystroke that does not parse must not cost the user the default.
        Some(custom) if parses(custom) => Some(custom),
        _ => action.key,
    }
}

/// Bind every application shortcut. This is the only place they are bound.
pub(super) fn bind(cx: &mut App, overrides: &KeyOverrides) {
    let bindings = ACTIONS
        .iter()
        .filter_map(|action| {
            let context = action
                .context()
                .and_then(|context| KeyBindingContextPredicate::parse(context).ok())
                .map(Rc::new);
            let keys = keystroke(action, overrides)?;
            KeyBinding::load(keys, action.action(), context, false, None, cx).ok()
        })
        .collect::<Vec<_>>();
    cx.bind_keys(bindings);
}

/// The menu bar: one menu per category, in table order.
fn menus() -> Vec<Menu> {
    let mut menus: Vec<(&str, Vec<MenuItem>)> = Vec::new();
    for action in ACTIONS.iter().filter(|action| action.place != Hidden) {
        if menus
            .last()
            .is_none_or(|(name, _)| *name != action.category)
        {
            menus.push((action.category, Vec::new()));
        }
        let Some((_, items)) = menus.last_mut() else {
            continue;
        };
        if action.place == Group && !items.is_empty() {
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::Action {
            name: action.title.into(),
            action: action.action(),
            os_action: None,
            checked: false,
            disabled: false,
        });
    }
    menus
        .into_iter()
        .map(|(name, items)| Menu::new(name).items(items))
        .collect()
}

/// Bind all keys and build the menus. Call again with a new keymap to rebind:
/// the previous bindings are cleared first, and the menus pick up the new keys.
pub(super) fn install(cx: &mut App, overrides: &KeyOverrides) {
    cx.clear_key_bindings();
    // Controls bind their own editing keys.
    composer::bind_keys(cx);
    text_field::bind_keys(cx);
    palette::bind_keys(cx);
    bind(cx, overrides);
    cx.set_menus(menus());
}

/// `cmd-shift-n` as documentation writes it: `Cmd-Shift-N`.
#[cfg(test)]
fn documented(keystroke: &str) -> String {
    let parts = keystroke.split('-').map(|part| match part {
        "cmd" => "Cmd".to_owned(),
        "alt" => "Option".to_owned(),
        "ctrl" => "Control".to_owned(),
        "," => "comma".to_owned(),
        part => {
            let mut letters = part.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(letters).collect()
            })
        }
    });
    parts.collect::<Vec<_>>().join("-")
}

/// The default shortcuts as the Markdown table in docs/MACOS.md.
#[cfg(test)]
fn keyboard_table() -> String {
    let mut table = String::from("| Shortcut | Action | ID |\n| --- | --- | --- |\n");
    for action in ACTIONS {
        if let Some(key) = action.key {
            table.push_str(&format!(
                "| {} | {} | `{}` |\n",
                documented(key),
                action.title,
                action.id
            ));
        }
    }
    table
}

#[cfg(test)]
mod tests {
    // Named imports: a glob would bring in GPUI's own `test` attribute.
    use super::{ACTIONS, KeyOverrides, documented, keyboard_table, keystroke, menus, parses};
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_default_keystrokes_parse_without_collisions() {
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        let mut names = HashSet::new();
        for action in ACTIONS {
            assert!(ids.insert(action.id), "duplicate ID {}", action.id);
            assert!(
                names.insert(action.action().name()),
                "{} repeats an action",
                action.id
            );
            assert!(!action.title.is_empty() && !action.category.is_empty());
            if let Some(key) = action.key {
                assert!(parses(key), "{key} does not parse");
                assert!(keys.insert(key), "{key} is bound twice");
            }
        }
    }

    #[test]
    fn each_category_is_one_contiguous_menu() {
        let mut seen = Vec::new();
        for action in ACTIONS {
            if seen.last() != Some(&action.category) {
                assert!(
                    !seen.contains(&action.category),
                    "{} is split",
                    action.category
                );
                seen.push(action.category);
            }
        }
        let names = menus()
            .iter()
            .map(|menu| menu.name.to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Plait", "File", "View", "Window"]);
    }

    #[test]
    fn an_override_replaces_the_default_and_a_bad_one_does_not_remove_it() {
        let new_thread = ACTIONS.iter().find(|a| a.id == "thread.new").unwrap();
        let updates = ACTIONS
            .iter()
            .find(|a| a.id == "app.check_for_updates")
            .unwrap();
        let mut overrides = KeyOverrides::new();
        assert_eq!(keystroke(new_thread, &overrides), Some("cmd-n"));
        assert_eq!(keystroke(updates, &overrides), None);
        overrides.insert("thread.new".into(), "cmd-t".into());
        overrides.insert("app.check_for_updates".into(), "cmd-shift-u".into());
        assert_eq!(keystroke(new_thread, &overrides), Some("cmd-t"));
        assert_eq!(keystroke(updates, &overrides), Some("cmd-shift-u"));
        // Empty unbinds; a keystroke that does not parse falls back.
        overrides.insert("thread.new".into(), String::new());
        assert_eq!(keystroke(new_thread, &overrides), None);
        overrides.insert("thread.new".into(), "cmd-n-x".into());

        assert_eq!(keystroke(new_thread, &overrides), Some("cmd-n"));
        overrides.insert("unknown.action".into(), "cmd-u".into());
        assert_eq!(keystroke(new_thread, &overrides), Some("cmd-n"));
    }

    #[test]
    fn keystrokes_are_documented_in_words() {
        assert_eq!(documented("cmd-shift-n"), "Cmd-Shift-N");
        assert_eq!(documented("cmd-,"), "Cmd-comma");
        assert_eq!(documented("cmd-alt-down"), "Cmd-Option-Down");
        assert_eq!(documented("shift-tab"), "Shift-Tab");
    }

    #[test]
    fn the_documented_keyboard_table_is_this_table() {
        let documentation = include_str!("../../../../docs/MACOS.md");
        let table = keyboard_table();
        assert!(
            documentation.contains(&table),
            "docs/MACOS.md is out of date. Replace its application shortcut table with:\n\n{table}"
        );
    }
}
