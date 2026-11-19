//! Headless accessibility tree guard: duplicate node ID detection and
//! steady-frame reachability of required controls across workspace states.

use super::{
    Harness, SettingsPage, SettingsTab, fixture_repository, isolated, sample_harnesses, tabs,
};
use crate::ui::{Workspace, a11y::A11yFrame, settings_agent::AgentView};
use gpui::Role;
use plait_core::{
    handoff::{EditorChoice, TerminalChoice},
    host::{
        MessageRole,
        agent::{
            AgentResources, Instructions, McpServer, McpTransport, Origin, Problem, Skill, Template,
        },
        policy::CommandPolicy,
    },
};
use std::{path::Path, sync::Arc};

/// Draw two consecutive frames with the collector active and return frame 2.
///
/// Frame 1 builds child views; frame 2 is the steady frame where a `.cached()`
/// view would replay cached paint without running child prepaint, dropping its
/// accessibility nodes just as GPUI's AccessKit collector does.
fn steady_frame(harness: &mut Harness, scene: &str) -> A11yFrame {
    harness
        .draw_a11y()
        .assert_no_duplicates(&format!("{scene} (frame 1)"));
    let second = harness.draw_a11y();
    second.assert_no_duplicates(&format!("{scene} (frame 2)"));
    second
}

fn assert_views_reachable(frame: &A11yFrame, scene: &str) {
    for view in ["Conversation", "Changes", "History", "Files"] {
        assert!(
            frame.has_node(Role::Tab, |node| node.label.as_deref() == Some(view)),
            "{scene}: missing view tab {view:?} in accessibility tree"
        );
    }
}

fn assert_workspace_reachable(
    harness: &mut Harness,
    frame: &A11yFrame,
    scene: &str,
    all_messages: bool,
) {
    let mut expected_threads = Vec::new();
    let mut expected_tabs = Vec::new();
    let mut expected_messages = Vec::new();
    let mut expected_streaming = None;
    harness.update(|plait, _, _| {
        for thread in &plait.host.connection.threads {
            expected_threads.push((thread.id.clone(), plait.thread_title(&thread.id)));
        }
        for meta in &plait.tabs.rows {
            expected_tabs.push((
                format!("{} — {}", meta.title, meta.state.word(true)),
                format!("Close {}", meta.title),
            ));
        }
        let snapshot = plait
            .host
            .thread
            .snapshot
            .as_ref()
            .expect("thread snapshot");
        for message in &snapshot.messages {
            let author = if message.role == MessageRole::User {
                "Your"
            } else {
                "Plait"
            };
            expected_messages.push(format!("{author} message: {}", message.text));
        }
        if !snapshot.live_text.is_empty() {
            expected_streaming = Some(format!("Plait is writing: {}", snapshot.live_text));
        }
    });

    assert!(
        expected_threads.len() >= 3,
        "{scene}: expected multiple sidebar threads"
    );
    for (id, title) in &expected_threads {
        let prefix = format!("Thread {id} · ");
        assert!(
            frame.has_node(Role::TreeItem, |node| {
                node.label
                    .as_deref()
                    .is_some_and(|label| label.starts_with(&prefix) && label.contains(title))
            }),
            "{scene}: missing sidebar thread row for {id:?} ({title:?}) on frame 2"
        );
    }

    assert!(
        expected_tabs.len() >= 3,
        "{scene}: expected multiple visible tabs"
    );
    for (tab_name, close_name) in &expected_tabs {
        assert!(
            frame.has_node(Role::Tab, |node| {
                node.label.as_deref() == Some(tab_name.as_str())
            }),
            "{scene}: missing tab {tab_name:?} on frame 2"
        );
        assert!(
            frame.has_node(Role::Button, |node| {
                node.label.as_deref() == Some(close_name.as_str())
            }),
            "{scene}: missing tab close control {close_name:?} on frame 2"
        );
    }

    assert_views_reachable(frame, scene);

    assert!(
        !expected_messages.is_empty(),
        "{scene}: expected conversation messages"
    );
    let checked_messages = if all_messages {
        expected_messages.as_slice()
    } else {
        // When a tall approval card shares the viewport, the bottom-aligned
        // virtualized list may scroll the earliest message above the fold.
        &expected_messages[expected_messages.len() - 1..]
    };
    for message in checked_messages {
        assert!(
            frame.has_node(Role::Group, |node| {
                node.label.as_deref() == Some(message.as_str())
            }),
            "{scene}: missing message node {message:?} on frame 2"
        );
    }
    if let Some(streaming) = expected_streaming {
        assert!(
            frame.has_node(Role::Group, |node| {
                node.label.as_deref() == Some(streaming.as_str())
            }),
            "{scene}: missing streaming reply node {streaming:?} on frame 2"
        );
    }
}

fn project_agent_resources(checkout: &Path) -> AgentResources {
    let skill = |name: &str, origin| Skill {
        name: name.into(),
        description: "Repeated skill description to exercise row uniqueness.".into(),
        origin,
        path: format!("/opt/sample-agent/skills/{name}/SKILL.md").into(),
        model_invocation: true,
    };
    AgentResources {
        checkout: checkout.into(),
        trusted: false,
        instructions: Some(Instructions {
            path: "/opt/sample-agent/AGENTS.md".into(),
            bytes: 2150,
        }),
        skills: (0..4)
            .map(|index| skill(&format!("sample-skill-{index:02}"), Origin::User))
            .collect(),
        templates: vec![Template {
            name: "review".into(),
            description: "Review a change".into(),
            argument_hint: Some("<path> [focus]".into()),
            origin: Origin::User,
            path: "/opt/sample-agent/prompts/review.md".into(),
        }],
        problems: vec![Problem {
            path: Some("/opt/sample-agent/skills/broken/SKILL.md".into()),
            message: "description is required".into(),
        }],
        mcp_servers: vec![McpServer {
            name: "local-docs".into(),
            transport: Some(McpTransport::Stdio),
            source: "/opt/sample-agent/mcp.json".into(),
            override_source: None,
            program: Some("uvx".into()),
            argument_count: Some(2),
            host: None,
            enabled: true,
            can_enable: true,
            needs_sign_in: Some(false),
            signed_in: false,
            disabled_reason: None,
            problem: None,
            state: None,
            tool_count: None,
            definition: Some("a".repeat(64)),
            consent: None,
        }],
    }
}

fn assert_settings_modals(harness: &mut Harness) {
    harness.update(|plait, window, cx| {
        plait.inject_harnesses(Some(sample_harnesses("ready", None)));
        // Two customized shortcuts exercise repeated "Customized" badges.
        plait
            .preferences
            .value
            .keymap
            .insert("thread.new".into(), "cmd-shift-n".into());
        plait
            .preferences
            .value
            .keymap
            .insert("view.refresh".into(), "cmd-shift-r".into());
        plait.open_page(Some(SettingsPage::Application), window, cx);
    });
    for tab in SettingsTab::ALL {
        harness.update(|plait, _, _| plait.settings_tab = tab);
        let scene = format!("settings-modal-{}", tab.label());
        let frame = steady_frame(harness, &scene);
        assert!(
            frame.has_node(Role::Dialog, |node| node.label.as_deref()
                == Some("Settings")),
            "{scene}: missing Settings modal dialog"
        );
    }

    // Editor both-Custom state (matching scripts/acceptance/fixture.py): both
    // settings render the identical note and must stay distinct in the tree.
    let base = "/private/tmp/plait-acceptance-bx6m5w3v";
    let recorder = format!("{base}/record-arguments");
    harness.update(|plait, _, cx| {
        plait.settings_tab = SettingsTab::Editor;
        plait.preferences.value.open_files_with = EditorChoice::Custom(format!(
            "\"{recorder}\" \"{base}/editor-args.txt\" \"{{file}}\" \"{{line}}\" \"{{project}}\""
        ));
        plait.preferences.value.open_terminals_with = TerminalChoice::Custom(format!(
            "\"{recorder}\" \"{base}/terminal-args.txt\" \"{{project}}\""
        ));
        plait.show_editor_commands(cx);
    });
    let custom_frame = steady_frame(harness, "settings-editor-both-custom");
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

    // Project settings modal with policy, MCP servers, and repeated agent rows.
    harness.update(|plait, window, cx| {
        plait.show_project_settings(window, cx);
        plait.policy.key = Some((
            plait.host.connection.epoch,
            plait.host.creation.checkout.clone(),
        ));
        plait.policy.value = Some(CommandPolicy::Sandboxed);
        plait.agent.key = Some((
            plait.host.connection.epoch,
            plait.host.creation.checkout.clone(),
        ));
        plait.agent.view = Some(Arc::new(AgentView::new(project_agent_resources(
            &plait.host.creation.checkout,
        ))));
    });
    let project_frame = steady_frame(harness, "project-settings-modal");
    assert!(
        project_frame.has_node(Role::Dialog, |node| {
            node.label.as_deref() == Some("Project settings")
        }),
        "missing Project settings dialog"
    );
    let open_folder_buttons = project_frame
        .nodes
        .iter()
        .filter(|node| node.role == Role::Button && node.label.as_deref() == Some("Open folder"))
        .count();
    assert!(
        open_folder_buttons >= 5,
        "expected Open folder buttons for agent rows, got {open_folder_buttons}"
    );
    harness.update(|plait, window, cx| plait.show_settings(false, window, cx));
}

fn assert_views_and_overlays(harness: &mut Harness, repository: &Path) {
    // Changes, History, and Files views.
    harness.update(|plait, window, cx| plait.show_tab(Workspace::Changes, window, cx));
    let changes = steady_frame(harness, "workspace-changes");
    assert_views_reachable(&changes, "workspace-changes");

    harness.update(|plait, window, cx| plait.show_tab(Workspace::History, window, cx));
    harness.settle();
    let history = steady_frame(harness, "workspace-history");
    assert_views_reachable(&history, "workspace-history");

    harness.update(|plait, _, cx| plait.open_file("src/review.rs".into(), Some(1), cx));
    harness.settle();
    let files = steady_frame(harness, "workspace-files");
    assert_views_reachable(&files, "workspace-files");

    // Approval card over the conversation with tabs and sidebar.
    harness.connect(repository, true);
    tabs::install(harness, 3);
    harness.update(|plait, window, cx| plait.show_tab(Workspace::Conversation, window, cx));
    let approval = steady_frame(harness, "workspace-approval");
    assert_workspace_reachable(harness, &approval, "workspace-approval", false);
    assert!(
        approval.has_node(Role::Button, |node| node.label.as_deref()
            == Some("Deny once")),
        "workspace-approval: missing Deny once button"
    );
    assert!(
        approval.has_node(Role::Button, |node| node.label.as_deref()
            == Some("Allow once")),
        "workspace-approval: missing Allow once button"
    );

    // Tab overflow list open with 20 tabs.
    tabs::install(harness, 20);
    harness.update(|plait, _, _| plait.tabs.list_open = true);
    let overflow = steady_frame(harness, "tabs-overflow-open");
    assert!(
        overflow.has_node(Role::Menu, |node| node.label.as_deref()
            == Some("All open tabs")),
        "tabs-overflow-open: missing All open tabs menu"
    );
    assert!(
        overflow.has_node(Role::MenuItem, |node| {
            node.label.as_deref() == Some("Improve retry handling")
        }),
        "tabs-overflow-open: missing tab menu item"
    );
}

#[core::prelude::v1::test]
fn a11y_tree_has_no_duplicates_and_exposes_required_nodes() {
    if !isolated("a11y::a11y_tree_has_no_duplicates_and_exposes_required_nodes") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-a11y-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let repository = fixture_repository(&scratch);
    let mut harness = Harness::new(&repository, None, 1380., 860.);
    harness.connect(&repository, false);
    tabs::install(&mut harness, 3);

    // Streaming reply with multiple tabs and sidebar thread rows.
    let streaming = steady_frame(&mut harness, "workspace-streaming");
    assert_workspace_reachable(&mut harness, &streaming, "workspace-streaming", true);

    // Settings and Project settings as modals over the live workspace.
    assert_settings_modals(&mut harness);

    // Changes, History, Files, approval, and the open tab overflow list.
    assert_views_and_overlays(&mut harness, &repository);

    drop(harness);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every `.rs` file under `src/ui`, except this guard, with comments and the
/// contents of string literals blanked to spaces. Offsets and line breaks are
/// kept, so a hit maps back to its line.
fn ui_sources() -> Vec<(std::path::PathBuf, String)> {
    fn visit(dir: &Path, files: &mut Vec<(std::path::PathBuf, String)>) {
        for entry in std::fs::read_dir(dir).expect("read ui dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                visit(&path, files);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs")
                && !path.ends_with("ui/snapshot/a11y.rs")
            {
                let source = std::fs::read_to_string(&path).expect("read ui source");
                files.push((path, code_only(&source)));
            }
        }
    }
    let mut files = Vec::new();
    visit(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui"),
        &mut files,
    );
    files.sort();
    files
}

/// `source` with line comments and string contents replaced by spaces, so
/// brackets and `//` inside text cannot mislead the checks below.
fn code_only(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let (mut index, mut in_string) = (0, false);
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if byte == b'\\' {
                out[index] = b' ';
                if index + 1 < bytes.len() && bytes[index + 1] != b'\n' {
                    out[index + 1] = b' ';
                }
                index += 2;
                continue;
            }
            if byte == b'"' {
                in_string = false;
            } else if byte != b'\n' {
                out[index] = b' ';
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b'\'' && bytes.get(index + 2) == Some(&b'\'') {
            // A one-byte char literal such as '(' or '"'.
            out[index + 1] = b' ';
            index += 3;
            continue;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && bytes[index] != b'\n' {
                out[index] = b' ';
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    String::from_utf8(out).expect("blanking keeps UTF-8 boundaries")
}

/// `path:line` for a hit, relative to the crate so the message reads the same anywhere.
fn place(path: &Path, code: &str, offset: usize) -> String {
    let path = path
        .strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(path);
    let line = code[..offset].matches('\n').count() + 1;
    format!("{}:{line}", path.display())
}

/// Every accessible text element in `ui/` must go through `a11y::text` (via
/// `a11y_text`, `tag`, or `section_label`) with an explicit place-based ID.
#[core::prelude::v1::test]
fn no_raw_text_macro_in_ui() {
    let mut hits = Vec::new();
    for (path, code) in ui_sources() {
        for (offset, _) in code.match_indices("text!(") {
            hits.push(place(&path, &code, offset));
        }
    }
    assert!(
        hits.is_empty(),
        "raw text!() bypasses place-based accessibility IDs and the collector:\n{}",
        hits.join("\n")
    );
}

/// Names of functions in `ui/` that return an element already wrapped in
/// `Audited`, so a chain starting from one of them is seen by the collector.
fn audited_constructors(files: &[(std::path::PathBuf, String)]) -> Vec<String> {
    let mut names = Vec::new();
    for (_, code) in files {
        for (offset, _) in code.match_indices("fn ") {
            let rest = &code[offset + 3..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let signature = &rest[..rest.find(['{', ';']).unwrap_or(rest.len())];
            let returns = signature.rsplit("->").next().unwrap_or("").trim();
            if signature.contains("->")
                && ["Control", "Option<Control>", "Audited<"]
                    .iter()
                    .any(|wrapped| returns.starts_with(wrapped))
            {
                names.push(name);
            }
        }
    }
    names
}

/// The builder chain around `at`: from the expression's start to where the
/// chain ends at the same bracket depth.
fn chain_around(code: &[u8], at: usize) -> (usize, usize) {
    let mut start = 0;
    let mut depth = 0usize;
    for index in (0..at).rev() {
        match code[index] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth == 0 => {
                start = index + 1;
                break;
            }
            b'(' | b'[' | b'{' => depth -= 1,
            b',' | b';' | b'|' | b'=' if depth == 0 => {
                start = index + 1;
                break;
            }
            _ => {}
        }
    }
    let mut end = code.len();
    depth = 0;
    for (index, byte) in code.iter().enumerate().skip(at) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => {
                end = index;
                break;
            }
            b')' | b']' | b'}' => depth -= 1,
            b',' | b';' if depth == 0 => {
                end = index;
                break;
            }
            _ => {}
        }
    }
    (start, end)
}

/// Whether the chain containing `at` reaches the collector: it calls
/// `.audited()`, starts from an audited constructor, or continues a chain
/// (a closure's parameter, or a `let` binding audited later) that does.
fn chain_is_audited(code: &str, at: usize, constructors: &[String]) -> bool {
    let bytes = code.as_bytes();
    let (start, end) = chain_around(bytes, at);
    let chain = &code[start..end];
    if chain.contains(".audited()") {
        return true;
    }
    let head = chain.trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '_'));
    let head = head.strip_prefix("self.").unwrap_or(head);
    let name: String = head
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let after = head[name.len()..].trim_start();
    if (after.starts_with('(') || after.starts_with("::")) && constructors.contains(&name) {
        return true;
    }
    if start > 0 && bytes[start - 1] == b'|' {
        // `|el| el.role(..)`: the closure is an argument of the outer chain.
        let mut depth = 0usize;
        for index in (0..start).rev() {
            match bytes[index] {
                b')' | b']' | b'}' => depth += 1,
                b'(' if depth == 0 => return chain_is_audited(code, index, constructors),
                b'(' | b'[' | b'{' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        return false;
    }
    if start > 0 && bytes[start - 1] == b'=' {
        // `let name = ..`: audited where the binding is used.
        let binding = code[..start - 1]
            .trim_end()
            .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        let later = &code[end..];
        return !binding.is_empty()
            && (later.contains(&format!("{binding}.audited()"))
                || later.contains(&format!("{binding}\n.audited()")));
    }
    false
}

/// GPUI gives a node to every element with an id and a role, and to every
/// `Text` with an id. The collector sees only what is wrapped in `Audited`, so
/// an element built any other way could repeat an id unseen. This fails on a
/// `.role(` chain that never reaches `Audited`, and on `Text::new` or
/// `InteractiveText` outside `ui/a11y.rs`.
#[core::prelude::v1::test]
fn every_accessible_element_is_audited() {
    let files = ui_sources();
    let constructors = audited_constructors(&files);
    let mut hits = Vec::new();
    for (path, code) in &files {
        let own = path.ends_with("ui/a11y.rs");
        for (offset, _) in code.match_indices(".role(") {
            if !own && !chain_is_audited(code, offset, &constructors) {
                hits.push(format!("{} .role(", place(path, code, offset)));
            }
        }
        for raw in ["Text::new(", "InteractiveText"] {
            for (offset, _) in code.match_indices(raw) {
                let styled = code[..offset].ends_with("Styled");
                if !own && !styled {
                    hits.push(format!("{} {raw}", place(path, code, offset)));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "accessible elements the collector cannot see; wrap them with `.audited()` \
         or build them with an audited helper:\n{}",
        hits.join("\n")
    );
}
