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
