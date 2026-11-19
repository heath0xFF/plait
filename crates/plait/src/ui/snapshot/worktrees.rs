//! Scripted additive host replies exercise the desktop without launching an app.
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
fn host_defaults_and_grouping() {
    if !isolated("worktrees::host_defaults_and_grouping") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-worktree-ui-{}", std::process::id()));
    let repository = fixture_repository(&scratch);
    let main = repository.canonicalize().unwrap();
    let store = PathBuf::from(std::env::var_os("PLAIT_HOST_STORE").unwrap());
    std::fs::create_dir_all(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = store.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let observed = requests.clone();
    let stopping = Arc::new(AtomicBool::new(false));
    let done = stopping.clone();
    let root = main.clone();
    let server = std::thread::spawn(move || {
        let mut snapshot = Value::Null;
        while !done.load(Ordering::Relaxed) {
            let (mut stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut input = String::new();
            BufReader::new(&mut stream).read_line(&mut input).unwrap();
            let request: Value = serde_json::from_str(&input).unwrap();
            let result = match request["op"].as_str().unwrap() {
                "worktree_info" => {
                    json!({"schema":1,"worktree_threads_v1":true,"parent":plait_core::git::worktree::worktree_parent(&root).unwrap()})
                }
                "create_worktree_thread" => {
                    let index = observed.lock().unwrap().len() + 1;
                    let id = format!("{index:08x}-1234-4abc-8abc-123456789abc");
                    let parent = serde_json::from_value(request["parent"].clone()).unwrap();
                    let tree = plait_core::git::worktree::create_managed_worktree(
                        &parent,
                        &id,
                        None,
                        |_| Ok(()),
                    )
                    .unwrap();
                    observed.lock().unwrap().push(request.clone());
                    snapshot = json!({"id":id,"checkout":tree.path,"model":{"provider":request["provider"],"model_id":request["model_id"]},"status":"idle","messages":[],"live_text":"","tools":[],"pending_approval":null,"error":null,"truncated":false,"harness":{"id":request["harness"],"version":null,"mode":request["mode"],"reported":null}});
                    json!({"schema":1,"operation":{"request_id":request["request_id"],"thread_id":id,"state":"ready","worktree":tree,"error":null}})
                }
                "watch" => json!({"thread":snapshot}),
                "list_worktrees" => json!({"schema":1,"operations":[]}),
                _ => Value::Null,
            };
            let reply = if result.is_null() {
                json!({"ok":false,"error":{"code":"invalid_request","message":"Scripted host does not handle this operation."}})
            } else {
                json!({"ok":true,"result":result})
            };
            let _ = writeln!(stream, "{reply}");
        }
    });
    for (index, chosen) in [HarnessId::Codex, HarnessId::Claude]
        .into_iter()
        .enumerate()
    {
        let mut harness = Harness::new(&repository, None, 900., 600.);
        harness.connect(&repository, false);
        harness.update(|plait, window, cx| {
            plait
                .host
                .connection
                .status
                .as_mut()
                .unwrap()
                .protocol_version = 6;
            plait.inject_harnesses(Some(claude_harnesses("ready", None)));
            plait.preferences.value.default_harness = chosen;
            plait.new_worktree_thread(window, cx);
            assert!(plait.worktree_busy);
            assert!(plait.creation_bar(cx).is_some());
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            harness.settle();
            let ready = harness
                .window
                .update(&mut harness.cx, |plait, _, _| !plait.worktree_busy)
                .unwrap();
            if ready {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "host worktree completion timed out"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let request = requests.lock().unwrap()[index].clone();
        assert_eq!(
            request["harness"],
            if chosen == HarnessId::Codex {
                "codex"
            } else {
                "claude"
            }
        );
        assert_eq!(
            request["mode"],
            if chosen == HarnessId::Codex {
                "full_access"
            } else {
                "auto"
            }
        );
        harness.update(|plait, _, _| {
            assert!(plait.worktree_pending.is_none(), "{:?}", plait.notice);
            let summary = plait
                .host
                .connection
                .threads
                .iter()
                .find(|thread| thread.harness == Some(chosen))
                .unwrap();
            let tree = plait
                .preferences
                .value
                .navigation
                .worktree(&summary.checkout)
                .unwrap();
            assert_eq!(tree.main, main);
            assert!(tree.creation_id.is_some());
            assert!(tree.identity.is_some());
            assert!(summary.checkout.join(".git").is_file());
        });
    }
    stopping.store(true, Ordering::Relaxed);
    server.join().unwrap();
    std::fs::remove_dir_all(scratch).unwrap();
}
