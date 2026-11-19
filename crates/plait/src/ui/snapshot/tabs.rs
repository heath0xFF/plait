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
