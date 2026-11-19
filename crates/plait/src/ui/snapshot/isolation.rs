//! Checkout write-exclusion scene (`isolation-wait`) and desktop non-blocking render tests.

use super::{Harness, fixture_repository, git, isolated};
use crate::ui::{Plait, Workspace};
use plait_core::{
    git::WriteOp,
    host::ThreadStatus,
    lease::{CHECKOUT_BUSY, LeaseClass, acquire_checkout},
    model::DiffMode,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// Run queued background work until `done` holds.
fn wait(harness: &mut Harness, done: impl Fn(&Plait) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        harness.settle();
        let ready = harness
            .window
            .update(&mut harness.cx, |plait, _, _| done(plait))
            .expect("window update");
        if ready {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "background Git work did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Captures `isolation-wait`: Changes view when a desktop Git mutation is
/// refused because an active run holds the checkout lease. No new control.
pub(super) fn walk(
    repository: &Path,
    output: Option<&Path>,
    suffix: &str,
    width: f32,
    height: f32,
) {
    let mut harness = Harness::new(repository, output, width, height);
    harness.connect(repository, false);
    harness.update(|plait, window, cx| {
        let root = plait
            .state
            .repository
            .snapshot()
            .ready()
            .unwrap()
            .root
            .clone();
        plait.host.connection.threads[0].checkout = root;
        plait.host.connection.threads[0].status = ThreadStatus::Running;
        plait.show_tab(Workspace::Changes, window, cx);
        plait.select_file(1, DiffMode::Unstaged, window, cx);
        plait.state.repository.write_mut().note = Some((CHECKOUT_BUSY.into(), true));
    });
    harness.capture(&format!("isolation-wait-{suffix}"));
    harness.change_snapshot(|snapshot| snapshot.status = ThreadStatus::Stopped);
    harness.update(|plait, _, _| {
        plait.refresh_tab_rows();
        plait.cleanup.key = Some((
            plait.host.connection.epoch,
            plait.host.creation.generation,
            plait.host.creation.checkout.clone(),
        ));
        plait.cleanup.value = Some(plait_core::lease::cleanup::Cleanup {
            process: None,
            token: "sample-cleanup".into(),
            reason: "Codex did not confirm its turn ended and its app-server has not exited."
                .into(),
        });
        plait.state.repository.write_mut().note =
            Some((plait.cleanup.value.as_ref().unwrap().message(), true));
    });
    harness.capture(&format!("isolation-cleanup-uncertain-{suffix}"));

    let mut creation = Harness::new(repository, output, width, height);
    creation.connect(repository, false);
    creation.update(|plait, _, _| {
        plait.worktree_busy = true;
        plait.notice = Some("Creating worktree… You can keep working in another tab.".into());
    });
    creation.capture(&format!("isolation-create-worktree-{suffix}"));
    creation.update(|plait, _, _| {
        plait.worktree_busy = false;
        plait.notice = Some("Worktree creation was interrupted. Existing files and branch were preserved; inspect them before making a new request.".into());
    });
    creation.capture(&format!("isolation-create-failed-{suffix}"));
    creation.update(|plait, _, cx| {
        let path = repository.with_extension("worktrees").join("123456781234");
        plait.preferences.value.navigation.remember_worktree(
            path.clone(),
            repository.into(),
            "plait/123456781234".into(),
        );
        plait
            .host
            .connection
            .threads
            .push(plait_core::host::ThreadSummary {
                id: "isolated-background".into(),
                checkout: path,
                model: plait.host.connection.threads[0].model.clone(),
                title: Some("New worktree thread".into()),
                harness: Some(plait_core::host::harness::HarnessId::Codex),
                ..Default::default()
            });
        plait.rebuild_projects(cx);
        plait.notice =
            Some("Worktree thread created in the background. Open it from the sidebar.".into());
    });
    creation.capture(&format!("isolation-created-in-background-{suffix}"));
    let mut busy = Harness::new(repository, output, width, height);
    busy.connect(repository, false);
    busy.update(|plait, window, cx| {
        plait.open_draft(window, cx);
        plait.admission.choice = true;
        plait.host.creation.pending_create = Some(("choice".into(), super::choice()));
    });
    busy.capture(&format!("isolation-busy-choice-{suffix}"));
    let mut many = Harness::new(repository, output, width, height);
    many.connect(repository, false);
    super::tabs::install(&mut many, 20);
    many.update(|plait, _, cx| {
        for (i, thread) in plait.host.connection.threads.iter_mut().enumerate() {
            thread.checkout = repository
                .with_extension("worktrees")
                .join(format!("run-{i}"));
            thread.status = ThreadStatus::Running;
            thread.pending_approval = Some(false);
            plait.preferences.value.navigation.remember_worktree(
                thread.checkout.clone(),
                repository.into(),
                format!("plait/run-{i}"),
            );
        }
        plait.refresh_tab_rows();
        plait.rebuild_projects(cx);
    });
    many.capture(&format!("isolation-twenty-running-{suffix}"));
    many.update(|plait, _, _| {
        if let Some(snapshot) = plait.host.thread.snapshot.as_mut() {
            std::sync::Arc::make_mut(snapshot).status = ThreadStatus::Running;
        }
        plait.admission.preparing = Some((
            super::THREAD.into(),
            Some(plait_core::host::admission::Phase::ResourceWait),
        ));
    });
    many.capture(&format!("isolation-resource-wait-{suffix}"));
}

/// Holding a host writer lease on the open checkout must never block desktop
/// render, and every desktop Git mutation must refuse truthfully off render.
#[core::prelude::v1::test]
fn host_locking_never_blocks_desktop_render() {
    if !isolated("isolation::host_locking_never_blocks_desktop_render") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-isolation-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let repository = fixture_repository(&scratch);
    git(&repository, &["branch", "other"]);

    // Hold a real host-style writer lease on the checkout before opening the desktop view.
    let writer = acquire_checkout(&repository, None, LeaseClass::Writer).unwrap();
    let mut harness = Harness::new(&repository, None, 1380., 860.);

    // Drawing every view while the writer lease is held must not block.
    for view in [
        Workspace::Conversation,
        Workspace::Changes,
        Workspace::History,
        Workspace::Files,
    ] {
        let start = Instant::now();
        harness.update(|plait, window, cx| plait.show_tab(view, window, cx));
        harness.draw();
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "render blocked while checkout lease was held on {view:?}"
        );
    }

    // Attempt desktop Git writes (commit, stage, branch switch, discard) while the
    // writer lease is held: every mutation runs off render and refuses with CHECKOUT_BUSY.
    harness.update(|plait, window, cx| plait.show_tab(Workspace::Changes, window, cx));
    for op in [
        WriteOp::StageAll,
        WriteOp::Commit {
            message: "refused while writer holds lease".into(),
        },
        WriteOp::SwitchBranch("other".into()),
        WriteOp::Discard {
            path: "src/review.rs".into(),
            untracked: false,
            hunk: None,
        },
    ] {
        harness.update(|plait, _, cx| plait.git_write(op, cx));
        // Rendering while the write job is in flight must stay non-blocking.
        harness.draw();
        wait(&mut harness, |plait| !plait.state.repository.write().busy());
        harness.update(|plait, _, _| {
            let note = plait.state.repository.write().note.clone();
            assert_eq!(note, Some((CHECKOUT_BUSY.into(), true)));
            let snap = plait.state.repository.snapshot().ready().unwrap();
            assert_eq!(snap.branch, "main");
        });
    }

    // Once the writer lease drops, a desktop commit succeeds cleanly.
    drop(writer);
    harness.update(|plait, _, cx| {
        plait.git_write(
            WriteOp::Commit {
                message: "commit after writer released".into(),
            },
            cx,
        );
    });
    wait(&mut harness, |plait| !plait.state.repository.write().busy());
    harness.update(|plait, _, _| {
        let (note, failed) = plait.state.repository.write().note.clone().unwrap();
        assert!(!failed && note.starts_with("Committed "), "{note}");
    });

    drop(harness);
    std::fs::remove_dir_all(scratch).unwrap();
}
