//! Linux-only layout snapshots rendered by GPUI's headless renderer.
//!
//! These images check layout, density, and color on a machine without a
//! display. They use Linux fonts and a software renderer, so they are not a
//! substitute for inspecting the native macOS window.
//!
//! ```sh
//! PLAIT_SNAPSHOT_DIR=/tmp/plait-snapshots \
//!   cargo test --locked -p plait snapshot -- --ignored
//! ```

use super::{Plait, composer::Composer, theme::Assets};
use gpui::{prelude::*, *};
use plait_core::{
    host::{
        Approval, ApprovalKind, Message, MessageRole, Model, ModelChoice, Snapshot, Status,
        ThreadStatus, ThreadSummary, Tool, ToolStatus,
    },
    model::DiffMode,
    state::Load,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

const THREAD: &str = "thr_5791c2e4";

fn git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .expect("git is required for snapshots");
    assert!(status.success(), "git {arguments:?} failed");
}

/// A disposable repository with staged, unstaged, untracked, and deleted files.
fn fixture_repository(root: &Path) -> PathBuf {
    let repository = root.join("plait");
    std::fs::create_dir_all(repository.join("src")).unwrap();
    git(&repository, &["init", "-q", "-b", "main"]);
    git(&repository, &["config", "user.name", "Plait Snapshot"]);
    git(
        &repository,
        &["config", "user.email", "snapshot@example.invalid"],
    );
    git(&repository, &["config", "commit.gpgsign", "false"]);
    let review = repository.join("src/review.rs");
    std::fs::write(&review, "pub fn refresh() {\n    read_status();\n}\n").unwrap();
    std::fs::write(repository.join("README.md"), "# Fixture\n").unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-qm", "fixture"]);
    std::fs::write(&review, "pub fn refresh() {\n    queue_status();\n}\n").unwrap();
    git(&repository, &["add", "src/review.rs"]);
    std::fs::write(
        &review,
        "pub fn refresh() {\n    queue_status();\n    notify_when_ready();\n}\n",
    )
    .unwrap();
    std::fs::write(repository.join("notes.md"), "# Read-only review\n").unwrap();
    std::fs::remove_file(repository.join("README.md")).unwrap();
    repository
}

fn choice() -> ModelChoice {
    ModelChoice {
        provider: "openai-codex".into(),
        model_id: "gpt-5.6-luna".into(),
    }
}

fn status() -> Status {
    let model = |provider: &str, model_id: &str, name: &str, authenticated| Model {
        provider: provider.into(),
        model_id: model_id.into(),
        name: name.into(),
        authenticated,
    };
    Status {
        protocol_version: 1,
        default_model: Some(choice()),
        models: vec![
            model("openai-codex", "gpt-5.6-luna", "GPT-5.6 Luna", true),
            model("openai-codex", "gpt-5.6-sol", "GPT-5.6 Sol", true),
            model("openai-codex", "gpt-6-astra", "GPT-6 Astra", true),
            model(
                "openrouter",
                "anthropic/claude-fable-5",
                "Anthropic: Claude Fable 5",
                true,
            ),
            model(
                "openrouter",
                "anthropic/claude-opus-latest",
                "Anthropic: Claude Opus Latest",
                true,
            ),
            model("google", "gemini-3-pro", "Gemini 3 Pro", false),
        ],
    }
}

fn thread_snapshot(checkout: &Path, approval: bool) -> Snapshot {
    let message = |id: &str, role, text: &str| Message {
        id: id.into(),
        role,
        text: text.into(),
    };
    Snapshot {
        id: THREAD.into(),
        checkout: checkout.to_owned(),
        model: choice(),
        status: if approval {
            ThreadStatus::AwaitingApproval
        } else {
            ThreadStatus::Running
        },
        messages: vec![
            message(
                "m1",
                MessageRole::User,
                "Queue the status refresh instead of reading it inline, then run the tests.",
            ),
            message(
                "m2",
                MessageRole::Assistant,
                "I’ll change refresh() to queue the read and notify when it is ready. The edit is limited to src/review.rs; nothing else in the checkout is touched.",
            ),
            message("m3", MessageRole::User, "Go ahead."),
        ],
        live_text: if approval {
            String::new()
        } else {
            "The edit is applied. Running the tests in the sandbox now".into()
        },
        tools: vec![
            Tool {
                id: "t1".into(),
                name: "edit".into(),
                status: ToolStatus::Done,
                input: "src/review.rs".into(),
                output: "Replaced 1 line.".into(),
            },
            Tool {
                id: "t2".into(),
                name: "command".into(),
                status: ToolStatus::Running,
                input: "cargo test".into(),
                output: String::new(),
            },
        ],
        pending_approval: approval.then(|| Approval {
            id: "a1".into(),
            tool_id: "t2".into(),
            kind: ApprovalKind::Command,
            summary: "Run cargo test".into(),
            details: "{\"command\":\"cargo\",\"args\":[\"test\"],\"cwd\":\".\"}".into(),
            dirty_paths: vec!["notes.md".into()],
            risk: "Runs in the command sandbox with a read-only checkout and no network.".into(),
        }),
        error: None,
        truncated: false,
    }
}

struct Harness {
    cx: HeadlessAppContext,
    window: WindowHandle<Plait>,
    /// Where to write PNG files. `None` draws every scene without a renderer.
    output: Option<PathBuf>,
}

impl Harness {
    fn new(repository: &Path, output: Option<&Path>, width: f32, height: f32) -> Self {
        // Without a display server the Linux platform has no text system, so
        // shape with the fonts installed here. Metrics differ from macOS.
        let text_system = Arc::new(gpui_wgpu::CosmicTextSystem::new("DejaVu Sans"));
        let render = output.is_some();
        let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(Assets), move || {
            if render {
                gpui_platform::current_headless_renderer()
            } else {
                Ok(None)
            }
        });
        cx.allow_parking();
        let path = repository.to_owned();
        let window = cx
            .open_window(size(px(width), px(height)), |window, cx| {
                cx.new(|cx| Plait::new(path, Instant::now(), window, cx))
            })
            .expect("headless window");
        let mut harness = Self {
            cx,
            window,
            output: output.map(Path::to_owned),
        };
        harness.settle();
        harness
    }

    /// Run queued work until the Git reads and the host probe have landed.
    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.cx.run_until_parked();
            let busy = self
                .window
                .update(&mut self.cx, |plait, _, _| {
                    let repository = &plait.state.repository;
                    matches!(repository.snapshot, Load::Loading)
                        || matches!(repository.diff, Load::Loading)
                        || plait.host.connecting
                })
                .expect("window update");
            if !busy {
                return;
            }
            assert!(Instant::now() < deadline, "background work did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn update(&mut self, change: impl FnOnce(&mut Plait, &mut Window, &mut Context<Plait>)) {
        self.window
            .update(&mut self.cx, |plait, window, cx| {
                change(plait, window, cx);
                cx.notify();
            })
            .expect("window update");
        self.settle();
    }

    fn change_snapshot(&mut self, change: impl FnOnce(&mut Snapshot)) {
        self.update(|plait, _, _| {
            let previous = plait.host.snapshot.clone().expect("sample snapshot");
            change(Arc::make_mut(plait.host.snapshot.as_mut().unwrap()));
            plait.refresh_live_list(Some(&previous));
        });
    }

    /// Draw the scene, which runs layout and paint, and save it when rendering.
    fn capture(&mut self, name: &str) {
        let window = self.window.into();
        self.cx
            .update_window(window, |_, window, cx| window.draw(cx).clear(cx))
            .expect("draw");
        if let Some(output) = &self.output {
            let image = self.cx.capture_screenshot(window).expect("screenshot");
            image
                .save(output.join(format!("{name}.png")))
                .expect("write snapshot");
        }
    }

    /// Present a connected host without a socket. Labeled sample data only.
    fn connect(&mut self, repository: &Path, approval: bool) {
        let checkout = repository.to_owned();
        self.update(move |plait, window, cx| {
            plait.host.connecting = false;
            plait.host.connected = true;
            plait.host.error = None;
            plait.host.status = Some(status());
            plait.host.settings_model = Some(choice());
            plait.host.new_model = Some(choice());
            plait.host.threads = vec![
                ThreadSummary {
                    id: THREAD.into(),
                    checkout: checkout.clone(),
                    model: choice(),
                    status: ThreadStatus::Running,
                },
                ThreadSummary {
                    id: "thr_0c44a9d1".into(),
                    checkout: checkout.clone(),
                    model: choice(),
                    status: ThreadStatus::Idle,
                },
            ];
            // Mirror select_host_thread without opening a socket.
            plait.host.selected = Some(THREAD.into());
            plait.host.snapshot = Some(Arc::new(thread_snapshot(&checkout, approval)));
            plait.settings_open = false;
            plait.new_thread_open = false;
            plait.show_changes = false;
            let composer = cx.new(|cx| Composer::new(window, cx));
            plait.composers.insert(THREAD.into(), composer);
            plait.refresh_live_list(None);
            plait.sync_composer(cx);
        });
    }
}

/// Every view at the default size and at the minimum window size.
fn walk(output: Option<&Path>) {
    let scratch = std::env::temp_dir().join(format!(
        "plait-snapshot-{}-{}",
        std::process::id(),
        if output.is_some() { "render" } else { "draw" }
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    let repository = fixture_repository(&scratch);

    for (suffix, width, height) in [("wide", 1380., 860.), ("narrow", 900., 600.)] {
        let mut harness = Harness::new(&repository, output, width, height);
        // No socket in tests, so the host starts disconnected.
        harness.capture(&format!("home-disconnected-{suffix}"));
        harness.update(|plait, window, cx| {
            plait.select_file(1, DiffMode::Unstaged, window, cx);
        });
        harness.capture(&format!("changes-{suffix}"));
        harness.update(|plait, _, _| {
            plait.show_changes = false;
            plait.host.connecting = false;
            plait.host.connected = true;
            plait.host.status = Some(status());
        });
        harness.capture(&format!("home-empty-{suffix}"));
        harness.connect(&repository, false);
        harness.capture(&format!("live-{suffix}"));
        harness.update(|plait, _, _| {
            plait.host.connected = false;
            plait.host.error = Some("Local host connection failed: sample error.".into());
        });
        harness.capture(&format!("live-disconnected-{suffix}"));
        harness.connect(&repository, true);
        harness.update(|plait, _, cx| plait.toggle_live_tool("t1", cx));
        harness.capture(&format!("live-approval-{suffix}"));
        streaming_walk(&mut harness, &repository, suffix);
        harness.update(|plait, _, _| plait.settings_open = true);
        harness.capture(&format!("settings-{suffix}"));
        harness.update(|plait, _, cx| plait.open_new_thread(cx));
        harness.capture(&format!("new-thread-{suffix}"));
        harness.update(|plait, _, _| plait.new_thread_open = false);
        harness.capture(&format!("home-threads-{suffix}"));
        // Cmd-2 selects the second thread; next/previous wrap around the list.
        harness.update(|plait, window, cx| {
            plait.jump_to(2, window, cx);
            assert_eq!(plait.host.selected.as_deref(), Some("thr_0c44a9d1"));
            plait.step_thread(true, window, cx);
            assert_eq!(plait.host.selected.as_deref(), Some(THREAD));
            plait.step_thread(false, window, cx);
            assert_eq!(plait.host.selected.as_deref(), Some("thr_0c44a9d1"));
            plait.jump_to(9, window, cx);
            assert_eq!(plait.host.selected.as_deref(), Some("thr_0c44a9d1"));
            plait.sidebar = false;
            plait.diagnostics = true;
        });
        harness.capture(&format!("no-sidebar-{suffix}"));
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

fn streaming_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.change_snapshot(|snapshot| {
        snapshot.messages.extend((3..40).map(|index| Message {
            id: format!("history-{index}"),
            role: MessageRole::Assistant,
            text: format!(
                "Sample history row {index}. This is a rendering fixture, not an agent run."
            ),
        }));
    });
    harness.update(|plait, _, cx| {
        plait.toggle_live_tool("t2", cx);
        plait.live_scroll.scroll_to(ListOffset {
            item_ix: 12,
            offset_in_item: px(7.),
        });
    });
    harness.capture(&format!("stream-history-{suffix}"));
    let anchor = harness
        .window
        .update(&mut harness.cx, |plait, _, _| {
            plait.live_scroll.logical_scroll_top()
        })
        .unwrap();
    for _ in 0..20 {
        harness.update(|plait, _, _| {
            let measured = plait.live_scroll.bounds_for_item(anchor.item_ix);
            assert!(
                measured.is_some(),
                "visible history row must already be measured"
            );
            let previous = plait.host.snapshot.clone().unwrap();
            Arc::make_mut(plait.host.snapshot.as_mut().unwrap())
                .live_text
                .push_str(" another streamed word");
            plait.refresh_live_list(Some(&previous));
            assert_eq!(plait.live_scroll.bounds_for_item(anchor.item_ix), measured);
            let current = plait.live_scroll.logical_scroll_top();
            assert_eq!(current.item_ix, anchor.item_ix);
            assert_eq!(current.offset_in_item, anchor.offset_in_item);
            assert!(plait.expanded_live_tools.contains("t2"));
        });
    }
    harness.change_snapshot(|snapshot| {
        snapshot.messages.remove(0);
        snapshot.messages.push(Message {
            id: "new-user".into(),
            role: MessageRole::User,
            text: "Sample follow-up".into(),
        });
        snapshot.live_text.clear();
        snapshot.tools[1].output = "Sample test output: 24 tests passed.".into();
    });
    harness.update(|plait, _, _| {
        let current = plait.live_scroll.logical_scroll_top();
        assert_eq!(current.item_ix, anchor.item_ix - 1);
        assert_eq!(current.offset_in_item, anchor.offset_in_item);
        assert!(plait.expanded_live_tools.contains("t2"));
        assert!(!plait.expanded_live_tools.contains("t1"));
        plait.live_scroll.scroll_to_end();
    });
    harness.capture(&format!("stream-tool-{suffix}"));
    harness.change_snapshot(|snapshot| {
        snapshot.tools.remove(1);
    });
    harness.update(|plait, _, _| assert!(!plait.expanded_live_tools.contains("t2")));
}

// The glob import above brings in `gpui::test`; these are plain tests.

/// Layout and paint run for every view. GPUI reports some misuse, such as a
/// second hover style on one element, only when the element is drawn.
#[core::prelude::v1::test]
fn every_view_draws() {
    walk(None);
}

#[core::prelude::v1::test]
#[ignore = "writes PNG files; set PLAIT_SNAPSHOT_DIR and pass --ignored"]
fn snapshot() {
    let Some(output) = std::env::var_os("PLAIT_SNAPSHOT_DIR").map(PathBuf::from) else {
        panic!("Set PLAIT_SNAPSHOT_DIR to an output directory.");
    };
    std::fs::create_dir_all(&output).unwrap();
    walk(Some(&output));
}
