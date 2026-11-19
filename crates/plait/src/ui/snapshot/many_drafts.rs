//! Independent drafts through the actual headless window and recovery boundary.
use super::*;
use plait_core::{
    host::admission::{Placement, Request},
    tabs::TabTarget,
};

pub(super) fn set_pending(p: &mut Plait, request_id: &str) {
    p.draft.pending = Some(crate::ui::draft_tabs::Pending {
        request: Request::CreateAdmittedThread {
            request_id: request_id.into(),
            checkout: p.host.creation.checkout.clone(),
            placement: Placement::Project,
            harness: HarnessId::Pi,
            provider: choice().provider,
            model_id: choice().model_id,
            approval: ApprovalMode::Ask,
            mode: None,
            thinking: None,
            source_thread_id: None,
        },
        creating: true,
        choice: false,
    });
}
pub(super) fn pending_choice(p: &Plait) -> (String, ModelChoice) {
    let Request::CreateAdmittedThread {
        request_id,
        provider,
        model_id,
        ..
    } = &p.draft.pending.as_ref().unwrap().request
    else {
        panic!("creation");
    };
    (
        request_id.clone(),
        ModelChoice {
            provider: provider.clone(),
            model_id: model_id.clone(),
        },
    )
}
pub(super) fn pending_thinking(p: &Plait) -> Option<plait_core::host::ThinkingLevel> {
    let Request::CreateAdmittedThread { thinking, .. } = &p.draft.pending.as_ref().unwrap().request
    else {
        panic!("creation");
    };
    *thinking
}
pub(super) fn finish(
    p: &mut Plait,
    request: &str,
    reply: Result<plait_core::host::ThreadReply, plait_core::host::Error>,
    w: &mut Window,
    cx: &mut Context<Plait>,
) {
    let draft = std::iter::once(&p.draft)
        .chain(p.tabs.lifecycle.drafts.values())
        .find(|d| d.sending.as_ref().is_some_and(|s| s.request_id == request))
        .unwrap_or(&p.draft);
    let created = crate::ui::host::Created {
        draft_id: draft.id.clone(),
        request_id: request.into(),
        choice: choice(),
        harness: HarnessId::Pi,
        selection_epoch: 0,
        checkout: Some(p.host.creation.checkout.clone()),
    };
    p.finish_draft_creation(
        created,
        p.host.connection.epoch,
        reply.map(|r| (None, Some(r))),
        w,
        cx,
    );
}

#[core::prelude::v1::test]
fn every_new_route_opens_independent_drafts_and_three_hundred_stay_cold() {
    if !isolated(
        "many_drafts::every_new_route_opens_independent_drafts_and_three_hundred_stay_cold",
    ) {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-many-routes-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let mut h = Harness::new(&repo, None, 900., 600.);
    h.connect(&repo, false);
    for key in ["cmd-t", "cmd-n"] {
        for _ in 0..3 {
            h.keystroke(key);
        }
    }
    h.update(|p, w, cx| {
        assert_eq!(p.tabs.model.tabs().len(), 7);
        // The plus and sidebar control use open_draft; the project control uses open_draft_in.
        for _ in 0..3 {
            p.open_draft(w, cx);
            p.open_draft_in(repo.clone(), w, cx);
        }
        assert_eq!(p.tabs.model.tabs().len(), 13);
        assert!(p.draft.composer.as_ref().unwrap().read(cx).is_empty());
        p.draft.composer.clone().unwrap().update(cx, |c, cx| {
            c.restore_draft("First draft".into(), 2, None, cx)
        });
        p.choose_draft_model(HarnessId::Pi, other_choice(), cx);
        p.choose_draft_approval(Some(ApprovalMode::Ask), cx);
        set_pending(p, "choice-a");
        p.draft.pending.as_mut().unwrap().choice = true;
        p.draft.pending.as_mut().unwrap().creating = false;
        let first = p.tabs.model.active().unwrap();
        p.open_draft(w, cx);
        assert_eq!(draft_text(p, cx), "");
        assert!(!p.draft_isolation_choice());
        assert_eq!(p.draft.model, None);
        assert_eq!(p.draft.approval, None);
        p.request_tab(crate::ui::tab_lifecycle::Intent::Activate(first), w, cx);
        assert_eq!(draft_text(p, cx), "First draft");
        assert_eq!(p.draft.model, Some(other_choice()));
        assert!(p.draft_isolation_choice());
        while p.tabs.model.tabs().len() < 300 {
            p.open_draft(w, cx);
        }
        assert_eq!(p.tabs.model.tabs().len(), 300);
        assert!(
            p.tabs
                .lifecycle
                .drafts
                .values()
                .all(|d| d.composer.is_none())
        );
        assert_eq!(p.workspace.composers.len(), 0);
    });
    #[cfg(debug_assertions)]
    {
        let a = h.draw_a11y();
        let b = h.draw_a11y();
        a.assert_no_duplicates("300 drafts first");
        b.assert_no_duplicates("300 drafts steady");
        let tab_nodes = |frame: &crate::ui::a11y::A11yFrame| {
            frame
                .nodes
                .iter()
                .filter(|node| node.path.contains("tab-row"))
                .map(|node| (node.path.clone(), node.node_id))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            tab_nodes(&a),
            tab_nodes(&b),
            "steady tab IDs must not change"
        );
        assert!(b.nodes.iter().filter(|node| node.role == Role::Tab).count() < 20);
        assert!(b.has_node(Role::Button, |n| n.label.as_deref()
            == Some("New thread tab")));
        h.update(|p, _, _| p.tabs.list_open = true);
        let a = h.draw_a11y();
        let b = h.draw_a11y();
        a.assert_no_duplicates("300 drafts overflow first");
        b.assert_no_duplicates("300 drafts overflow steady");
        let entries = |frame: &crate::ui::a11y::A11yFrame| {
            frame
                .nodes
                .iter()
                .filter(|node| node.role == Role::MenuItem)
                .map(|node| (node.path.clone(), node.node_id))
                .collect::<Vec<_>>()
        };
        assert_eq!(entries(&a), entries(&b));
        assert!(!entries(&b).is_empty() && entries(&b).len() < 20);
    }
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[core::prelude::v1::test]
fn two_creations_complete_once_in_their_tabs_after_switch_and_close() {
    if !isolated("many_drafts::two_creations_complete_once_in_their_tabs_after_switch_and_close") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-many-sends-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let mut h = Harness::new(&repo, None, 900., 600.);
    h.connect(&repo, false);
    h.update(|p,w,cx| {
        p.host.connection.status.as_mut().unwrap().protocol_version=6;
        p.host.connection.connected=false; // Completions are injected; submissions remain in their real ledger.
        for (request,text) in [("send-a","Message A"),("send-b","Message B")] {
            p.open_draft(w,cx);
            p.draft.composer.clone().unwrap().update(cx,|c,cx| c.restore_draft(text.into(),1,None,cx));
            set_pending(p,request);
            p.draft.sending=Some(crate::ui::new_thread::Sending { generation:p.draft.generation, request_id:request.into(),text:text.into(),images:Vec::new(),revision:1,approval:Some(ApprovalMode::Ask),choice:Some(ApprovalMode::Ask) });
            p.capture_session(cx);
        }
        let b=p.tabs.model.active().unwrap();
        let a=p.tabs.model.tabs()[1].id();
        // B can complete while A is still preparing; closing A must preserve its operation.
        p.request_tab(crate::ui::tab_lifecycle::Intent::Close(a),w,cx);
        let reply=|id:&str| Ok(plait_core::host::ThreadReply { thread: Snapshot { id:id.into(),checkout:repo.clone(),model:choice(),..Default::default() } });
        finish(p,"send-b",reply("created-b"),w,cx);
        assert_eq!(p.tabs.model.active(),Some(b));
        finish(p,"send-a",reply("created-a"),w,cx);
        assert_eq!(p.tabs.model.active(),Some(b));
        for (id,text) in [("created-a","Message A"),("created-b","Message B")] {
            assert_eq!(p.host.submissions.all(id).len(),1);
            assert_eq!(p.host.submissions.all(id)[0].text,text);
            assert_eq!(p.session.data.as_ref().unwrap().threads[id].pending.as_ref().unwrap().text,text);
        }
        finish(p,"send-a",reply("created-a"),w,cx);
        assert_eq!(p.host.submissions.all("created-a").len(),1);
        p.request_tab(crate::ui::tab_lifecycle::Intent::Navigate(crate::ui::tabs::TabNavigation::Reopen),w,cx);
        assert!(matches!(p.tabs.model.get(p.tabs.model.active().unwrap()).unwrap().session().target(),TabTarget::Existing { id,.. } if id=="created-a"));
    });
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
}

pub(super) fn scenes(h: &mut Harness, suffix: &str) {
    h.update(|p, w, cx| {
        p.tabs = Default::default();
        p.park_workspace(cx);
        for n in 0..3 {
            p.open_draft(w, cx);
            p.draft.composer.clone().unwrap().update(cx, |c, cx| {
                c.restore_draft(format!("Draft {} for this project", n + 1), 1, None, cx)
            });
        }
    });
    h.capture(&format!("tabs-many-drafts-{suffix}"));
    h.update(|p, w, cx| {
        while p.tabs.model.tabs().len() < 300 {
            p.open_draft(w, cx);
        }
    });
    h.capture(&format!("tabs-three-hundred-{suffix}"));
    h.update(|p, _, _| p.tabs.list_open = true);
    h.capture(&format!("tabs-three-hundred-list-{suffix}"));
}

#[core::prelude::v1::test]
fn drafts_restart_in_order_with_images_settings_and_evicted_history() {
    if !isolated("many_drafts::drafts_restart_in_order_with_images_settings_and_evicted_history") {
        return;
    }
    use plait_core::{
        session::Journal,
        tabs::{CheckoutKey, HostKey, storage},
    };
    let scratch = std::env::temp_dir().join(format!("plait-many-restart-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let (directory, store) = plait_core::session::locations().unwrap();
    let (journal, data) = Journal::open(&directory, store.clone()).unwrap();
    let host = HostKey {
        id: "test-host".into(),
        store,
    };
    let checkout = CheckoutKey {
        host: host.clone(),
        root: repo.clone(),
        directory: crate::ui::tab_identity::directory(&repo),
    };
    let path = scratch.join("tabs.json");
    let image = plait_core::images::persist(include_bytes!(
        "../../../../../runtime/test/support/images/0.png"
    ))
    .unwrap();
    let mut h = Harness::new(&repo, None, 900., 600.);
    h.connect(&repo, false);
    h.update(|p, w, cx| {
        p.host.connection.connected = false;
        p.tabs = Default::default();
        p.park_workspace(cx);
        p.install_tab_identity(host.clone(), vec![checkout.clone()], cx);
        p.session.journal = Some(journal.clone());
        p.session.data = Some(data);
        p.tabs.identity.catalog_ready = true;
        p.tabs.persistence.requested = true;
        p.tabs.persistence.loaded = true;
        p.tabs.persistence.restored = true;
        p.tabs.persistence.writable = true;
        p.tabs.persistence.path = Some(path.clone());
        p.tabs.persistence.window = Some(w.window_handle());
    });
    for n in 0..3 {
        h.update(|p, w, cx| p.open_draft(w, cx));
        wait_tabs(&mut h);
        h.update(|p, _, cx| {
            p.draft.model = Some(if n == 1 { other_choice() } else { choice() });
            p.draft.harness = Some(if n == 1 {
                HarnessId::Claude
            } else {
                HarnessId::Pi
            });
            p.draft.approval = Some(ApprovalMode::Ask);
            p.draft.mode = Some(format!("mode-{n}"));
            p.draft.composer.clone().unwrap().update(cx, |c, cx| {
                c.restore_draft(format!("Draft {n}"), n + 1, None, cx);
                if n == 1 {
                    c.restore_images(vec![image.clone()], cx);
                }
            });
            if n == 0 || n == 2 {
                let request = if n == 0 {
                    "restart-pending"
                } else {
                    "restart-worktree"
                };
                set_pending(p, request);
                p.draft.pending.as_mut().unwrap().creating = false;
                p.draft.pending.as_mut().unwrap().choice = n == 0;
                if n == 2
                    && let Request::CreateAdmittedThread { placement, .. } =
                        &mut p.draft.pending.as_mut().unwrap().request
                {
                    *placement = Placement::Worktree;
                }
                p.draft.sending = Some(crate::ui::new_thread::Sending {
                    generation: p.draft.generation,
                    request_id: request.into(),
                    text: format!("Draft {n}"),
                    revision: n + 1,
                    images: Vec::new(),
                    approval: Some(ApprovalMode::Ask),
                    choice: Some(ApprovalMode::Ask),
                });
            }
            p.capture_session(cx);
        });
    }
    h.update(|p, w, cx| p.open_draft(w, cx));
    wait_tabs(&mut h);
    h.update(|p, w, cx| {
        p.discard_draft(w, cx);
        p.capture_session(cx);
    });
    let (order, save) = h
        .window
        .update(&mut h.cx, |p, w, cx| {
            let ids: Vec<_> = p.tabs.model.tabs().iter().map(|t| t.id()).collect();
            p.tabs.model.move_tab(ids[2], 0).unwrap();
            let order: Vec<_> = p
                .tabs
                .model
                .tabs()
                .iter()
                .map(|t| t.session().target().clone())
                .collect();
            (order, p.flush_tab_preferences(Some(w), cx).unwrap())
        })
        .unwrap();
    save.write().unwrap();
    h.update(|p, _, _| {
        p.session = Default::default();
        p.tabs = Default::default();
    });
    drop(h);
    drop(journal);
    let deadline = Instant::now() + Duration::from_secs(5);
    let (journal, data) = loop {
        match Journal::open(&directory, host.store.clone()) {
            Ok(value) => break value,
            Err(error) => {
                assert!(
                    Instant::now() < deadline,
                    "journal ownership did not retire: {error}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    let mut h = Harness::new(&repo, None, 900., 600.);
    h.connect(&repo, false);
    h.update(|p, w, cx| {
        p.host.connection.connected = false;
        p.tabs = Default::default();
        p.park_workspace(cx);
        p.session.journal = Some(journal.clone());
        p.session.data = Some(data);
        p.install_tab_identity(host, vec![checkout], cx);
        p.tabs.identity.catalog_ready = true;
        p.tabs.persistence.requested = true;
        p.tabs.persistence.loaded = true;
        p.tabs.persistence.writable = true;
        p.tabs.persistence.bytes = storage::read(&path).unwrap();
        p.try_restore_tabs(w, cx);
        assert_eq!(
            p.tabs
                .model
                .tabs()
                .iter()
                .map(|t| t.session().target().clone())
                .collect::<Vec<_>>(),
            order
        );
    });
    for (position, n) in [2, 0, 1, 3].into_iter().enumerate() {
        h.update(|p, w, cx| {
            let tab = p.tabs.model.tabs()[position].id();
            p.request_tab(crate::ui::tab_lifecycle::Intent::Activate(tab), w, cx);
        });
        wait_tabs(&mut h);
        h.update(|p, _, cx| {
            if n == 3 {
                assert_eq!(draft_text(p, cx), "");
                return;
            }
            assert_eq!(draft_text(p, cx), format!("Draft {n}"));
            assert_eq!(p.draft_isolation_choice(), n == 0);
            if n == 0 || n == 2 {
                assert_eq!(
                    pending_choice(p).0,
                    if n == 0 {
                        "restart-pending"
                    } else {
                        "restart-worktree"
                    }
                );
                assert!(!p.draft_creating());
            }
            assert_eq!(p.draft.mode.as_deref(), Some(format!("mode-{n}").as_str()));
            assert_eq!(p.draft.approval, Some(ApprovalMode::Ask));
            assert_eq!(
                p.draft.harness,
                Some(if n == 1 {
                    HarnessId::Claude
                } else {
                    HarnessId::Pi
                })
            );
            if n == 1 {
                assert_eq!(
                    p.draft.composer.as_ref().unwrap().read(cx).saved_images(),
                    std::slice::from_ref(&image)
                );
            }
        });
    }
    // Twenty later closes evict the first draft's bookmark, but Cmd-Shift-T can still recover its content.
    for _ in 0..23 {
        h.update(|p, w, cx| p.open_draft(w, cx));
        wait_tabs(&mut h);
        h.update(|p, w, cx| {
            p.draft.composer.clone().unwrap().update(cx, |c, cx| {
                c.restore_draft("Kept after history eviction".into(), 1, None, cx)
            });
            let tab = p.tabs.model.active().unwrap();
            p.request_tab(crate::ui::tab_lifecycle::Intent::Close(tab), w, cx);
        });
        wait_tabs(&mut h);
    }
    for _ in 0..23 {
        h.keystroke("cmd-shift-t");
        wait_tabs(&mut h);
    }
    h.update(|p, _, _| assert_eq!(p.tabs.model.tabs().len(), 27));
    drop(h);
    drop(journal);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[core::prelude::v1::test]
fn restore_forgets_empty_drafts_no_tab_returned() {
    if !isolated("many_drafts::restore_forgets_empty_drafts_no_tab_returned") {
        return;
    }
    restore_with_open_empty_draft(false);
}

#[core::prelude::v1::test]
fn restore_keeps_open_drafts_behind_worktrees() {
    if !isolated("many_drafts::restore_keeps_open_drafts_behind_worktrees") {
        return;
    }
    restore_with_open_empty_draft(true);
}

fn restore_with_open_empty_draft(worktrees_front: bool) {
    use plait_core::{
        session::{Draft, DraftSettings, DraftTab, Session},
        tabs::{CheckoutKey, HostKey},
    };
    let scratch = std::env::temp_dir().join(format!("plait-orphan-drafts-{}", std::process::id()));
    let repo = fixture_repository(&scratch);
    let host = HostKey {
        id: "test-host".into(),
        store: "/host".into(),
    };
    let checkout = CheckoutKey {
        host: host.clone(),
        root: repo.clone(),
        directory: crate::ui::tab_identity::directory(&repo),
    };
    let entry = |text: &str| DraftTab {
        legacy: false,
        creation: None,
        checkout: repo.clone(),
        draft: Draft {
            text: text.into(),
            ..Default::default()
        },
        settings: DraftSettings::default(),
    };
    // A crash after the journal save, before the tab file, leaves an empty entry
    // that no tab record names. It is the checkout's legacy owner.
    let mut data = Session::empty("/host".into());
    data.set_draft_tab("orphan".into(), entry(""));
    data.set_draft_tab("open-empty".into(), entry(""));
    data.set_draft_tab("closed-text".into(), entry("Kept text"));
    let mut h = Harness::new(&repo, None, 900., 600.);
    h.connect(&repo, false);
    h.update(|p, w, cx| {
        p.host.connection.connected = false;
        p.tabs = Default::default();
        p.park_workspace(cx);
        p.install_tab_identity(host, vec![checkout.clone()], cx);
        p.session.data = Some(data);
        let draft_tab = p
            .tabs
            .model
            .open(
                TabTarget::Draft {
                    id: "open-empty".into(),
                    checkout: checkout.clone(),
                },
                None,
            )
            .unwrap();
        if worktrees_front {
            p.tabs
                .model
                .open(TabTarget::Worktrees { checkout }, None)
                .unwrap();
        }
        p.tabs.persistence.edited = true;
        p.tabs.identity.catalog_ready = true;
        p.tabs.persistence.requested = true;
        p.tabs.persistence.loaded = true;
        p.try_restore_tabs(w, cx);
        assert!(
            p.tabs
                .model
                .get(draft_tab)
                .unwrap()
                .session()
                .target()
                .is_draft()
        );
        assert_eq!(p.worktrees_visible(), worktrees_front);
        let data = p.session.data.as_ref().unwrap();
        let mut ids: Vec<_> = data.draft_tabs.keys().map(String::as_str).collect();
        ids.sort();
        assert_eq!(ids, ["closed-text", "open-empty"]);
        assert_eq!(
            data.new_draft_for(&repo).unwrap().text,
            "Kept text",
            "an older build is shown the draft with text"
        );
    });
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
}

fn wait_tabs(h: &mut Harness) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        h.settle();
        if h.window
            .update(&mut h.cx, |p, _, _| {
                !p.tabs.lifecycle.busy && !p.tabs.lifecycle.hydrating
            })
            .unwrap()
        {
            return;
        }
        assert!(Instant::now() < deadline, "tab save did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}
