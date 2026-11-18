//! Synthetic recorded Changes scenes and socket-backed UI reply guards.
use super::undo::{Host, inject, prepare, record, setup, wait};
use super::*;
use crate::ui::{Workspace, turn_review::TurnReview};
use plait_core::review::recorded::{Record, Target};
use serde_json::{Value, json};

const PATCH: &str = "diff --git a/src/review.rs b/src/review.rs\n--- a/src/review.rs\n+++ b/src/review.rs\n@@ -1,3 +1,4 @@\n pub fn refresh() {\n-    read_on_frame();\n+    queue_status();\n+    notify_when_ready();\n }\n";
fn target(p: &Plait, record: &str, path: Option<&str>) -> Target {
    Target {
        thread: p.host.selected.clone().unwrap(),
        checkout: p.host.checkout.clone(),
        record: record.into(),
        original: "last".into(),
        path: path.map(PathBuf::from),
    }
}
pub(super) fn scene(
    h: &mut Harness,
    repository: &Path,
    kind: &str,
    lines: Option<Arc<plait_core::model::Diff>>,
) {
    prepare(h, repository);
    h.update(|p,_,_| {
        let mut data=record("last", &["last"], "ready");
        data["paths"][0]["added"]=json!(2);
        if kind=="empty" { data["paths"]=json!([]); data["paths_total"]=json!(0); }
        if kind=="undo" { data["id"]=json!("restore"); data["kind"]=json!("undo"); data["undoes"]=json!("last"); data["paths"].as_array_mut().unwrap().truncate(1); data["paths_total"]=json!(1); }
        if kind=="long" { data["paths"][0]["path"]=json!("src/components/review/very_long_module_name_describing_recorded_checkout_changes.rs"); }
        let id=if kind=="undo" {"restore"} else {"last"};
        if kind=="undo" { inject(p,vec![data,record("last", &["last"], "undone")]); } else { inject(p,vec![data]); }
        let target=target(p,id,None);
        let mut saved=Record::prepare(&p.turn_records.data,&target).unwrap();
        saved.describe(&p.turn_records.data,p.host.snapshot.as_deref());
        p.turn_review=TurnReview { selected:saved.selected(&target), target:Some(target), record:Some(Arc::new(saved)), diff:(!matches!(kind,"empty"|"loading"|"unavailable"|"old")).then(||lines.unwrap_or_else(||Arc::new(plait_core::git::recorded_diff(PATCH.as_bytes(),false,Duration::from_millis(18))))), epoch:Some(p.host.epoch), ..Default::default() };
        if kind=="loading" {p.turn_review.loading=true;}
        if kind=="unavailable" {p.turn_review.error=Some(("Unavailable","The saved content is unavailable.".into()));}
        if kind=="old" {p.turn_review.older=true;p.turn_review.error=Some(("Older host","Turn review needs a newer host. Current Git changes are still available.".into()));}
        if kind=="clean" { p.state.repository.complete(plait_core::state::Job::Status {epoch:p.state.repository.epoch(),path:repository.into()}, plait_core::state::Reply::Status(Ok(Arc::new(plait_core::model::Snapshot {root:repository.into(),branch:"main".into(),named:true,tracking:None,changes:vec![],elapsed:Duration::ZERO})))); p.state.repository.selected=None; }
        p.diff_scroll=UniformListScrollHandle::new(); p.file_scroll=UniformListScrollHandle::new();p.tab=Workspace::Changes;
    });
}
pub(super) fn walk(h: &mut Harness, repository: &Path, suffix: &str) {
    for (name, kind) in [
        ("turn-recorded", "ready"),
        ("turn-recorded-clean", "clean"),
        ("turn-recorded-empty", "empty"),
        ("turn-recorded-loading", "loading"),
        ("turn-recorded-unavailable", "unavailable"),
        ("turn-review-old-host", "old"),
        ("turn-recorded-undo", "undo"),
        ("turn-recorded-long-path", "long"),
    ] {
        scene(h, repository, kind, None);
        h.capture(&format!("{name}-{suffix}"));
    }
    scene(h, repository, "ready", None);
    h.update(|p, _, cx| {
        p.themes.forced = Some(true);
        p.apply_theme(cx);
    });
    h.capture(&format!("turn-recorded-light-{suffix}"));
    h.update(|p, _, cx| {
        p.themes.forced = Some(false);
        p.apply_theme(cx);
        p.clear_turn_review();
        p.state.repository.refresh();
        p.pump(cx);
    });
}
fn info() -> Value {
    json!({"schema":1,"checkout":{"host_id":"host","id":"checkout","catalog_revision":1,"review_revision":0},"features":{"record_diff":true,"attribution":false,"stage_turn":false,"since_review":false},"retention_days":30})
}
fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for p in bytes.chunks(3) {
        let a = p[0];
        let b = *p.get(1).unwrap_or(&0);
        let c = *p.get(2).unwrap_or(&0);
        out.push(ALPHABET[(a >> 2) as usize] as char);
        out.push(ALPHABET[((a & 3) << 4 | b >> 4) as usize] as char);
        out.push(if p.len() > 1 {
            ALPHABET[((b & 15) << 2 | c >> 6) as usize] as char
        } else {
            '='
        });
        out.push(if p.len() > 2 {
            ALPHABET[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
fn diff(request: &Value) -> Value {
    json!({"schema":1,"checkout":info()["checkout"],"view_id":"view","path":request["path"],"side":null,"basis":"record","source_record_id":request["record_id"],"before":{"mode":"100644","fingerprint":"before","bytes":50},"after":{"mode":"100644","fingerprint":"after","bytes":60},"status":"ready","reason":null,"patch_bytes":PATCH.len(),"patch":{"token":"patch","offset":0,"total":1,"items":[{"byte_offset":0,"bytes_base64":encode(PATCH.as_bytes())}],"next_offset":null},"hunks":{"token":"hunks","offset":0,"total":0,"items":[],"next_offset":null},"review":{"state":"unreviewed","revision":0,"reviewed_at":null,"expires_at":null,"source_record_id":null,"can_mark":false},"truncated":false,"elapsed_ms":3})
}
fn answer(request: &Value) -> Option<Value> {
    match request["op"].as_str().unwrap() {
        "turn_review_info" => Some(info()),
        "turn_diff" => Some(diff(request)),
        "turn_records" => {
            Some(json!({"records":[record("last", &["last"], "ready")],"before":null}))
        }
        other => panic!("unexpected {other}"),
    }
}
#[core::prelude::v1::test]
fn recorded_identity_survives_undo() {
    if !isolated("turn_review::recorded_identity_survives_undo") {
        return;
    }
    let (scratch, repository, mut h) = setup("review-binding");
    let host = Host::start(answer);
    h.update(|p, w, cx| p.open_turn_review(target(p, "last", Some("README.md")), w, cx));
    wait(&mut h, |p| p.turn_review.diff.is_some());
    h.update(|p, _, cx| {
        let mut leaf = record("restore", &["undo"], "ready");
        leaf["kind"] = json!("undo");
        leaf["undoes"] = json!("last");
        inject(
            p,
            vec![
                leaf,
                record("later", &["later"], "ready"),
                record("last", &["last"], "undone"),
            ],
        );
        p.refresh_turn_review(cx);
    });
    wait(&mut h, |p| !p.turn_review.running);
    h.update(|p, _, _| {
        assert_eq!(p.turn_review.target.as_ref().unwrap().record, "last");
        assert_eq!(p.recorded_file().unwrap().path, PathBuf::from("README.md"));
        assert_eq!(p.turn_review.record.as_ref().unwrap().files.len(), 4);
    });
    assert!(
        host.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["op"] == "turn_diff")
            .all(|r| r["record_id"] == "last" && r["path"] == "README.md")
    );
    drop(host);
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
    let _ = repository;
}
#[core::prelude::v1::test]
fn old_host_and_incomplete_pages() {
    if !isolated("turn_review::old_host_and_incomplete_pages") {
        return;
    }
    let (scratch, _, mut h) = setup("review-errors");
    let host = Host::start(|_| {
        Some(
            json!({"ok":false,"error":{"code":"invalid_request","message":"Unknown operation or field."}}),
        )
    });
    h.update(|p, w, cx| p.open_turn_review(target(p, "last", None), w, cx));
    wait(&mut h, |p| p.turn_review.older);
    h.update(|p, _, _| {
        assert!(p.turn_review.diff.is_none());
        assert_eq!(p.turn_review.error.as_ref().unwrap().0, "Older host");
    });
    drop(host);
    let host = Host::start(|request| {
        if request["op"] == "turn_diff" {
            let mut value = diff(request);
            value["patch"]["total"] = json!(2);
            Some(value)
        } else {
            answer(request)
        }
    });
    h.update(|p, w, cx| {
        p.turn_review.older = false;
        p.open_turn_review(target(p, "last", None), w, cx);
    });
    wait(&mut h, |p| !p.turn_review.running);
    h.update(|p, _, _| {
        assert!(p.turn_review.diff.is_none());
        assert_eq!(p.turn_review.error.as_ref().unwrap().0, "Unavailable");
    });
    drop(host);
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
}
#[core::prelude::v1::test]
fn stale_review_replies() {
    if !isolated("turn_review::stale_review_replies") {
        return;
    }
    let (scratch, repository, mut h) = setup("review-stale");
    for phase in ["turn_review_info", "turn_diff", "turn_review_page"] {
        for change in 0..8 {
            prepare(&mut h, &repository);
            let host = Host::start(move |request| match request["op"].as_str().unwrap() {
                "turn_diff" if phase == "turn_review_page" => {
                    let mut value = diff(request);
                    value["patch"]["total"] = json!(2);
                    value["patch"]["items"][0]["bytes_base64"] =
                        json!(encode(&PATCH.as_bytes()[..50]));
                    value["patch"]["next_offset"] = json!(1);
                    Some(value)
                }
                "turn_review_page" => Some(
                    json!({"schema":1,"token":"patch","offset":1,"total":2,"items":[{"byte_offset":50,"bytes_base64":encode(&PATCH.as_bytes()[50..])}],"next_offset":null}),
                ),
                _ => answer(request),
            });
            h.update(|p, w, cx| {
                p.open_turn_review(target(p, "last", None), w, cx);
                // Each identity member is changed alone. Going offline prevents
                // a replacement read, so only the old response can install data.
                p.tab = Workspace::Conversation;
                p.host.connected = false;
                match change {
                    0 => p.host.selected = Some("other".into()),
                    1 => p.host.checkout.push("other"),
                    2 => p.host.epoch += 1,
                    3 => p.host.watch_epoch += 1,
                    4 => {
                        p.state.repository.refresh();
                        p.pump(cx);
                    }
                    5 => p.turn_review.generation += 1,
                    6 => p.turn_review.current = true,
                    _ => p.turn_review.target.as_mut().unwrap().path = Some("other.rs".into()),
                }
            });
            wait(&mut h, |p| !p.turn_review.running);
            h.update(|p, _, _| {
                assert!(p.turn_review.diff.is_none(), "{phase}, identity {change}");
                assert!(p.turn_review.record.is_none(), "{phase}, identity {change}");
                p.clear_turn_review();
            });
            assert!(
                host.requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|r| r["op"] == phase)
            );
            drop(host);
        }
    }
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[core::prelude::v1::test]
fn scope_keys_and_clean_files() {
    if !isolated("turn_review::scope_keys_and_clean_files") {
        return;
    }
    let (scratch, repository, mut h) = setup("review-keys");
    let host = Host::start(answer);
    h.update(|p, _, _| {
        let mut undo = record("restore", &["undo"], "ready");
        undo["kind"] = json!("undo");
        undo["undoes"] = json!("last");
        undo["paths"].as_array_mut().unwrap().truncate(1);
        undo["paths_total"] = json!(1);
        let mut original = record("last", &["last"], "undone");
        original["paths"][1]["change"] = json!("deleted");
        inject(p, vec![undo, original]);
        let line = p.turn_records.lines.latest.as_ref().unwrap();
        assert_eq!(line.run, "last");
        assert_eq!(line.displayed, "restore");
        assert_eq!(line.files.len(), 1);
    });
    h.update(|p, w, cx| p.open_turn_review(target(p, "restore", None), w, cx));
    wait(&mut h, |p| p.turn_review.diff.is_some());
    h.update(|p, w, cx| {
        assert_eq!(p.turn_review.record.as_ref().unwrap().files.len(), 1);
        assert!(p.focus.is_focused(w));
        p.open_review_scopes(w, cx);
    });
    wait(&mut h, |p| p.overlay.is_some());
    assert_eq!(shown(&mut h), ["All changes", "Restored", "Original"]);
    press(&mut h, "escape");
    h.update(|p, w, _| {
        assert!(p.overlay.is_none());
        assert!(p.focus.is_focused(w));
    });
    h.update(|p, w, cx| p.open_review_scopes(w, cx));
    wait(&mut h, |p| p.overlay.is_some());
    set_query(&mut h, "Original");
    press(&mut h, "enter");
    wait(&mut h, |p| !p.turn_review.running);
    h.update(|p, w, cx| {
        assert_eq!(p.turn_review.target.as_ref().unwrap().record, "last");
        assert_eq!(p.turn_review.record.as_ref().unwrap().files.len(), 4);
        p.select_recorded_file(1, w, cx);
    });
    wait(&mut h, |p| !p.turn_review.running);
    h.update(|p, _, cx| {
        assert_eq!(p.recorded_file().unwrap().path, PathBuf::from("README.md"));
        assert_eq!(p.recorded_file().unwrap().label, "Deleted");
        p.state.repository.snapshot = Load::Ready(Arc::new(plait_core::model::Snapshot {
            root: repository.clone(),
            branch: "main".into(),
            named: true,
            tracking: None,
            changes: vec![],
            elapsed: Duration::ZERO,
        }));
        p.state.repository.selected = None;
        p.review_comparison(true, cx);
        assert!(!p.recorded_current_available());
    });
    wait(&mut h, |p| !p.turn_review.running);
    h.update(|p, _, cx| p.review_comparison(false, cx));
    wait(&mut h, |p| p.turn_review.diff.is_some());
    h.update(|p, _, _| assert_eq!(p.recorded_file().unwrap().path, PathBuf::from("README.md")));
    drop(host);
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[core::prelude::v1::test]
fn empty_and_more_links() {
    if !isolated("turn_review::empty_and_more_links") {
        return;
    }
    let (scratch, _, mut h) = setup("review-links");
    let host = Host::start(answer);
    h.update(|p, _, _| {
        let line = p.turn_records.lines.latest.as_ref().unwrap();
        assert_eq!(line.more_label, "1 more");
        assert_eq!(line.more_detail, "Review all 4 files in this record.");
        assert_eq!(line.displayed, "last");
    });
    // The no-path target used by the step count and N more opens the first file.
    h.update(|p, w, cx| p.open_turn_review(target(p, "last", None), w, cx));
    wait(&mut h, |p| !p.turn_review.running);
    h.update(|p, _, _| assert_eq!(p.turn_review.selected, Some(0)));
    h.update(|p, w, cx| {
        let mut empty = record("last", &["last"], "ready");
        empty["paths"] = json!([]);
        empty["paths_total"] = json!(0);
        inject(p, vec![empty]);
        p.open_turn_review(target(p, "last", None), w, cx);
    });
    wait(&mut h, |p| !p.turn_review.running);
    h.update(|p, _, _| {
        assert!(p.turn_review.record.as_ref().unwrap().files.is_empty());
        assert!(p.turn_review.selected.is_none());
        assert!(p.turn_review.diff.is_none());
    });
    assert_eq!(
        host.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["op"] == "turn_diff")
            .count(),
        1
    );
    drop(host);
    drop(h);
    std::fs::remove_dir_all(scratch).unwrap();
}
