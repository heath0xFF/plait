//! Headless accessibility tree checks: duplicate node ID detection and
//! reachability of required controls across frames.

use super::{Harness, SettingsPage, SettingsTab, fixture_repository, isolated};
use gpui::Role;
use plait_core::handoff::{EditorChoice, TerminalChoice};

#[core::prelude::v1::test]
fn editor_page_a11y_tree_is_observable_headlessly() {
    if !isolated("a11y::editor_page_a11y_tree_is_observable_headlessly") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-a11y-repro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let repository = fixture_repository(&scratch);
    let mut harness = Harness::new(&repository, None, 1380., 860.);
    harness.connect(&repository, false);

    // Default Editor state (System default + Terminal): no duplicate IDs.
    harness.update(|plait, window, cx| {
        plait.open_page(Some(SettingsPage::Application), window, cx);
        plait.settings_tab = SettingsTab::Editor;
    });
    let default_frame = harness.draw_a11y();
    default_frame.assert_no_duplicates("settings-editor-default");
    assert!(default_frame.has_node(Role::Dialog, |node| {
        node.label.as_deref() == Some("Settings")
    }));

    // Both-Custom state, as scripts/acceptance/fixture.py writes before launch.
    let base = "/private/tmp/plait-acceptance-bx6m5w3v";
    let recorder = format!("{base}/record-arguments");
    harness.update(|plait, _, cx| {
        plait.preferences.value.open_files_with = EditorChoice::Custom(format!(
            "\"{recorder}\" \"{base}/editor-args.txt\" \"{{file}}\" \"{{line}}\" \"{{project}}\""
        ));
        plait.preferences.value.open_terminals_with = TerminalChoice::Custom(format!(
            "\"{recorder}\" \"{base}/terminal-args.txt\" \"{{project}}\""
        ));
        plait.show_editor_commands(cx);
    });
    let custom_frame = harness.draw_a11y();
    custom_frame.assert_no_duplicates("settings-editor-both-custom");
    let expected_note = "Runs /private/tmp/plait-acceptance-bx6m5w3v/record-arguments directly, not through a shell.";
    let note_nodes: Vec<_> = custom_frame
        .nodes
        .iter()
        .filter(|node| node.role == Role::Label && node.value.as_deref() == Some(expected_note))
        .collect();
    assert_eq!(
        note_nodes.len(),
        2,
        "both Open files with and Open terminals with notes must stay in the a11y tree"
    );
    assert_ne!(note_nodes[0].node_id, note_nodes[1].node_id);

    drop(harness);
    let _ = std::fs::remove_dir_all(&scratch);
}
