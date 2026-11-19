//! Branch graph fixtures are confined to scenes and tests.
use super::*;
use plait_core::{
    git::{
        log::{Commit, GraphCommit},
        worktree::inventory::Preservation,
    },
    host::worktree::Checkout,
    tabs::TabTarget,
    worktree_graph,
    worktree_size::Size,
};
fn oid(n: usize) -> String {
    format!("{n:04x}").repeat(10)
}
fn commit(n: usize, parent: usize, subject: String) -> GraphCommit {
    GraphCommit {
        commit: Commit {
            id: oid(n),
            short: oid(n)[..7].into(),
            author: "Sample author".into(),
            time: super::super::commits::now() - n as i64 * 60,
            subject,
        },
        parents: if parent == 0 {
            vec![]
        } else {
            vec![oid(parent)]
        },
    }
}
pub(super) fn install(h: &mut Harness, count: usize) {
    h.update(|p,_,cx|{
        p.park_workspace(cx);
        p.notice=None;
        p.settings_open=false;
        let root=p.host.creation.checkout.clone();
        let view = p.tabs.model.open(TabTarget::Worktrees {checkout:p.tab_checkout(root.clone())},Some(p.host.connection.epoch)).unwrap();
        p.tabs.model.open(TabTarget::Draft { id:"scene-draft".into(), checkout:p.tab_checkout(root.clone()) },Some(p.host.connection.epoch)).unwrap();
        p.tabs.model.activate(view).unwrap();
        p.host.connection.threads.retain(|t| !t.id.starts_with("worktree-scene-"));
        let mut rows=Vec::new();let mut commits=Vec::new();
        for i in 0..count {
            let path=if i==0 {root.clone()}else{root.with_extension("worktrees").join(format!("sample-{i}"))};
            let title=match i {0=>"Main checkout".into(),1=>"Retry requests after rate limits".into(),2=>"Read the local project settings".into(),3=>"Keep attachments across restarts".into(),_=>format!("Worktree {i}")};
            let id=format!("worktree-scene-{i}");
            if i > 0 { p.host.connection.threads.push(ThreadSummary{id:id.clone(),checkout:path.clone(),title:Some(title),status:if i==1{ThreadStatus::Running}else if i==2{ThreadStatus::AwaitingApproval}else{ThreadStatus::Idle},..Default::default()}); }
            rows.push(Checkout {path,main:i==0,branch:Some(if i==0{"main".into()}else{format!("plait/topic-{i}")}),head:Some(oid(if i==0{10}else{100+i})),base:(i>0).then(||oid(9-i.min(6))),thread_id:(i>0).then_some(id),identity:Some(plait_core::git::worktree::Identity{dev:"1".into(),ino:(i+1).to_string()}),missing:false,removed:false,preservation:Preservation{changed:Some(if i==2{3}else{0}),unpushed:Some(if i==1{2}else{0}),exclusive:Some(0),refusal:if i==1{Some("This worktree has unpushed commits. Push them or explicitly confirm preserving only the local branch.".into())}else if i==2{Some("This worktree has uncommitted or ignored files. Preserve or remove them before removing the worktree.".into())}else if i==0{Some("The main checkout cannot be removed here.".into())}else{None}},creation_id:(i>0).then(||format!("creation-{i}")),last_used:Some(super::super::commits::now() as u64*1000-i as u64*300000)});
            if i>0 {commits.push(commit(100+i,9-i.min(6),(if i==1{"Retry transient failures"}else if i==2{"Validate project overrides"}else{"Preserve draft attachments"}).to_string()));}
        }
        for n in (1..=10).rev(){commits.push(commit(n,n-1,match n {10=>"Keep thread tabs independent".into(),9=>"Read host-owned worktrees".into(),8=>"Preserve the checkout identity".into(),_=>format!("Project change {n}")}));}
        p.worktrees.graph=worktree_graph::layout(&rows,commits,false);
        p.worktrees.sizes=(0..count).map(|i|Some(Ok(Size{source:((i+1)*12_000_000) as u64,build:if i==1{3_800_000_000}else if i==3{1_400_000_000}else{0},incomplete:false,entries:300}))).collect();
        p.worktrees.scroll=UniformListScrollHandle::new();
        p.worktrees.rows=rows;p.worktrees.loading=false;p.worktrees.error=None;p.worktrees.selected=1.min(count.saturating_sub(1));p.worktrees.prepare();p.worktree_labels();p.refresh_tab_rows();cx.notify();
    });
}
pub(super) fn walk(repo: &Path, out: Option<&Path>, suffix: &str, width: f32, height: f32) {
    let mut h = Harness::new(repo, out, width, height);
    h.connect(repo, false);
    for light in [false, true] {
        h.update(|p, _, cx| {
            p.themes.forced = Some(light);
            p.apply_theme(cx);
        });
        let theme = if light { "light" } else { "dark" };
        install(&mut h, 4);
        h.capture(&format!("worktrees-graph-{theme}-{suffix}"));
        h.pointer(PlatformInput::MouseMove(MouseMoveEvent {
            position: point(
                px(if width > 1000. { 1290. } else { 480. }),
                px(if width > 1000. { 143. } else { 416. }),
            ),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }));
        h.capture(&format!("worktrees-refusal-hover-{theme}-{suffix}"));
        h.pointer(PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(10.), px(10.)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }));
        h.update(|p, _, _| {
            p.worktrees.sizes.iter_mut().for_each(|s| *s = None);
            p.worktrees.account();
        });
        h.capture(&format!("worktrees-measuring-{theme}-{suffix}"));
        install(&mut h, 21);
        h.capture(&format!("worktrees-twenty-{theme}-{suffix}"));
        h.update(|p, _, _| {
            p.worktrees
                .scroll
                .scroll_to_item(p.worktrees.items.len() - 1, ScrollStrategy::Bottom)
        });
        h.capture(&format!("worktrees-twenty-forks-{theme}-{suffix}"));
        install(&mut h, 4);
        h.update(|p, _, _| {
            p.worktrees.rows[3].head = Some(oid(8));
            p.worktrees.rows[1].base = Some(oid(999));
            let commits = p
                .worktrees
                .graph
                .rows
                .iter()
                .map(|r| r.commit.clone())
                .collect();
            p.worktrees.graph = worktree_graph::layout(&p.worktrees.rows, commits, false);
            p.worktrees.prepare();
            p.worktree_labels();
        });
        h.capture(&format!("worktrees-merged-detached-{theme}-{suffix}"));
        install(&mut h, 4);
        h.update(|p,_,_| {
            p.worktrees.rows[1].missing=true;
            p.worktrees.rows[1].preservation.refusal=Some("The worktree folder is missing. Nothing was removed. If you moved it, move it back; if its disk is not connected, connect it.".into());
            p.worktrees.sizes[1]=Some(Err("The checkout folder is missing.".into()));
            p.worktrees.rows[2].missing=true;p.worktrees.rows[2].removed=true;
            p.worktrees.rows[2].preservation.refusal=Some("This worktree was removed. Its branch, thread history and private build cache remain.".into());
            p.worktrees.rows[3].creation_id=None;p.worktrees.rows[3].thread_id=None;
            p.worktrees.rows[3].preservation.refusal=Some("Plait did not create this worktree. Remove it with your Git tools.".into());
            p.worktrees.prepare();p.worktree_labels();
        });
        h.capture(&format!("worktrees-missing-foreign-{theme}-{suffix}"));
        install(&mut h, 1);
        h.capture(&format!("worktrees-empty-{theme}-{suffix}"));
        h.update(|p, _, _| {
            p.worktrees.park();
            p.worktrees.error = Some(plait_core::host::worktree::OLDER_HOST.into());
        });
        h.capture(&format!("worktrees-old-host-{theme}-{suffix}"));
        h.update(|p, _, _| {
            p.worktrees.error =
                Some("Offline · Connect the host to see this project's worktrees.".into())
        });
        h.capture(&format!("worktrees-offline-{theme}-{suffix}"));
    }
}
#[core::prelude::v1::test]
#[ignore = "render worktree scenes without a window"]
fn scenes() {
    if !isolated("worktrees_view::scenes") {
        return;
    }
    let scratch = PathBuf::from("/tmp/plait-worktree-scenes");
    let _ = std::fs::remove_dir_all(&scratch);
    let repo = fixture_repository(&scratch);
    let out = std::env::var_os("PLAIT_SNAPSHOT_DIR").map(PathBuf::from);
    for (suffix, w, h) in [("wide", 1380., 860.), ("narrow", 900., 600.)] {
        walk(&repo, out.as_deref(), suffix, w, h);
    }
}
#[core::prelude::v1::test]
fn graph_accessibility_is_unique_stable_and_has_no_thread_view_bar() {
    if !isolated("worktrees_view::graph_accessibility_is_unique_stable_and_has_no_thread_view_bar")
    {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-worktree-a11y-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let mut h = Harness::new(&repo, None, 1380., 860.);
    h.connect(&repo, false);
    install(&mut h, 4);
    for i in 0..2 {
        let frame = h.draw_a11y();
        frame.assert_no_duplicates(&format!("worktrees frame {i}"));
        assert!(frame.has_node(Role::Button, |node| node.label.as_deref()
            == Some("Refresh")));
        assert!(!frame.has_node(Role::Tab, |node| node.label.as_deref()
            == Some("Conversation")));
    }
    h.update(|p, _, cx| {
        let owner = p.worktrees.rows[1].thread_id.clone().unwrap();
        p.host
            .connection
            .threads
            .iter_mut()
            .find(|t| t.id == owner)
            .unwrap()
            .title = None;
        p.worktree_labels();
        assert_eq!(p.worktrees.titles[1], "Untitled thread");
        assert!(p.worktrees.openable[1]);
        p.host.connection.threads.retain(|t| t.id != owner);
        p.worktree_labels();
        assert_eq!(p.worktrees.titles[1], "Thread unavailable");
        assert!(!p.worktrees.openable[1]);
        let generation = p.worktrees.generation;
        let stamp = p
            .tabs
            .model
            .read_stamp(p.tabs.model.active().unwrap())
            .unwrap();
        p.tabs.model.next_tab().unwrap();
        p.park_workspace(cx);
        assert!(!p.worktrees_visible());
        assert!(!p.tabs.model.accepts_read(stamp));
        assert!(!p.accepts_worktrees(stamp, generation));
        p.tabs.model.close(stamp.lifetime.tab, false).unwrap();
        assert!(!p.accepts_worktrees(stamp, generation));
        p.refresh_worktrees_view(true, cx);
        assert_eq!(p.worktrees.generation, generation + 1);
        assert!(p.worktrees.items.is_empty());
    });
}
/// Handlers belong to the last drawn frame. A size reply that re-sorts the
/// lanes before the next frame must not turn Remove into another worktree's.
#[core::prelude::v1::test]
fn remove_asks_for_the_worktree_that_was_drawn() {
    if !isolated("worktrees_view::remove_asks_for_the_worktree_that_was_drawn") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-worktree-drawn-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let mut h = Harness::new(&repo, None, 1380., 860.);
    h.connect(&repo, false);
    install(&mut h, 4);
    h.update(|p, window, cx| {
        let row = &mut p.worktrees.rows[1];
        row.preservation.refusal = None;
        row.preservation.unpushed = Some(0);
        p.worktrees.selected = 1;
        // What the drawn Remove button holds.
        let drawn = p.worktrees.rows[1].path.clone();
        let identity = p.worktrees.rows[1].identity.clone();
        // A measurement reply re-sorts by size before the next frame.
        p.worktrees.sizes[3] = Some(Ok(Size {
            source: 9_000_000_000,
            build: 0,
            incomplete: false,
            entries: 1,
        }));
        p.sort_worktree_lanes(true, cx);
        assert_ne!(p.worktrees.rows[1].path, drawn);
        p.ask_remove_view_worktree(&drawn, identity.as_ref(), window, cx);
        let asked = p.worktrees.removal.asking.take().map(|a| a.path);
        assert_eq!(
            asked,
            Some(drawn.clone()),
            "Remove asked about another worktree"
        );
        // A row that is no longer the one shown, or a replaced folder, asks nothing.
        let other = (1..4).find(|&i| i != p.worktrees.selected).unwrap();
        let (path, id) = (
            p.worktrees.rows[other].path.clone(),
            p.worktrees.rows[other].identity.clone(),
        );
        p.worktrees.rows[other].preservation.refusal = None;
        p.ask_remove_view_worktree(&path, id.as_ref(), window, cx);
        assert!(p.worktrees.removal.asking.is_none());
        let replaced = plait_core::git::worktree::Identity {
            dev: "1".into(),
            ino: "999".into(),
        };
        p.ask_remove_view_worktree(&drawn, Some(&replaced), window, cx);
        assert!(p.worktrees.removal.asking.is_none());
    });
    drop(h);
    let _ = std::fs::remove_dir_all(scratch);
}
/// A host helper runs to the end even when its result is retired, so
/// refreshes that arrive while one reads wait for it and run once.
#[core::prelude::v1::test]
fn refreshes_during_a_read_run_once_after_it() {
    if !isolated("worktrees_view::refreshes_during_a_read_run_once_after_it") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-worktree-once-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let mut h = Harness::new(&repo, None, 1380., 860.);
    h.connect(&repo, false);
    install(&mut h, 4);
    h.update(|p, _, cx| {
        assert!(p.host.connection.connected && p.worktrees_visible());
        let generation = p.worktrees.generation;
        for force in [false, false, true, false] {
            p.refresh_worktrees_view(force, cx);
        }
        assert_eq!(
            p.worktrees.generation,
            generation + 1,
            "inventories overlapped"
        );
        assert_eq!(p.worktrees.again, Some(true));
    });
    h.update(|p, _, _| {
        assert!(!p.worktrees.reading);
        assert_eq!(p.worktrees.again, None);
    });
    drop(h);
    let _ = std::fs::remove_dir_all(scratch);
}
/// A worktree removed here leaves its thread marked missing in the sidebar
/// and composer, as the sidebar's own Remove does.
#[core::prelude::v1::test]
fn removal_here_marks_the_folder_missing_for_the_sidebar() {
    if !isolated("worktrees_view::removal_here_marks_the_folder_missing_for_the_sidebar") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-worktree-gone-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let mut h = Harness::new(&repo, None, 1380., 860.);
    h.connect(&repo, false);
    install(&mut h, 4);
    h.update(|p, _, cx| {
        let row = p.worktrees.rows[3].clone();
        p.navigation.available.insert(row.path.clone(), true);
        assert!(
            p.worktrees
                .removal
                .ask(&row, p.tabs.host.clone(), "removal".into())
        );
        let attempt = p.worktrees.removal.answer(true).unwrap();
        let removed = plait_core::host::worktree::Operation {
            request_id: "creation-3".into(),
            thread_id: "12345678-1234-4abc-8abc-123456789abc".into(),
            state: plait_core::host::worktree::Stage::Removed,
            worktree: None,
            error: None,
        };
        p.settle_view_removal(&attempt, Ok(removed), cx);
        assert_eq!(p.navigation.available.get(&row.path), Some(&false));
        assert!(p.worktrees.removal.pending.is_none());
        // A refusal changes nothing about the folder.
        let other = p.worktrees.rows[1].path.clone();
        p.navigation.available.insert(other.clone(), true);
        p.worktrees.rows[1].preservation.refusal = None;
        let row = p.worktrees.rows[1].clone();
        assert!(
            p.worktrees
                .removal
                .ask(&row, p.tabs.host.clone(), "refused".into())
        );
        let attempt = p.worktrees.removal.answer(true).unwrap();
        let refused = plait_core::host::worktree::Operation {
            request_id: "creation-1".into(),
            thread_id: "12345678-1234-4abc-8abc-123456789abc".into(),
            state: plait_core::host::worktree::Stage::Ready,
            worktree: None,
            error: Some(plait_core::host::HostError {
                code: "busy".into(),
                message: "New work appeared.".into(),
            }),
        };
        p.settle_view_removal(&attempt, Ok(refused), cx);
        assert_eq!(p.navigation.available.get(&other), Some(&true));
    });
    drop(h);
    let _ = std::fs::remove_dir_all(scratch);
}
/// One window holds one removal state. A removal whose reply was lost in one
/// project stays with that project's Worktrees tab.
#[core::prelude::v1::test]
fn an_unconfirmed_removal_stays_with_its_project() {
    if !isolated("worktrees_view::an_unconfirmed_removal_stays_with_its_project") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-worktree-scope-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let other = scratch.join("other-project");
    std::fs::create_dir_all(&other).unwrap();
    let mut h = Harness::new(&repo, None, 1380., 860.);
    h.connect(&repo, false);
    install(&mut h, 4);
    let shown = |frame: &crate::ui::a11y::A11yFrame| {
        frame
            .nodes
            .iter()
            .any(|n| n.path.ends_with("worktrees-result"))
    };
    let (mut here, mut there) = (None, None);
    h.update(|p, window, cx| {
        p.worktrees.selected = 3;
        let row = p.worktrees.rows[3].clone();
        p.ask_remove_view_worktree(&row.path, row.identity.as_ref(), window, cx);
        let attempt = p.worktrees.removal.answer(true).unwrap();
        assert!(
            p.worktrees
                .removal
                .finish(&attempt, Err("Connection lost.".into()))
        );
        here = p.tabs.model.active();
        let checkout = p.tab_checkout(other.clone());
        let id = p
            .tabs
            .model
            .open(
                TabTarget::Worktrees { checkout },
                Some(p.host.connection.epoch),
            )
            .unwrap();
        p.tabs.model.activate(id).unwrap();
        there = Some(id);
        assert!(p.worktrees_visible() && !p.removal_here());
        // Refresh here reads this project; it does not resend the other's removal.
        p.refresh_worktrees_view(true, cx);
        assert!(!p.worktree_busy);
        assert!(p.worktrees.removal.pending.is_some());
    });
    assert!(!shown(&h.draw_a11y()), "another project's removal is shown");
    h.update(|p, _, _| {
        assert_ne!(here, there);
        p.tabs.model.activate(here.unwrap()).unwrap();
        assert!(p.removal_here());
    });
    assert!(shown(&h.draw_a11y()));
    drop(h);
    let _ = std::fs::remove_dir_all(scratch);
}
