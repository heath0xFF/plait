//! Injected tab states; no fixture data reaches the shipped view.
use super::*;
use plait_core::tabs::{CheckoutKey, ThreadTarget};

pub(super) fn install(harness: &mut Harness, count: usize) {
    install_projects(harness, count, false);
}

fn install_projects(harness: &mut Harness, count: usize, projects: bool) {
    harness.update(move |plait, _, _| {
        plait.tabs = Default::default();
        let template = plait
            .host
            .connection
            .threads
            .first()
            .cloned()
            .unwrap_or_default();
        plait.host.connection.threads.clear();
        for index in 0..count {
            let mut thread = template.clone();
            thread.id = if index == 0 {
                THREAD.into()
            } else {
                format!("tab-thread-{index}")
            };
            thread.title = Some(match index {
                0 => "Improve retry handling".into(),
                1 => "Run the parser tests".into(),
                2 => "Review dependency update".into(),
                _ => format!("Task {}: refine checkout handling", index + 1),
            });
            thread.status = match index % 7 {
                0 | 3 | 5 => ThreadStatus::Idle,
                1 => ThreadStatus::Running,
                2 => ThreadStatus::AwaitingApproval,
                4 => ThreadStatus::Error,
                _ => ThreadStatus::Stopped,
            };
            if projects && count == 3 && index > 0 {
                thread.checkout = thread.checkout.parent().unwrap().join(if index == 1 {
                    "tools"
                } else {
                    "runtime"
                });
            }
            if index == 6 {
                thread.status = ThreadStatus::Interrupted;
            }
            if count == 20 && (index == 10 || index == 11) {
                thread.status = ThreadStatus::Running;
            }
            thread.pending_approval = Some(thread.status == ThreadStatus::AwaitingApproval);
            if index == 3 {
                plait
                    .preferences
                    .value
                    .navigation
                    .unread
                    .insert(thread.id.clone());
            }
            let target = ThreadTarget::Existing {
                id: thread.id.clone(),
                checkout: CheckoutKey {
                    host: plait.tabs.host.clone(),
                    root: thread.checkout.clone(),
                    directory: None,
                },
            };
            plait
                .tabs
                .model
                .open(target, Some(plait.host.connection.epoch))
                .unwrap();
            plait.host.connection.threads.push(thread);
        }
        if let Some(first) = plait.tabs.model.tabs().first() {
            let id = first.id();
            plait.tabs.model.activate(id).unwrap();
        }
        plait.refresh_tab_rows();
    });
}

pub(super) fn walk(
    repository: &Path,
    output: Option<&Path>,
    suffix: &str,
    width: f32,
    height: f32,
) {
    let mut h = Harness::new(repository, output, width, height);
    h.connect(repository, false);
    for (name, count, light) in [
        ("one", 1, true),
        ("three", 3, true),
        ("states", 8, true),
        ("eight", 8, false),
        ("twenty", 20, false),
        ("overflow-open", 20, true),
    ] {
        install_projects(&mut h, count, true);
        h.update(move |p, _, _| {
            p.tabs.list_open = name == "overflow-open";
            if name == "twenty" {
                let id = p.tabs.model.tabs()[12].id();
                p.tabs.model.activate(id).unwrap();
                p.refresh_tab_rows();
            }
        });
        h.capture(&format!("tabs-{name}-{suffix}"));
        if light {
            h.update(|p, _, cx| {
                p.themes.forced = Some(true);
                p.apply_theme(cx);
            });
            h.capture(&format!("light-tabs-{name}-{suffix}"));
            h.update(|p, _, cx| {
                p.themes.forced = Some(false);
                p.apply_theme(cx);
            });
        }
    }
    install(&mut h, 3);
    h.update(|p, _, _| {
        p.tabs.list_open = false;
        p.sidebar = false;
    });
    h.capture(&format!("tabs-no-sidebar-{suffix}"));
    h.update(|p, _, _| {
        p.tabs = Default::default();
        p.host.select(None);
        p.draft.open = false;
        p.sidebar = true;
    });
    h.capture(&format!("tabs-empty-{suffix}"));
    h.connect(repository, false);
    install(&mut h, 3);
    for (name, view) in [
        ("conversation", super::super::Workspace::Conversation),
        ("changes", super::super::Workspace::Changes),
        ("history", super::super::Workspace::History),
        ("files", super::super::Workspace::Files),
    ] {
        h.update(|p, w, cx| {
            p.sidebar = true;
            p.show_tab(view, w, cx);
            if name == "files" {
                p.open_file("src/review.rs".into(), Some(2), cx);
            }
        });
        h.capture(&format!("tabs-all-views-restored-{name}-{suffix}"));
    }
    h.update(|p, _, _| {
        p.workspace.tab = super::super::Workspace::Conversation;
        p.workspace.history = Default::default();
        p.workspace.formatted = Default::default();
        p.host.thread.snapshot = None;
        p.refresh_live_list(None);
    });
    h.capture(&format!("tabs-cold-loading-{suffix}"));
    h.update(|p, _, _| {
        p.notice = Some(
            "The saved view is unavailable. Showing this thread from its latest messages.".into(),
        )
    });
    h.capture(&format!("tabs-bookmark-evicted-{suffix}"));
}

pub(super) fn exercise() {
    let scratch = std::env::temp_dir().join(format!("plait-tab-actions-{}", std::process::id()));
    let repository = fixture_repository(&scratch);
    let mut h = Harness::new(&repository, None, 900., 600.);
    h.connect(&repository, false);
    install(&mut h, 3);
    // Use offline data so selection does not start a real host watch.
    h.update(|p, _, _| {
        p.host.connection.connected = false;
    });
    h.draw();
    h.keystroke("cmd-2");
    h.update(|p, _, _| {
        assert_eq!(p.tabs.model.active(), Some(p.tabs.model.tabs()[1].id()));
    });
    h.keystroke("cmd-shift-]");
    h.update(|p, _, _| {
        assert_eq!(p.tabs.model.active(), Some(p.tabs.model.tabs()[2].id()));
    });
    h.keystroke("ctrl-shift-left");
    h.update(|p, _, _| {
        assert_eq!(p.tabs.model.active(), Some(p.tabs.model.tabs()[1].id()));
    });
    h.keystroke("cmd-w");
    h.update(|p, _, _| {
        assert_eq!(p.tabs.model.tabs().len(), 2);
        assert_eq!(p.tabs.model.closed_len(), 1);
    });
    h.keystroke("cmd-shift-t");
    h.update(|p, _, _| {
        assert_eq!(p.tabs.model.tabs().len(), 3);
        assert_eq!(p.tabs.model.active(), Some(p.tabs.model.tabs()[1].id()));
    });
    h.keystroke("cmd-9");
    h.update(|p, _, _| {
        assert_eq!(p.tabs.model.active(), Some(p.tabs.model.tabs()[2].id()));
    });
    h.keystroke("cmd-w");
    h.keystroke("cmd-w");
    h.keystroke("cmd-w");
    h.update(|p, _, _| {
        assert!(p.tabs.model.tabs().is_empty());
        assert!(p.host.thread.selected.is_none());
    });
    let _ = std::fs::remove_dir_all(scratch);
}

pub(super) fn cold_workspaces() {
    let scratch = std::env::temp_dir().join(format!("plait-cold-tabs-{}", std::process::id()));
    let repository = fixture_repository(&scratch);
    let mut h = Harness::new(&repository, None, 900., 600.);
    h.connect(&repository, false);
    install(&mut h, 3);
    h.update(|p, _, cx| {
        p.host.connection.connected = false;
        p.workspace.composers[THREAD].update(cx, |c, cx| {
            c.restore_draft("Draft survives parking".into(), 9, None, cx)
        });
    });
    for view in [
        super::super::Workspace::Conversation,
        super::super::Workspace::Changes,
        super::super::Workspace::History,
        super::super::Workspace::Files,
    ] {
        let old = h
            .window
            .update(&mut h.cx, |p, _, cx| {
                p.workspace.tab = view;
                p.workspace.thread_files.collapsed = true;
                p.workspace.thread_controls.usage_open = true;
                p.workspace
                    .commit_message
                    .update(cx, |f, cx| f.set_text("Preserve this commit message", cx));
                p.restore_find_bookmark(
                    super::super::file_view::FindIn::Conversation,
                    &plait_core::tabs::bookmark::FindBookmark {
                        open: true,
                        query: "queue".into(),
                        current: None,
                    },
                    cx,
                );
                (
                    p.host.read_stamp(),
                    p.tabs
                        .model
                        .read_stamp(p.tabs.model.active().unwrap())
                        .unwrap(),
                )
            })
            .unwrap();
        h.draw();
        h.keystroke("cmd-2");
        h.update(|p, _, _| {
            assert!(!p.host.accepts_read(&old.0));
            assert!(!p.tabs.model.accepts_read(old.1));
            assert_eq!(p.workspace.composers.len(), 1);
            assert!(p.workspace.formatted.messages.is_empty());
            assert!(p.workspace.history.cache.entries.is_empty());
            assert!(p.workspace.host_watch.is_none());
        });
        h.draw();
        h.keystroke("cmd-1");
        h.update(|p, _, cx| {
            assert_eq!(p.workspace.tab, view);
            assert_eq!(
                p.workspace.composers[THREAD].read(cx).saved_draft().0,
                "Draft survives parking"
            );
            assert!(p.workspace.thread_files.collapsed && p.workspace.thread_controls.usage_open);
            assert_eq!(
                p.workspace.commit_message.read(cx).text(),
                "Preserve this commit message"
            );
            assert_eq!(
                p.workspace
                    .conversation_find
                    .field
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .text(),
                "queue"
            );
            assert!(!p.host.accepts_read(&old.0));
        });
    }
    h.draw();
    h.keystroke("cmd-w");
    h.draw();
    h.keystroke("cmd-shift-t");
    h.update(|p, _, cx| {
        assert_eq!(
            p.workspace.composers[THREAD].read(cx).saved_draft().0,
            "Draft survives parking"
        )
    });
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
}
