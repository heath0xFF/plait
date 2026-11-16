//! Local commands offered when a draft starts with `/`.
//!
//! A command names something the app already does. The composer only reports
//! the choice; its parent performs it. To add one: add a `CommandId`, a row in
//! `COMMANDS`, and an arm where the parent handles `ComposerEvent::Command`.

use plait_core::fuzzy::fuzzy_match;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandId {
    NewThread,
    ShowChanges,
    ShowConversation,
    StopRun,
    OpenSettings,
    RefreshGit,
}

pub struct Command {
    pub id: CommandId,
    /// What the user types after `/`.
    pub name: &'static str,
    pub detail: &'static str,
    /// Offered only while the agent is working.
    pub needs_run: bool,
}

pub const COMMANDS: &[Command] = &[
    Command {
        id: CommandId::NewThread,
        name: "new",
        detail: "Start a new thread",
        needs_run: false,
    },
    Command {
        id: CommandId::ShowChanges,
        name: "changes",
        detail: "Show changed files and diffs",
        needs_run: false,
    },
    Command {
        id: CommandId::ShowConversation,
        name: "conversation",
        detail: "Show the conversation",
        needs_run: false,
    },
    Command {
        id: CommandId::StopRun,
        name: "stop",
        detail: "Stop the running agent",
        needs_run: true,
    },
    Command {
        id: CommandId::OpenSettings,
        name: "settings",
        detail: "Open settings",
        needs_run: false,
    },
    Command {
        id: CommandId::RefreshGit,
        name: "refresh",
        detail: "Refresh Git status",
        needs_run: false,
    },
];

/// A command that matches the typed query.
pub(super) struct CommandMatch {
    pub command: &'static Command,
    /// Matched byte offsets in the name, or in the detail when only it matched.
    pub name_matches: Vec<usize>,
    pub detail_matches: Vec<usize>,
}

/// Commands matching `query`, best first; ties keep the table's order. A name
/// match always ranks above a match found only in the description.
pub(super) fn matching(commands: &'static [Command], query: &str, busy: bool) -> Vec<CommandMatch> {
    let mut found: Vec<(bool, i32, CommandMatch)> = commands
        .iter()
        .filter(|command| busy || !command.needs_run)
        .filter_map(|command| {
            let by_name = fuzzy_match(query, command.name);
            let by_detail = by_name
                .is_none()
                .then(|| fuzzy_match(query, command.detail))
                .flatten();
            let score = by_name.as_ref().or(by_detail.as_ref())?.score;
            Some((
                by_name.is_some(),
                score,
                CommandMatch {
                    command,
                    name_matches: by_name.map(|found| found.positions).unwrap_or_default(),
                    detail_matches: by_detail.map(|found| found.positions).unwrap_or_default(),
                },
            ))
        })
        .collect();
    found.sort_by_key(|(by_name, score, _)| {
        (std::cmp::Reverse(*by_name), std::cmp::Reverse(*score))
    });
    found.into_iter().map(|(_, _, found)| found).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(query: &str, busy: bool) -> Vec<&'static str> {
        matching(COMMANDS, query, busy)
            .iter()
            .map(|found| found.command.name)
            .collect()
    }

    #[test]
    fn an_empty_query_lists_every_available_command_in_table_order() {
        assert_eq!(
            names("", false),
            ["new", "changes", "conversation", "settings", "refresh"]
        );
        assert_eq!(names("", true).len(), COMMANDS.len());
        assert!(names("", true).contains(&"stop"));
    }

    #[test]
    fn typing_narrows_by_name_first_and_description_second() {
        assert_eq!(names("sett", false), ["settings"]);
        assert_eq!(names("set", false)[0], "settings");
        assert_eq!(names("st", false)[0], "settings");
        assert!(!names("st", false).contains(&"stop"));
        assert_eq!(names("st", true)[0], "stop");
        assert_eq!(names("c", false)[..2], ["changes", "conversation"]);
        // "git" is only in a description; "thread" too.
        assert_eq!(names("git", false), ["refresh"]);
        assert_eq!(names("thread", false), ["new"]);
        assert!(names("zzz", true).is_empty());
        let by_detail = &matching(COMMANDS, "git", false)[0];
        assert!(by_detail.name_matches.is_empty());
        assert_eq!(by_detail.detail_matches, [8, 9, 10]);
    }

    #[test]
    fn command_names_are_unique_single_words() {
        for (index, command) in COMMANDS.iter().enumerate() {
            assert!(!command.name.is_empty() && !command.detail.is_empty());
            assert!(command.name.chars().all(|ch| ch.is_ascii_lowercase()));
            assert!(
                COMMANDS[..index]
                    .iter()
                    .all(|other| other.name != command.name && other.id != command.id)
            );
        }
    }
}
