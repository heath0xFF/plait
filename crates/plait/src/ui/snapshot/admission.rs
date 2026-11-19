//! A scripted host exercises choice, retry and late acknowledgement ownership.
use super::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[core::prelude::v1::test]
fn choice_flow() {
    if !isolated("admission::choice_flow") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-admission-ui-{}", std::process::id()));
    let repository = fixture_repository(&scratch);
    let root = repository.canonicalize().unwrap();
    let store = PathBuf::from(std::env::var_os("PLAIT_HOST_STORE").unwrap());
    std::fs::create_dir_all(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = store.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = requests.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let main = root.clone();
    let server = std::thread::spawn(move || {
        let parent = plait_core::git::worktree::worktree_parent(&main).unwrap();
        let mut receipt = Value::Null;
        let mut snapshot = Value::Null;
        let mut lost = false;
        while !done.load(Ordering::Relaxed) {
            let (mut stream, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            seen.lock().unwrap().push(request.clone());
            let result = match request["op"].as_str().unwrap() {
                "admission_info" => {
                    json!({"schema":1,"worktree_at_start_v1":true,"linked_continuation_v1":true,"queue":false,"strict_reader":false,"checkout":main,"identity":parent.identity,"project":main,"project_identity":parent.main_identity,"busy":true,"auto_worktree":false,"parent":parent,"refusal":null})
                }
                "create_admitted_thread" if request["placement"] == "project" => {
                    json!({"schema":1,"admission":{"request_id":request["request_id"],"state":"choice","thread_id":null,"checkout":main,"worktree":null,"source_thread_id":null,"error":null}})
                }
                "create_admitted_thread" => {
                    if receipt.is_null() {
                        let id = "12345678-1234-4abc-8abc-123456789abc";
                        let tree = plait_core::git::worktree::create_managed_worktree(
                            &parent,
                            id,
                            None,
                            |_| Ok(()),
                        )
                        .unwrap();
                        snapshot = json!({"id":id,"checkout":tree.path,"model":{"provider":request["provider"],"model_id":request["model_id"]},"status":"idle","messages":[],"live_text":"","tools":[],"pending_approval":null,"error":null,"truncated":false,"harness":{"id":request["harness"],"mode":request["mode"],"version":null,"reported":null}});
                        receipt = json!({"schema":1,"admission":{"request_id":request["request_id"],"state":"ready","thread_id":id,"checkout":tree.path,"worktree":tree,"source_thread_id":null,"error":null}});
                    }
                    if !lost {
                        lost = true;
                        continue;
                    }
                    receipt.clone()
                }
                "watch" => json!({"thread":snapshot}),
                "list_worktrees" => json!({"schema":1,"operations":[]}),
                "submit" => {
                    json!({"thread_id":request["thread_id"],"accepted":true,"request_id":request["request_id"]})
                }
                _ => Value::Null,
            };
            let reply = if result.is_null() {
                json!({"ok":false,"error":{"code":"invalid_request","message":"not supported"}})
            } else {
                json!({"ok":true,"result":result})
            };
            let _ = writeln!(stream, "{reply}");
        }
    });
    let mut harness = Harness::new(&repository, None, 900., 600.);
    harness.connect(&repository, false);
    harness.update(|p, w, cx| {
        p.host.connection.status.as_mut().unwrap().protocol_version = 6;
        p.inject_harnesses(Some(claude_harnesses("ready", None)));
        p.preferences.value.default_harness = HarnessId::Claude;
        p.open_draft(w, cx);
        let composer = p.draft.composer.clone().unwrap();
        composer.update(cx, |composer, cx| {
            composer.restore_draft("Keep my first message".into(), 3, None, cx)
        });
        p.send_draft("Keep my first message".into(), 3, w, cx);
    });
    wait(&mut harness, |p| p.admission.choice);
    harness.update(|p, w, cx| {
        assert!(p.isolation_choice(cx).is_some());
        assert!(p.draft.sending.is_some());
        assert!(p.creation_bar(cx).is_none());
        p.draft.open = false;
        assert!(p.creation_bar(cx).is_some());
        p.draft.open = true;
        p.accept_isolation(w, cx);
    });
    wait(&mut harness, |p| !p.host.creation.creating);
    harness.update(|p, w, cx| {
        assert!(p.admission.pending.is_some());
        assert!(p.host.creation.pending_create.is_some());
        p.retry_draft(w, cx);
        // The person moves away while the retried acknowledgement is in flight.
        p.draft.open = false;
        p.host.thread.watch_epoch += 1;
        let target = plait_core::tabs::ThreadTarget::Existing {
            id: THREAD.into(),
            checkout: p.tab_checkout(root.clone()),
        };
        p.tabs.model.open(target, None).unwrap();
    });
    wait(&mut harness, |p| p.host.creation.pending_create.is_none());
    harness.update(|p,_,_|{
        assert!(!p.draft_visible());
        let created=p.host.connection.threads.iter().find(|t|t.id=="12345678-1234-4abc-8abc-123456789abc").unwrap();
        assert_eq!(created.harness,Some(HarnessId::Claude));assert_ne!(created.checkout,root);
        assert_ne!(p.host.thread.selected.as_deref(),Some(created.id.as_str()));
        let tab=p.tabs.model.tabs().iter().find(|tab|matches!(tab.thread().target(),plait_core::tabs::ThreadTarget::Existing{id,..} if id==&created.id)).unwrap();
        assert_eq!(tab.thread().target().checkout().root,created.checkout);

        assert!(p.notice.as_deref().unwrap().contains("uncommitted files"));
        assert!(p.workspace.composers.contains_key(&created.id));
    });
    let seen = requests.lock().unwrap();
    let creates: Vec<_> = seen
        .iter()
        .filter(|r| r["op"] == "create_admitted_thread" && r["placement"] == "worktree")
        .collect();
    assert_eq!(creates.len(), 2);
    assert_eq!(creates[0], creates[1]);
    assert_eq!(creates[0]["harness"], "claude");
    drop(seen);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    std::fs::remove_dir_all(scratch).unwrap();
}
fn wait(h: &mut Harness, condition: impl Fn(&Plait) -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        h.settle();
        if h.window.update(&mut h.cx, |p, _, _| condition(p)).unwrap() {
            return;
        }
        assert!(Instant::now() < end, "admission did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}
