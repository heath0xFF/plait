//! Scene snapshots of the root view, rendered by GPUI's headless renderers.
//!
//! No window opens: GPUI's test platform lays out and paints the real root
//! view, and a headless renderer draws the scene into an offscreen texture.
//! On macOS that is Metal with Core Text, the renderer and text system the app
//! uses. On Linux it is wgpu on a software Vulkan driver with Linux fonts, so
//! those images check layout, density, and color only.
//!
//! Neither platform exercises a native window, real input events, or the
//! accessibility tree an assistive client reads.
//!
//! ```sh
//! bash scripts/snapshots.sh /tmp/plait-snapshots
//! ```
//!
//! To add a scene, put its sample state in a small function beside the others,
//! then call `harness.capture("name-{suffix}")` from `walk` or a `*_walk`
//! helper. Every scene runs at both window sizes.

use super::{Plait, composer::Composer, model_picker::ModelPurpose, theme::Assets};
use gpui::{prelude::*, *};
use plait_core::{
    host::{
        Approval, ApprovalKind, ApprovalMode, HostError, Message, MessageRole, Model, ModelChoice,
        Snapshot, Status, ThreadStatus, ThreadSummary, Tool, ToolStatus,
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

/// Set in the child process that draws scenes. Names its scratch home.
const SCENE_HOME: &str = "PLAIT_SCENE_HOME";

/// Whether this process may build the root view.
///
/// `Plait::new` probes the local host socket and reads desktop preferences,
/// both found through `HOME`. On a developer's machine those belong to a
/// running Plait, so the test process itself never draws. It re-runs the named
/// test in a child whose home and host store are empty scratch directories and
/// reports the child's result. Only that child returns `true`.
fn isolated(test: &str) -> bool {
    if let Some(home) = std::env::var_os(SCENE_HOME) {
        assert_eq!(std::env::var_os("HOME"), Some(home), "scene home");
        assert!(std::env::var_os("PLAIT_HOST_STORE").is_some(), "host store");
        return true;
    }
    // Unix sockets have a short pathname limit, including the temporary home.
    let home = PathBuf::from("/tmp").join(format!("plait-scene-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("scratch home");
    // libtest names omit the crate: `ui::snapshot::<test>`.
    let module = module_path!().split_once("::").expect("module path").1;
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", &format!("{module}::{test}")])
        .args(["--include-ignored", "--test-threads=1", "--nocapture"])
        .env(SCENE_HOME, &home)
        .env("HOME", &home)
        .env("PLAIT_HOST_STORE", home.join("host"))
        .env("PLAIT_DESKTOP_STATE_DIR", home.join("desktop"))
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("re-run the test binary");
    let _ = std::fs::remove_dir_all(&home);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // A filter that matched nothing also exits successfully.
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "isolated run of {test} failed:\n{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

/// Real text shaping for the headless window, with the app's own fonts.
#[cfg(target_os = "macos")]
fn text_system() -> Arc<dyn PlatformTextSystem> {
    // Core Text, as in the app. The Mac platform needs the main thread; this does not.
    Arc::new(gpui_apple::AppleTextSystem::new())
}

/// Real text shaping for the headless window, with the fonts installed here.
#[cfg(target_os = "linux")]
fn text_system() -> Arc<dyn PlatformTextSystem> {
    // Without a display server the Linux platform has no text system.
    // Metrics differ from macOS.
    Arc::new(gpui_wgpu::CosmicTextSystem::new("DejaVu Sans"))
}

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
        ..Default::default()
    };
    Status {
        protocol_version: 2,
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
        ]
        .into_iter()
        // Enough catalog for the picker to scroll: providers that are not signed in.
        .chain((0..36).map(|index| {
            model(
                &format!("sample-provider-{}", index / 6 + 1),
                &format!("sample-model-{}", index + 1),
                &format!("Sample Model {}", index + 1),
                false,
            )
        }))
        .collect(),
    }
}

fn other_choice() -> ModelChoice {
    ModelChoice {
        provider: "openrouter".into(),
        model_id: "anthropic/claude-opus-latest".into(),
    }
}

fn thread_snapshot(checkout: &Path, approval: bool) -> Snapshot {
    let message = |id: &str, role, text: &str| Message {
        id: id.into(),
        role,
        text: text.into(),
        ..Default::default()
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
                "## Planned change\n\nI’ll change `refresh()` to **queue the read**, then notify when it is ready.\n\n```rust\npub fn refresh() {\n    queue_status();\n    notify_when_ready();\n}\n```\n\nThe edit is limited to `src/review.rs`.",
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
                input: "{\"path\":\"src/review.rs\",\"old_text\":\"    read_status();\",\"new_text\":\"    queue_status();\\n    notify_when_ready();\"}".into(),
                output: "Replaced 1 line.".into(),
                after: Some(Some("m3".into())),
            },
            Tool {
                id: "t2".into(),
                name: "command".into(),
                status: ToolStatus::Running,
                input: "{\"command\":\"cargo test\"}".into(),
                output: String::new(),
                after: Some(Some("m3".into())),
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
        approval_mode: Some(if approval {
            ApprovalMode::Ask
        } else {
            ApprovalMode::Auto
        }),
        ..Default::default()
    }
}

/// One sample tool call, ordered after the last sample message.
fn tool(id: &str, name: &str, status: ToolStatus, input: &str, output: &str) -> Tool {
    Tool {
        id: id.into(),
        name: name.into(),
        status,
        input: input.into(),
        output: output.into(),
        after: Some(Some("m3".into())),
    }
}

/// Finished tool calls in each end state, on a thread the host stopped with an error.
fn tool_states(snapshot: &mut Snapshot) {
    snapshot.status = ThreadStatus::Error;
    snapshot.live_text.clear();
    snapshot.tools = vec![
        tool(
            "t1",
            "edit",
            ToolStatus::Done,
            "{\"path\":\"src/review.rs\",\"old_text\":\"    read_status();\",\"new_text\":\"    queue_status();\\n    notify_when_ready();\"}",
            "Replaced 1 line.",
        ),
        tool(
            "t2",
            "command",
            ToolStatus::Done,
            "{\"command\":\"cargo test\"}",
            "Sample test output: 24 tests passed.",
        ),
        tool(
            "t3",
            "command",
            ToolStatus::Error,
            "{\"command\":\"cargo clippy -- -D warnings\"}",
            "Sample error output: exit status 101.",
        ),
        tool(
            "t4",
            "edit",
            ToolStatus::Interrupted,
            "{\"path\":\"notes.md\",\"old_text\":\"# Read-only review\",\"new_text\":\"# Review\"}",
            "",
        ),
    ];
    snapshot.error = Some(HostError {
        code: "sample_error".into(),
        message: "Sample host error. This is a rendering fixture, not an agent run.".into(),
    });
    snapshot.truncated = true;
}

/// The sample turn completed: nothing is streaming and no tool is running.
fn finished_turn(snapshot: &mut Snapshot) {
    snapshot.status = ThreadStatus::Idle;
    snapshot.live_text.clear();
    snapshot.tools[1].status = ToolStatus::Done;
    snapshot.tools[1].output = "Sample test output: 24 tests passed.".into();
    snapshot.messages.push(Message {
        id: "m4".into(),
        role: MessageRole::Assistant,
        text: "Sample result. The refresh is queued and the tests pass.\n\n- `src/review.rs` queues the read\n- 24 tests passed".into(),
        ..Default::default()
    });
}

/// Sample replies that exercise every kind of conversation text.
const PROSE: &str = "## Sample reading fixture\n\nThis is a rendering fixture, not an agent run. See [the GPUI site](https://gpui.rs/) or https://example.com/docs for more, with ~~struck~~ text, *emphasis*, **strong**, and `inline code`.\n\n1. First step\n   - nested point that is long enough to wrap onto a second line at the narrow window size so the hanging indent is visible\n     - deeper point\n2. Second step\n\n   A paragraph that belongs to the second step.\n\n- [x] finished task\n- [ ] open task\n\n> A quoted remark.\n\n| Crate | Role | Lines |\n| :-- | :-- | --: |\n| `plait` | GPUI presentation and input | 6200 |\n| `plait-core` | Models, **projection**, and Git | 5400 |\n| `plait-host` | Host lifetime | 1300 |\n\n---\n\nDone.";

const CODE: &str = "```rust\n// Sample code\npub fn refresh<'a>(queue: &'a mut Queue) -> Result<usize, Error> {\n    let count = queue.len() + 0x1f;\n    println!(\"queued {count}\");\n    Ok(count)\n}\n```\n\n```typescript\nexport async function load(path: string): Promise<Thread[]> {\n  const reply = await fetch(`/threads/${path}`); // sample\n  return reply.ok ? reply.json() : [];\n}\n```\n\n```python\ndef total(rows: list[int]) -> int:\n    \"\"\"Sample docstring.\"\"\"\n    return sum(row for row in rows if row > 0)  # positive only\n```\n\n```sh\nCARGO_TERM_COLOR=never cargo test --locked | tee \"$TMPDIR/test.log\" # sample\n```";

const DATA: &str = "```json\n{\"name\": \"plait\", \"private\": true, \"version\": 3}\n```\n\n```toml\n[workspace.package]\nedition = \"2024\" # sample\n```\n\n```yaml\nsteps:\n  - run: \"cargo test\"\n    retries: 2\n```\n\n```diff\n@@ -1,3 +1,3 @@\n-    read_status();\n+    queue_status();\n```\n\n```go\nfunc Sum(xs []int) int { return len(xs) } // sample\n```\n\n```cpp\n#include <vector>\nint main() { std::vector<int> v{1, 2}; return 0; }\n```\n\n```\nplain text with no language\n```\n\n| Sample | One | Two | Three | Four | Five | Six | Seven |\n| - | - | - | - | - | - | - | - |\n| A wide table scrolls sideways | 1 | 2 | 3 | 4 | 5 | 6 | A last cell that is long enough to wrap inside its column when it reaches the width limit |";

/// A finished thread whose replies cover prose, tables, links, and code.
fn reading(snapshot: &mut Snapshot) {
    snapshot.status = ThreadStatus::Idle;
    snapshot.live_text.clear();
    snapshot.tools.clear();
    let message = |id: &str, role, text: &str| Message {
        id: id.into(),
        role,
        text: text.into(),
        ..Default::default()
    };
    snapshot.messages = vec![
        message(
            "r1",
            MessageRole::User,
            "Show the sample fixture.\nIt links to https://example.com/start too.",
        ),
        message("r2", MessageRole::Assistant, PROSE),
        message("r3", MessageRole::User, "And the code samples."),
        message("r4", MessageRole::Assistant, CODE),
        message("r5", MessageRole::Assistant, DATA),
    ];
}

/// One finished call of every tool, a failed command, and arguments the host clipped.
fn tool_cards(snapshot: &mut Snapshot) {
    snapshot.status = ThreadStatus::Idle;
    snapshot.live_text.clear();
    let listing: String = (1..=30)
        .map(|line| format!("pub fn sample_{line}() {{}}\n"))
        .collect();
    snapshot.tools = vec![
        tool(
            "c1",
            "read",
            ToolStatus::Done,
            "{\"path\":\"src/review.rs\"}",
            &listing,
        ),
        tool(
            "c2",
            "edit",
            ToolStatus::Done,
            "{\"path\":\"src/review.rs\",\"old_text\":\"pub fn refresh() {\\n    read_status();\\n}\",\"new_text\":\"pub fn refresh() {\\n    queue_status();\\n    notify_when_ready();\\n}\"}",
            "Replaced 1 occurrence.",
        ),
        tool(
            "c3",
            "write",
            ToolStatus::Done,
            "{\"path\":\"notes/plan.toml\",\"content\":\"# Sample file\\n[plan]\\nsteps = 3\\n\"}",
            "Created notes/plan.toml.",
        ),
    ];
    snapshot.messages.push(Message {
        id: "m4".into(),
        role: MessageRole::Assistant,
        text: "Sample note between two runs of tool calls.".into(),
        ..Default::default()
    });
    let after = |mut call: Tool| {
        call.after = Some(Some("m4".into()));
        call
    };
    snapshot.tools.extend([
        after(tool(
            "c4",
            "command",
            ToolStatus::Done,
            "{\"command\":\"cargo test --locked\"}",
            "Sample output\nrunning 24 tests\ntest result: ok. 24 passed; 0 failed",
        )),
        after(tool(
            "c5",
            "command",
            ToolStatus::Error,
            "{\"command\":\"cargo clippy -- -D warnings\"}",
            "Sample error output\nerror: unused variable: `count`\nexit status 101",
        )),
        after(tool(
            "c6",
            "edit",
            ToolStatus::Done,
            "ample clipped arguments\",\"new_text\":\"the start was cut off\"}",
            "Replaced 1 occurrence.",
        )),
    ]);
}

/// A long run of calls: finished and folded, or still running with its latest steps.
fn long_run(snapshot: &mut Snapshot, running: bool) {
    snapshot.status = if running {
        ThreadStatus::Running
    } else {
        ThreadStatus::Idle
    };
    snapshot.live_text.clear();
    let names = ["read", "read", "edit", "command", "read", "edit", "command"];
    snapshot.tools = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let (input, output) = match *name {
                "command" => ("{\"command\":\"cargo check\"}", "Sample output."),
                "edit" => (
                    "{\"path\":\"src/review.rs\",\"old_text\":\"a\",\"new_text\":\"b\"}",
                    "Replaced 1 occurrence.",
                ),
                _ => ("{\"path\":\"src/review.rs\"}", "pub fn sample() {}\n"),
            };
            let last = index + 1 == names.len();
            let status = match (index, running && last) {
                (_, true) => ToolStatus::Running,
                (3, _) => ToolStatus::Error,
                _ => ToolStatus::Done,
            };
            tool(&format!("g{index}"), name, status, input, output)
        })
        .collect();
    if !running {
        snapshot.messages.push(Message {
            id: "m4".into(),
            role: MessageRole::Assistant,
            text: "Sample result after a long run of tool calls.".into(),
            ..Default::default()
        });
    }
}

/// An unsent message of several lines, as typed into the composer.
const DRAFT: &str =
    "Also notify when the queue is empty.\nKeep the public signature of refresh() as it is.";

struct Harness {
    cx: HeadlessAppContext,
    window: WindowHandle<Plait>,
    /// Where to write PNG files. `None` draws every scene without a renderer.
    output: Option<PathBuf>,
}

impl Harness {
    fn new(repository: &Path, output: Option<&Path>, width: f32, height: f32) -> Self {
        let text_system = text_system();
        let render = output.is_some();
        let mut cx = HeadlessAppContext::with_platform(text_system, Arc::new(Assets), move || {
            if render {
                gpui_platform::current_headless_renderer()
            } else {
                Ok(None)
            }
        });
        cx.allow_parking();
        // As main does, so scenes show the shortcuts the palette reads from the keymap.
        cx.update(super::menus);
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
        self.update(|plait, _, cx| {
            let previous = plait.host.snapshot.clone().expect("sample snapshot");
            change(Arc::make_mut(plait.host.snapshot.as_mut().unwrap()));
            plait.formatted = Arc::new(plait_core::markdown::Transcript::update(
                &plait.formatted,
                plait.host.snapshot.as_ref().unwrap(),
            ));
            plait.refresh_live_list(Some(&previous));
            let snapshot = plait.host.snapshot.as_ref().unwrap();
            if let Some(thread) = plait
                .host
                .threads
                .iter_mut()
                .find(|thread| thread.id == snapshot.id)
            {
                thread.status = snapshot.status;
            }
            plait.sync_composer(cx);
        });
    }

    /// Deliver one pointer event to the window, as the platform would. Handlers
    /// belong to the last drawn frame, so draw with `capture` first.
    fn pointer(&mut self, event: PlatformInput) {
        let window = self.window.into();
        self.cx
            .update_window(window, |_, window, cx| {
                window.dispatch_event(event, cx);
            })
            .expect("pointer event");
        self.settle();
    }

    /// Press one key combination, dispatched from the focused element.
    fn keystroke(&mut self, keys: &'static str) {
        let keystroke = Keystroke::parse(keys).expect("keystroke");
        let window = self.window.into();
        self.cx
            .update_window(window, |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            })
            .expect("keystroke");
        self.settle();
    }

    /// Run layout and paint without saving an image.
    fn draw(&mut self) {
        let window = self.window.into();
        self.cx
            .update_window(window, |_, window, cx| window.draw(cx).clear(cx))
            .expect("draw");
    }

    /// Draw the scene, which runs layout and paint, and save it when rendering.
    fn capture(&mut self, name: &str) {
        self.draw();
        let window = self.window.into();
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
                    ..Default::default()
                },
                ThreadSummary {
                    id: "thr_0c44a9d1".into(),
                    checkout: checkout.clone(),
                    model: choice(),
                    status: ThreadStatus::Idle,
                    ..Default::default()
                },
            ];
            // Mirror select_host_thread without opening a socket.
            plait.host.selected = Some(THREAD.into());
            plait.history = Default::default();
            plait.host.snapshot = Some(Arc::new(thread_snapshot(&checkout, approval)));
            plait.formatted = Arc::new(plait_core::markdown::Transcript::update(
                &plait.formatted,
                plait.host.snapshot.as_ref().unwrap(),
            ));
            plait.settings_open = false;
            plait.new_thread_open = false;
            plait.show_changes = false;
            let composer = cx.new(|cx| Composer::new(window, cx));
            plait.composers.insert(THREAD.into(), composer);
            plait.refresh_live_list(None);
            plait
                .session
                .data
                .get_or_insert_with(|| plait_core::session::Session::empty("/fixture-host".into()));
            plait.capture_session(cx);
            plait.sync_composer(cx);
        });
    }
}

#[core::prelude::v1::test]
fn desktop_recovery_does_not_rebind_saved_work_or_clear_a_newer_revision() {
    if !isolated("desktop_recovery_does_not_rebind_saved_work_or_clear_a_newer_revision") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-recovery-ui-{}", std::process::id()));
    let repository = fixture_repository(&scratch);
    let mut harness = Harness::new(&repository, None, 900., 600.);
    harness.connect(&repository, false);
    harness.update(|plait, window, cx| {
        let pending = plait
            .host
            .submissions
            .begin(THREAD, "first".into())
            .unwrap();
        plait.composers[THREAD].update(cx, |composer, cx| {
            composer.restore_draft("first".into(), 7, Some(("first".into(), 6)), cx);
        });
        plait.capture_session(cx);
        let saved = plait.session.data.as_ref().unwrap().threads[THREAD].clone();
        plait.host.threads[0].checkout = "/a-different-checkout".into();
        let snapshot = Arc::make_mut(plait.host.snapshot.as_mut().unwrap());
        snapshot.checkout = plait.host.threads[0].checkout.clone();
        snapshot.messages.push(Message {
            id: format!("request-{}", pending.request_id),
            role: MessageRole::User,
            text: "first".into(),
            ..Default::default()
        });
        let composer = cx.new(|cx| Composer::new(window, cx));
        plait.restore_composer(THREAD, &composer, cx);
        assert_eq!(composer.read(cx).saved_draft().0, "");
        plait.composers.insert(THREAD.into(), composer.clone());
        plait.capture_session(cx);
        plait.record_snapshot(cx);
        assert_eq!(plait.session.data.as_ref().unwrap().threads[THREAD], saved);
        assert!(!plait.session_accepts_thread(THREAD));
        assert!(plait.host.submissions.pending(THREAD).is_some());
        // Only the matching identity can reconcile the accepted request, not a reused thread ID.
        plait.host.threads[0].checkout = saved.checkout.clone();
        Arc::make_mut(plait.host.snapshot.as_mut().unwrap()).checkout = saved.checkout.clone();
        plait.restore_composer(THREAD, &composer, cx);
        plait.record_snapshot(cx);
        assert!(plait.host.submissions.pending(THREAD).is_none());
        assert_eq!(composer.read(cx).saved_draft().0, "first");
        assert_eq!(composer.read(cx).saved_draft().1, 7);
    });
    drop(harness);
    std::fs::remove_dir_all(scratch).unwrap();
}

/// New Thread creates at once with the default, asks when there is none, and
/// never replaces the request ID of a creation the host has not confirmed.
#[core::prelude::v1::test]
fn new_thread_keeps_its_request_id_and_overlays_return_focus() {
    if !isolated("new_thread_keeps_its_request_id_and_overlays_return_focus") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-new-thread-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let repository = fixture_repository(&scratch);
    let mut harness = Harness::new(&repository, None, 900., 600.);
    harness.connect(&repository, false);
    harness.update(|plait, window, cx| {
        let before = window.focused(cx);
        plait.toggle_command_palette(window, cx);
        assert!(plait.overlay.is_some());
        assert!(
            window.focused(cx) != before,
            "the palette takes the keyboard"
        );
        plait.toggle_command_palette(window, cx);
        assert!(plait.overlay.is_none());
        assert!(window.focused(cx) == before, "closing returns focus");
        // Without a usable default, nothing is created until a model is chosen.
        plait.host.status.as_mut().unwrap().default_model = None;
        plait.new_thread(None, window, cx);
        assert!(plait.overlay.is_some());
        assert!(plait.host.pending_create.is_none() && !plait.host.creating);
        plait.close_overlay(true, window, cx);
        plait.host.status = Some(status());
        // With one, the request is sent at once and no page or picker opens.
        plait.new_thread(None, window, cx);
        assert!(plait.overlay.is_none());
        assert!(plait.host.creating);
    });
    // No host runs in this scratch home, so the request fails and stays unconfirmed.
    let unconfirmed = |harness: &mut Harness| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            harness.settle();
            let pending = harness
                .window
                .update(&mut harness.cx, |plait, _, _| {
                    (!plait.host.creating).then(|| plait.host.pending_create.clone())
                })
                .expect("window update");
            if let Some(pending) = pending {
                return pending.expect("an unconfirmed creation is kept");
            }
            assert!(Instant::now() < deadline, "creation did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    let first = unconfirmed(&mut harness);
    assert_eq!(first.1, choice());
    harness.update(|plait, window, cx| {
        assert!(plait.host.error.is_some());
        assert!(plait.creation_bar(cx).is_some(), "the retry is offered");
        // Asking again, even for another model, retries the same request.
        plait.new_thread(Some(other_choice()), window, cx);
        assert!(plait.host.creating);
    });
    assert_eq!(unconfirmed(&mut harness), first);
    drop(harness);
    std::fs::remove_dir_all(scratch).unwrap();
}

/// Draw, then deliver one key as the platform would: bindings first, then text input.
fn press(harness: &mut Harness, key: &'static str) {
    harness.capture("keys");
    let window = harness.window.into();
    harness
        .cx
        .update_window(window, |_, window, cx| {
            window.dispatch_keystroke(Keystroke::parse(key).expect("keystroke"), cx);
        })
        .expect("key");
    harness.settle();
}

fn shown(harness: &mut Harness) -> Vec<String> {
    harness
        .window
        .update(&mut harness.cx, |plait, _, cx| {
            let overlay = plait.overlay.as_ref().expect("open overlay");
            overlay.palette.read(cx).shown()
        })
        .expect("window update")
}

/// Real key events reach the palette: Cmd-K opens it, typing filters, the
/// arrows move, Enter runs, and Escape returns focus.
#[core::prelude::v1::test]
fn keys_drive_the_palette_and_picker() {
    if !isolated("keys_drive_the_palette_and_picker") {
        return;
    }
    let scratch = std::env::temp_dir().join(format!("plait-keys-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let repository = fixture_repository(&scratch);
    let mut harness = Harness::new(&repository, None, 900., 600.);
    harness.connect(&repository, false);
    let focused = |harness: &mut Harness| {
        harness
            .window
            .update(&mut harness.cx, |_, window, cx| window.focused(cx))
            .expect("window update")
    };
    let before = focused(&mut harness);

    press(&mut harness, "cmd-k");
    assert_eq!(
        shown(&mut harness).len(),
        2 + super::actions::ACTIONS
            .iter()
            .filter(|action| action.place != super::actions::Place::Hidden
                && action.id != "palette.toggle")
            .count()
            + 5,
        "threads, commands, models"
    );
    press(&mut harness, "escape");
    harness.update(|plait, _, _| assert!(plait.overlay.is_none()));
    assert!(focused(&mut harness) == before, "Escape returns focus");

    // Typed letters go to the search field and rank the list.
    press(&mut harness, "cmd-k");
    for key in ["s", "i", "d", "e"] {
        press(&mut harness, key);
    }
    assert_eq!(shown(&mut harness)[0], "Toggle Sidebar");
    press(&mut harness, "backspace");
    press(&mut harness, "e");
    press(&mut harness, "enter");
    harness.update(|plait, _, _| {
        assert!(plait.overlay.is_none());
        assert!(!plait.sidebar, "Enter ran the selected command");
    });

    // Down moves to the second thread; Enter opens it and focuses its composer.
    press(&mut harness, "cmd-k");
    press(&mut harness, "down");
    press(&mut harness, "enter");
    harness.update(|plait, window, cx| {
        assert_eq!(plait.host.selected.as_deref(), Some("thr_0c44a9d1"));
        let composer = plait.composers["thr_0c44a9d1"].focus_handle(cx);
        assert!(window.focused(cx) == Some(composer));
        // Selecting dropped the sample connection; restore it for the picker.
        plait.host.connected = true;
        plait.host.error = None;
    });

    // Cmd-Shift-N asks for a model. Tab stays inside the overlay.
    press(&mut harness, "cmd-shift-n");
    assert_eq!(shown(&mut harness).len(), 42, "the whole catalog is listed");
    let field = focused(&mut harness);
    press(&mut harness, "tab");
    assert!(focused(&mut harness) == field);
    for key in ["a", "s", "t", "r", "a"] {
        press(&mut harness, key);
    }
    assert_eq!(shown(&mut harness), vec!["GPT-6 Astra"]);
    // Cmd-Return saves the default without closing; no host answers here.
    press(&mut harness, "cmd-enter");
    harness.update(|plait, _, _| {
        assert!(plait.overlay.is_some());
        let astra = ModelChoice {
            provider: "openai-codex".into(),
            model_id: "gpt-6-astra".into(),
        };
        assert_eq!(plait.host.settings_model, Some(astra));
        // The host's default is unchanged, and the state says so.
        assert_eq!(
            plait.host.status.as_ref().unwrap().default_model,
            Some(choice())
        );
        let (state, _) = plait.default_model_state().expect("save state");
        assert!(state.contains("not saved"), "{state}");
    });
    drop(harness);
    std::fs::remove_dir_all(scratch).unwrap();
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
        tool_states_walk(&mut harness, &repository, suffix);
        reading_walk(&mut harness, &repository, suffix);
        history_walk(&mut harness, &repository, suffix);
        tool_cards_walk(&mut harness, &repository, suffix);
        draft_walk(&mut harness, &repository, suffix);
        composer_walk(&mut harness, &repository, suffix);
        composer_v3_walk(&mut harness, &repository, suffix);
        settings_walk(&mut harness, &repository, suffix);
        settings_controls_walk(&mut harness, &repository, suffix);
        palette_walk(&mut harness, &repository, suffix);
        picker_walk(&mut harness, &repository, suffix);
        new_thread_walk(&mut harness, &repository, suffix);
        inspector_walk(&mut harness, &repository, suffix);
        // Threads exist, but none is open.
        harness.update(|plait, _, _| {
            plait.host_watch = None;
            plait.host.select(None);
        });
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
            ..Default::default()
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
            plait.formatted = Arc::new(plait_core::markdown::Transcript::update(
                &plait.formatted,
                plait.host.snapshot.as_ref().unwrap(),
            ));
            plait.refresh_live_list(Some(&previous));
            assert_eq!(plait.live_scroll.bounds_for_item(anchor.item_ix), measured);
            let current = plait.live_scroll.logical_scroll_top();
            assert_eq!(current.item_ix, anchor.item_ix);
            assert_eq!(current.offset_in_item, anchor.offset_in_item);
            assert!(plait.expanded_live_tools.contains("t2"));
        });
    }
    // Text arrived while the reader was elsewhere in the history.
    harness.capture(&format!("stream-new-activity-{suffix}"));
    harness.change_snapshot(|snapshot| {
        snapshot.messages.remove(0);
        snapshot.messages.push(Message {
            id: "new-user".into(),
            role: MessageRole::User,
            text: "Sample follow-up".into(),
            ..Default::default()
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

/// Done, failed, and interrupted tool calls, two of them expanded, with the
/// host's error line and the clipped-history note.
fn tool_states_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.change_snapshot(tool_states);
    harness.update(|plait, _, cx| {
        // The sidebar and composer follow the thread's status, as a host update would set it.
        plait.host.threads[0].status = ThreadStatus::Error;
        plait.sync_composer(cx);
        plait.toggle_live_tool("t2", cx);
        plait.toggle_live_tool("t3", cx);
        plait.live_scroll.scroll_to_end();
    });
    harness.capture(&format!("live-tools-{suffix}"));
    // Later scenes show the usual thread list again.
    harness.connect(repository, false);
}

/// Prose, a table, links, a selection across blocks, and highlighted code.
fn reading_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    use super::selection::Owner;
    harness.connect(repository, false);
    harness.change_snapshot(reading);
    let at = |item_ix| ListOffset {
        item_ix,
        offset_in_item: px(0.),
    };
    harness.update(|plait, _, cx| {
        plait.host.threads[0].status = ThreadStatus::Idle;
        plait.sync_composer(cx);
        plait.live_scroll.scroll_to(at(0));
    });
    harness.capture(&format!("read-prose-{suffix}"));
    let reply = || Owner::Message("r2".into());
    // Where the caret before a byte of one of the reply's blocks is on screen.
    let caret = |harness: &mut Harness, block: usize, offset: usize| {
        let found = harness.window.update(&mut harness.cx, |plait, _, _| {
            plait.text_point(&reply(), block, offset)
        });
        found.unwrap().expect("text is on screen")
    };
    let press = |position, click_count| {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count,
            first_mouse: false,
        })
    };
    let release = |position| {
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        })
    };
    let moved = |position, pressed_button| {
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button,
            modifiers: Modifiers::default(),
        })
    };
    let selected = |harness: &mut Harness| {
        harness
            .window
            .update(&mut harness.cx, |plait, _, _| plait.selected_text())
            .unwrap()
    };

    // Resting on a link shows where it leads, and the row's quiet controls.
    let link = caret(
        harness,
        1,
        "This is a rendering fixture, not an agent run. See the G".len(),
    );
    harness.pointer(moved(link, None));
    harness.update(|plait, _, _| {
        assert_eq!(
            plait.reading.hovered_link.as_deref(),
            Some("https://gpui.rs/")
        );
    });
    harness.capture(&format!("read-hover-{suffix}"));
    // A click that starts and ends on the link follows it and selects nothing.
    harness.pointer(press(link, 1));
    harness.pointer(release(link));
    harness.capture(&format!("read-hover-{suffix}"));
    assert_eq!(selected(harness), None);

    // Drag from inside the first paragraph into a later block: the table when
    // the window is tall enough to show it, otherwise a list item.
    let from = caret(
        harness,
        1,
        "This is a rendering fixture, not an agent run. ".len(),
    );
    let (block, offset, ending) = if suffix == "wide" {
        (
            10,
            "Crate\tRole\tLines\nplait\tGPUI presentation".len(),
            "plait\tGPUI presentation",
        )
    } else {
        (5, "2. Second".len(), "\n2. Second")
    };
    let to = caret(harness, block, offset);
    harness.pointer(press(from, 1));
    harness.pointer(moved(to, Some(MouseButton::Left)));
    harness.capture(&format!("read-selection-{suffix}"));
    harness.pointer(release(to));
    let copied = selected(harness).expect("dragged selection");
    assert!(copied.starts_with("See the GPUI site"), "{copied}");
    assert!(copied.contains("1. First step\n◦ nested point"), "{copied}");
    assert!(copied.ends_with(ending), "{copied}");
    // Cmd-C reaches the conversation because the press moved focus to it.
    harness.update(|_, _, cx| super::selection::bind_keys(cx));
    harness.keystroke("cmd-c");
    harness.update(|_, _, cx| {
        let clipboard = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(clipboard.as_deref(), Some(copied.as_str()));
    });

    // A double click takes a word; a shift-click extends from where it began.
    let word = caret(harness, 5, "2. Sec".len());
    harness.pointer(press(word, 2));
    harness.pointer(release(word));
    harness.capture(&format!("read-word-{suffix}"));
    assert_eq!(selected(harness).as_deref(), Some("Second"));
    let end = caret(harness, 5, "2. Second step".len());
    harness.pointer(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: end,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
        click_count: 1,
        first_mouse: false,
    }));
    harness.pointer(release(end));
    harness.capture(&format!("read-word-{suffix}"));
    assert_eq!(selected(harness).as_deref(), Some("Second step"));
    // A triple click takes the block, without its list marker.
    let item = caret(harness, 2, "1. Fir".len());
    harness.pointer(press(item, 3));
    harness.pointer(release(item));
    harness.capture(&format!("read-word-{suffix}"));
    assert_eq!(selected(harness).as_deref(), Some("First step"));
    harness.pointer(press(from, 1));
    harness.pointer(release(from));
    harness.capture(&format!("read-word-{suffix}"));
    assert_eq!(
        selected(harness),
        None,
        "a plain click clears the selection"
    );

    // Dragging past the bottom edge scrolls, and the selection follows.
    let (below, before) = harness
        .window
        .update(&mut harness.cx, |plait, _, _| {
            let list = plait.live_scroll.viewport_bounds();
            let offset = plait.live_scroll.scroll_px_offset_for_scrollbar().y;
            (point(from.x, list.bottom() + px(60.)), offset)
        })
        .unwrap();
    harness.pointer(press(from, 1));
    for _ in 0..16 {
        harness.pointer(moved(below, Some(MouseButton::Left)));
        harness.draw();
    }
    harness.capture(&format!("read-drag-scroll-{suffix}"));
    harness.pointer(release(below));
    harness.update(|plait, _, _| {
        let offset = plait.live_scroll.scroll_px_offset_for_scrollbar().y;
        assert!(offset < before - px(200.), "{offset} {before}");
        let copied = plait.selected_text().expect("selection across rows");
        assert!(copied.starts_with("See the GPUI site"), "{copied}");
        assert!(
            copied.contains("\n\nAnd the code samples.\n\n// Sample code"),
            "{copied}"
        );
    });

    harness.update(|plait, _, _| plait.live_scroll.scroll_to(at(3)));
    harness.capture(&format!("read-code-{suffix}"));
    // In code, a triple click takes one line.
    let code = Owner::Message("r4".into());
    let line = harness
        .window
        .update(&mut harness.cx, |plait, _, _| {
            plait.text_point(&code, 0, "// Sample code\npub fn re".len())
        })
        .unwrap()
        .expect("code is on screen");
    harness.pointer(press(line, 3));
    harness.pointer(release(line));
    harness.capture(&format!("read-code-{suffix}"));
    assert_eq!(
        selected(harness).as_deref(),
        Some("pub fn refresh<'a>(queue: &'a mut Queue) -> Result<usize, Error> {")
    );
    // The control over the list returns to the newest row.
    let jump = harness
        .window
        .update(&mut harness.cx, |plait, _, _| {
            assert!(plait.live_scroll.logical_scroll_top().item_ix < plait.live_count);
            let list = plait.live_scroll.viewport_bounds();
            // It ends at the right edge of the text column.
            let column = (list.size.width - px(48.)).min(px(760.));
            point(
                list.center().x + column * 0.5 - px(40.),
                list.bottom() - px(24.),
            )
        })
        .unwrap();
    harness.pointer(press(jump, 1));
    harness.pointer(release(jump));
    harness.capture(&format!("read-latest-{suffix}"));
    harness.update(|plait, _, _| {
        let top = plait.live_scroll.logical_scroll_top();
        assert_eq!(top.item_ix, plait.live_count, "at the newest row");
    });
    harness.capture(&format!("read-data-{suffix}"));
    harness.connect(repository, false);
}

/// Each tool's presentation, then a folded and an opened run of calls.
fn tool_cards_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.change_snapshot(tool_cards);
    harness.update(|plait, _, cx| {
        plait.host.threads[0].status = ThreadStatus::Idle;
        plait.sync_composer(cx);
        for id in ["c1", "c2", "c3"] {
            plait.toggle_live_tool(id, cx);
        }
        plait.live_scroll.scroll_to_end();
    });
    harness.capture(&format!("tools-files-{suffix}"));
    harness.update(|plait, _, cx| {
        for id in ["c1", "c2", "c3", "c4", "c5", "c6"] {
            plait.toggle_live_tool(id, cx);
        }
        plait.live_scroll.scroll_to_end();
    });
    harness.capture(&format!("tools-commands-{suffix}"));

    harness.connect(repository, false);
    harness.change_snapshot(|snapshot| long_run(snapshot, false));
    harness.update(|plait, _, _| plait.live_scroll.scroll_to_end());
    harness.capture(&format!("tools-group-{suffix}"));
    // How the group's first two rows were last drawn: (hidden, leads the group).
    let drawn = |plait: &Plait| {
        let folds = plait.reading.folds.borrow();
        (folds["g0"], folds["g1"])
    };
    harness.update(|plait, _, _| {
        assert_eq!(drawn(plait), ((true, true), (true, false)));
    });
    harness.update(|plait, _, cx| {
        plait.toggle_tool_group("g0", cx);
        plait.toggle_live_tool("g3", cx);
        plait.live_scroll.scroll_to_end();
    });
    harness.capture(&format!("tools-group-open-{suffix}"));
    harness.update(|plait, _, _| {
        assert_eq!(drawn(plait), ((false, true), (false, false)));
        assert!(plait.tool_details_shown("g3"));
    });
    harness.update(|plait, _, cx| plait.toggle_tool_group("g0", cx));
    harness.capture(&format!("tools-group-{suffix}"));
    harness.update(|plait, _, _| {
        assert_eq!(drawn(plait), ((true, true), (true, false)));
        // Its details stay open for when the group is, but are not on screen.
        assert!(!plait.tool_details_shown("g3"));
    });

    harness.connect(repository, false);
    harness.change_snapshot(|snapshot| long_run(snapshot, true));
    harness.update(|plait, _, _| plait.live_scroll.scroll_to_end());
    harness.capture(&format!("tools-running-{suffix}"));
    harness.connect(repository, false);
}

/// A finished turn with an unsent draft, so the composer is ready to send.
fn draft_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.change_snapshot(finished_turn);
    harness.update(|plait, _, cx| {
        plait.host.threads[0].status = ThreadStatus::Idle;
        plait.composers[THREAD].update(cx, |composer, cx| {
            composer.restore_draft(DRAFT.into(), 1, None, cx);
        });
        plait.sync_composer(cx);
        plait.live_scroll.scroll_to_end();
    });
    harness.capture(&format!("live-draft-{suffix}"));
    harness.connect(repository, false);
}

/// Sample checkout paths for the file-suggestion scenes. Not read from disk.
fn sample_files() -> Arc<plait_core::git::FileList> {
    let paths = [
        "Cargo.toml",
        "README.md",
        "crates/plait-core/src/git.rs",
        "crates/plait-core/src/git/tests.rs",
        "crates/plait-core/src/host_state.rs",
        "crates/plait-core/src/state.rs",
        "crates/plait/src/ui/changes.rs",
        "crates/plait/src/ui/composer/mod.rs",
        "crates/plait/src/ui/host.rs",
        "crates/plait/src/ui/session.rs",
        "crates/plait/src/ui/settings.rs",
        "docs/ARCHITECTURE.md",
        "docs/HOST.md",
        "scripts/snapshots.sh",
    ];
    Arc::new(plait_core::git::FileList {
        paths: paths.map(String::from).to_vec(),
        ..Default::default()
    })
}

impl Harness {
    /// A ready-to-send composer with nothing typed, on a finished turn.
    fn fresh_composer(&mut self, repository: &Path) {
        self.connect(repository, false);
        self.change_snapshot(finished_turn);
        self.update(|plait, _, cx| {
            plait.host.threads[0].status = ThreadStatus::Idle;
            plait.sync_composer(cx);
            plait.live_scroll.scroll_to_end();
        });
    }

    fn composer<R>(
        &mut self,
        change: impl FnOnce(&mut Composer, &mut Window, &mut Context<Composer>) -> R,
    ) -> R {
        let result = self
            .window
            .update(&mut self.cx, |plait, window, cx| {
                plait.composers[THREAD].update(cx, |composer, cx| change(composer, window, cx))
            })
            .expect("window update");
        self.settle();
        result
    }

    /// Wait for the file read and the background ranking behind a suggestion list.
    fn suggestions(&mut self) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let names = self.composer(|composer, _, _| composer.suggestion_names());
            if !names.is_empty() {
                return names;
            }
            assert!(Instant::now() < deadline, "suggestions did not arrive");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// The composer at each height, and its two suggestion lists.
fn composer_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.fresh_composer(repository);
    harness.capture(&format!("composer-empty-{suffix}"));

    // Up walks the thread's own user messages; Down returns the empty draft.
    harness.composer(|composer, window, cx| {
        composer.press_arrow(false, window, cx);
        assert_eq!(composer.text(), "Go ahead.");
        assert_eq!(composer.saved_draft().0, "");
        composer.press_arrow(false, window, cx);
        assert!(composer.text().starts_with("Queue the status refresh"));
        composer.press_arrow(true, window, cx);
        composer.press_arrow(true, window, cx);
        assert_eq!(composer.text(), "");
    });
    // A chosen command reaches the window, which performs it.
    harness.update(|plait, window, cx| {
        let event =
            super::composer::ComposerEvent::Command(super::composer::CommandId::ShowChanges);
        plait.composer_event(&event, window, cx);
        assert!(plait.show_changes);
        let event =
            super::composer::ComposerEvent::Command(super::composer::CommandId::ShowConversation);
        plait.composer_event(&event, window, cx);
        assert!(!plait.show_changes);
    });

    let lines = |count: usize| {
        (1..=count)
            .map(|line| format!("Sample draft line {line}: keep the public signature as it is."))
            .collect::<Vec<_>>()
            .join("\n")
    };
    harness.composer(|composer, _, cx| composer.restore_draft(lines(6), 1, None, cx));
    harness.capture(&format!("composer-lines-{suffix}"));
    // Far more lines than fit: the input stops growing and scrolls to its cursor.
    harness.composer(|composer, _, cx| composer.restore_draft(lines(60), 2, None, cx));
    harness.capture(&format!("composer-tall-{suffix}"));

    // A real mention: the list comes from the fixture repository through Git.
    harness.composer(|composer, window, cx| {
        composer.restore_draft(String::new(), 3, None, cx);
        composer.type_text("Compare @rev", window, cx);
    });
    harness.update(|plait, _, cx| plait.supply_files(cx));
    assert_eq!(harness.suggestions(), ["src/review.rs"]);
    harness.composer(|composer, window, cx| {
        assert!(composer.accept_suggestion(window, cx));
        assert_eq!(composer.text(), "Compare src/review.rs ");
        assert!(composer.suggestion_names().is_empty());
    });

    // A fuller sample list, to see eight rows, emphasis, and the list's edges.
    harness.composer(|composer, window, cx| {
        composer.restore_draft(String::new(), 4, None, cx);
        composer.set_files(super::composer::MentionFiles::Ready(sample_files()), cx);
        composer.type_text("Why does @s", window, cx);
    });
    assert!(harness.suggestions().len() > 8);
    harness.capture(&format!("composer-mention-{suffix}"));
    harness.composer(|composer, window, cx| {
        composer.restore_draft(String::new(), 5, None, cx);
        composer.type_text("@ui/se", window, cx);
    });
    assert_eq!(harness.suggestions()[0], "crates/plait/src/ui/session.rs");
    harness.capture(&format!("composer-mention-filtered-{suffix}"));

    harness.composer(|composer, window, cx| {
        composer.restore_draft(String::new(), 6, None, cx);
        composer.type_text("/", window, cx);
    });
    assert_eq!(harness.suggestions().len(), 5);
    harness.capture(&format!("composer-command-{suffix}"));
    harness.composer(|composer, window, cx| {
        composer.type_text("set", window, cx);
        assert_eq!(composer.suggestion_names()[0], "/settings: Open settings");
    });
    harness.capture(&format!("composer-command-filtered-{suffix}"));
    harness.update(|plait, window, cx| window.focus(&plait.focus, cx));
    harness.connect(repository, false);
}

/// Settings sections, with no signed-in model, and after a default-model save failed.
fn settings_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.update(|plait, _, _| plait.settings_open = true);
    harness.capture(&format!("settings-{suffix}"));
    harness.update(|plait, _, _| {
        for model in &mut plait.host.status.as_mut().unwrap().models {
            model.authenticated = false;
        }
    });
    harness.capture(&format!("settings-no-auth-{suffix}"));
    harness.update(|plait, _, _| {
        plait.host.status = Some(status());
        // As a failed save leaves it: the choice is kept, the host's default unchanged.
        plait.host.settings_model = Some(other_choice());
        plait
            .host
            .finish_save(Err("sample error; the host did not answer.".into()));
    });
    harness.capture(&format!("settings-save-failed-{suffix}"));
    harness.update(|plait, _, _| {
        plait.host.settings_model = Some(choice());
        plait.host.save_message = None;
        plait.settings_open = false;
    });
}

fn set_query(harness: &mut Harness, query: &'static str) {
    harness.update(move |plait, _, cx| {
        let overlay = plait.overlay.as_ref().expect("open overlay");
        overlay
            .palette
            .update(cx, |palette, cx| palette.set_query(query, cx));
    });
}

/// The command palette: everything grouped, a ranked query, and no matches.
fn palette_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.update(|plait, window, cx| plait.toggle_command_palette(window, cx));
    harness.capture(&format!("palette-{suffix}"));
    set_query(harness, "new");
    harness.capture(&format!("palette-query-{suffix}"));
    set_query(harness, "zzzz");
    harness.capture(&format!("palette-no-matches-{suffix}"));
    harness.update(|plait, window, cx| {
        plait.toggle_command_palette(window, cx);
        assert!(plait.overlay.is_none());
    });
}

/// The model picker: favorites, recents, and provider groups; a search that
/// mixes signed-in and signed-out models; the signed-out groups; no matches.
fn picker_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.update(|plait, window, cx| {
        // In memory only: the scene does not write a preferences file.
        let preferences = &mut plait.preferences.value;
        preferences.toggle_favorite(&other_choice());
        preferences.toggle_favorite(&ModelChoice {
            provider: "google".into(),
            model_id: "gemini-3-pro".into(),
        });
        preferences.record_recent(&ModelChoice {
            provider: "openai-codex".into(),
            model_id: "gpt-6-astra".into(),
        });
        plait.open_model_picker(ModelPurpose::NewThread, window, cx);
    });
    harness.capture(&format!("model-picker-{suffix}"));
    set_query(harness, "pro");
    harness.capture(&format!("model-picker-query-{suffix}"));
    set_query(harness, "zzzz");
    harness.capture(&format!("model-picker-no-matches-{suffix}"));
    set_query(harness, "");
    harness.update(|plait, _, cx| {
        let overlay = plait.overlay.as_ref().expect("open picker");
        overlay.palette.update(cx, |palette, cx| {
            palette.move_by(true, 12, cx);
            palette.move_by(true, 12, cx);
        });
    });
    harness.capture(&format!("model-picker-scrolled-{suffix}"));
    harness.update(|plait, window, cx| {
        plait.close_overlay(true, window, cx);
        // Disconnected: the picker says why it has nothing to list.
        plait.host.connected = false;
        plait.open_model_picker(ModelPurpose::DefaultModel, window, cx);
    });
    harness.capture(&format!("model-picker-disconnected-{suffix}"));
    harness.update(|plait, window, cx| {
        plait.close_overlay(true, window, cx);
        plait.preferences.value = Default::default();
    });
}

/// A thread creation the host has not confirmed, with its retry.
fn new_thread_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.update(|plait, _, _| {
        plait.host.pending_create = Some(("sample-request".into(), choice()));
        plait.host.error =
            Some("Sample error. Retry creation to reuse the same request ID.".into());
    });
    harness.capture(&format!("new-thread-unconfirmed-{suffix}"));
    harness.update(|plait, _, _| {
        plait.host.pending_create = None;
        plait.host.error = None;
    });
}

/// The Git inspector hidden. A narrow window has no inspector, so the toggle
/// explains where Git is instead.
fn inspector_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    harness.connect(repository, false);
    harness.update(|plait, window, cx| {
        plait.toggle_inspector(window, cx);
        let wide = Plait::wide(window);
        assert_eq!(plait.preferences.value.hide_inspector, wide);
        assert_eq!(plait.inspector_note, !wide);
    });
    harness.capture(&format!("inspector-hidden-{suffix}"));
    harness.update(|plait, window, cx| {
        plait.toggle_inspector(window, cx);
        assert!(!plait.preferences.value.hide_inspector);
        assert!(!plait.inspector_note);
    });
}

// The glob import above brings in `gpui::test`; these are plain tests.

/// Layout and paint run for every view. GPUI reports some misuse, such as a
/// second hover style on one element, only when the element is drawn.
#[core::prelude::v1::test]
fn every_view_draws() {
    if !isolated("every_view_draws") {
        return;
    }
    walk(None);
}

#[core::prelude::v1::test]
#[ignore = "writes PNG files; set PLAIT_SNAPSHOT_DIR and pass --ignored"]
fn snapshot() {
    if !isolated("snapshot") {
        return;
    }
    let Some(output) = std::env::var_os("PLAIT_SNAPSHOT_DIR").map(PathBuf::from) else {
        panic!("Set PLAIT_SNAPSHOT_DIR to an output directory.");
    };
    std::fs::create_dir_all(&output).unwrap();
    walk(Some(&output));
}

/// Settings controls use injected catalogs and inert sign-in fixtures.
fn settings_controls_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    use super::{
        actions,
        settings_providers::{EndpointForm, Form},
        text_field::TextField,
    };
    use plait_core::host::{policy::CommandPolicy, providers::*, sign_in::*};
    harness.connect(repository, false);
    harness.update(|plait, window, cx| {
        plait.show_settings(true, window, cx);
        plait.settings_section = 3;
        plait
            .keyboard
            .search
            .as_ref()
            .unwrap()
            .update(cx, |field, cx| field.set_text("sidebar", cx));
    });
    harness.capture(&format!("keyboard-filtered-{suffix}"));
    let sidebar = actions::shortcuts()
        .iter()
        .position(|row| row.actions[0].id == "view.toggle_sidebar")
        .unwrap();
    harness.update(|plait, window, cx| plait.record_shortcut(sidebar, window, cx));
    harness.capture(&format!("keyboard-recording-{suffix}"));
    harness.keystroke("cmd-k");
    harness.update(|plait, _, _| {
        assert!(
            plait.overlay.is_none(),
            "recording intercepted the command palette"
        )
    });
    harness.capture(&format!("keyboard-conflict-{suffix}"));
    harness.update(|plait, window, cx| plait.record_shortcut(sidebar, window, cx));
    harness.draw();
    harness.keystroke("ctrl-shift-b");
    harness.update(|plait, _, _| assert!(plait.sidebar));
    harness.draw();
    harness.keystroke("cmd-b");
    harness.update(|plait, _, _| assert!(plait.sidebar, "old shortcut no longer runs"));
    harness.keystroke("ctrl-shift-b");
    harness.update(|plait, _, _| assert!(!plait.sidebar, "new shortcut runs"));
    harness.capture(&format!("keyboard-customized-{suffix}"));
    harness.update(|plait, _, cx| {
        plait.sidebar = true;
        plait.preferences.value.keymap.clear();
        actions::install(cx, &plait.preferences.value.keymap);
        plait
            .keyboard
            .search
            .as_ref()
            .unwrap()
            .update(cx, |field, cx| field.set_text("", cx));
    });
    harness.capture(&format!("keyboard-default-{suffix}"));
    harness.update(|plait, _, _| {
        plait.settings_section = 1;
        plait.providers.loaded = true;
        plait.providers.rows = vec![
            Provider {
                id: "sample-subscription".into(),
                name: "Sample subscription provider".into(),
                kind: ProviderKind::Builtin,
                authenticated: true,
                credential: Credential::SignIn,
                model_count: 3,
                sign_in: Some(SignInMethod {
                    name: "Sign in".into(),
                    subscription: true,
                }),
                api_key: Some(KeyMethod {
                    name: "API key".into(),
                }),
                endpoint: None,
            },
            Provider {
                id: "openrouter".into(),
                name: "Sample API provider".into(),
                kind: ProviderKind::Builtin,
                authenticated: false,
                credential: Credential::None,
                model_count: 8,
                sign_in: None,
                api_key: Some(KeyMethod {
                    name: "API key".into(),
                }),
                endpoint: None,
            },
        ];
    });
    harness.capture(&format!("providers-list-{suffix}"));
    harness.update(|plait, _, cx| {
        plait.providers.form = Some(Form::Key {
            provider: "sample-provider".into(),
            field: cx.new(|cx| TextField::new("Provider API key", "Paste here", cx).secret()),
        });
    });
    harness.update(|plait, window, cx| {
        let Some(Form::Key { field, .. }) = &plait.providers.form else {
            panic!("key form");
        };
        field.update(cx, |field, cx| field.set_text("synthetic credential", cx));
        window.focus(&field.focus_handle(cx), cx);
        cx.write_to_clipboard(ClipboardItem::new_string(
            "untouched fixture clipboard".into(),
        ));
    });
    harness.draw();
    harness.keystroke("cmd-a");
    harness.keystroke("cmd-c");
    harness.update(|plait, _, cx| {
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("untouched fixture clipboard")
        );
        let Some(Form::Key { field, .. }) = &plait.providers.form else {
            panic!("key form");
        };
        let sent = field.update(cx, |field, cx| field.take_secret(cx));
        assert!(!sent.is_empty());
        assert!(field.read(cx).text().is_empty());
    });
    harness.keystroke("cmd-z");
    harness.update(|plait, _, cx| {
        let Some(Form::Key { field, .. }) = &plait.providers.form else {
            panic!("key form");
        };
        assert!(
            field.read(cx).text().is_empty(),
            "a sent key cannot be restored with undo"
        );
    });
    harness.capture(&format!("provider-api-key-{suffix}"));
    for (name, state, code, input, error) in [
        ("browser", SignInState::Waiting, None, None, None),
        (
            "device",
            SignInState::Waiting,
            Some("SAMPLE-CODE".to_owned()),
            None,
            None,
        ),
        (
            "paste",
            SignInState::Waiting,
            None,
            Some(SignInInput {
                kind: InputKind::Code,
                message: "Paste the sample sign-in code".into(),
                placeholder: None,
                required: false,
                choices: vec![],
            }),
            None,
        ),
        (
            "failed",
            SignInState::Failed,
            None,
            None,
            Some(Failure {
                code: "sample_failure".into(),
                message: "Sample sign-in was not completed.".into(),
            }),
        ),
    ] {
        harness.update(|plait, _, cx| {
            plait.providers.rows.clear();
            plait.providers.form = Some(Form::Flow {
                state: SignIn {
                    id: "sample-flow".into(),
                    provider: "sample-provider".into(),
                    state,
                    url: Some("https://example.invalid/sign-in".into()),
                    user_code: code,
                    input,
                    message: None,
                    expires_in_seconds: 120,
                    error,
                },
                field: cx.new(|cx| TextField::new("Sign-in answer", "Paste here", cx).secret()),
            });
        });
        harness.capture(&format!("provider-sign-in-{name}-{suffix}"));
    }
    harness.update(|plait, _, cx| {
        let field = |label, text: &str, cx: &mut Context<Plait>| {
            cx.new(|cx| {
                let mut f = TextField::new(label, "", cx);
                f.set_text(text, cx);
                f
            })
        };
        plait.providers.form = Some(Form::Endpoint(EndpointForm {
            id: None,
            name: field("Endpoint name", "Sample local server", cx),
            url: field("Endpoint base URL", "http://localhost:8000/v1", cx),
            models: field("Endpoint model IDs", "sample-model", cx),
            key: cx.new(|cx| TextField::new("Endpoint API key", "Optional", cx).secret()),
            checked: Some(EndpointCheck {
                reachable: true,
                reason: None,
                status: Some(200),
                model_ids: vec!["sample-model".into(), "sample-alternative".into()],
                truncated: false,
            }),
        }));
    });
    harness.capture(&format!("provider-endpoint-{suffix}"));
    harness.update(|plait, _, cx| {
        plait.close_provider_form(cx);
        plait.settings_section = 2;
        plait.policy.key = Some((plait.host.epoch, plait.host.checkout.clone()));
        plait.policy.value = Some(CommandPolicy::Sandboxed);
    });
    harness.update(|plait, _, _| {
        assert_eq!(plait.command_policy(), Some(CommandPolicy::Sandboxed));
        plait.policy.key.as_mut().unwrap().0 += 1;
        assert_eq!(
            plait.command_policy(),
            None,
            "a stale policy must not label this connection sandboxed"
        );
        plait.policy.key.as_mut().unwrap().0 -= 1;
    });
    harness.capture(&format!("policy-sandboxed-{suffix}"));
    harness.update(|plait, _, _| plait.policy.confirm = true);
    harness.capture(&format!("policy-confirm-trusted-{suffix}"));
    harness.update(|plait, _, _| {
        plait.policy.confirm = false;
        plait.policy.value = Some(CommandPolicy::Trusted);
    });
    harness.capture(&format!("policy-trusted-{suffix}"));
    harness.update(|plait, window, cx| {
        plait.show_settings(false, window, cx);
        plait.settings_section = 0;
    });
}

/// Version 3 input and context controls. All state is synthetic and no socket is opened.
fn composer_v3_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    use super::composer::ComposerEvent;
    use plait_core::host::{QueueMode, Queued, ThinkingLevel, Usage};
    harness.fresh_composer(repository);
    harness.update(|plait, _, cx| {
        let status = plait.host.status.as_mut().unwrap();
        status.protocol_version = 3;
        let model = status
            .models
            .iter_mut()
            .find(|m| m.choice() == choice())
            .unwrap();
        model.name = "Sample model with an exceptionally long readable model name".into();
        model.reasoning = Some(true);
        model.thinking_levels = Some(vec![
            ThinkingLevel::Off,
            ThinkingLevel::Low,
            ThinkingLevel::High,
        ]);
        model.context_window = Some(200_000);
        Arc::make_mut(plait.host.snapshot.as_mut().unwrap()).thinking_level =
            Some(ThinkingLevel::Low);
        plait.host.submissions = Default::default();
        plait.sync_composer(cx);
    });
    harness.capture(&format!("composer-v3-idle-{suffix}"));
    harness.composer(|composer, window, cx| {
        let revision = composer.saved_draft().1;
        assert!(composer.restore_cancelled("Restored exact message", revision, cx));
        composer.restore_draft(String::new(), revision + 2, None, cx);
        let empty_revision = composer.saved_draft().1;
        composer.type_text("Newer draft", window, cx);
        assert!(!composer.restore_cancelled("Earlier cancelled message", empty_revision, cx));
        assert_eq!(composer.text(), "Newer draft");
        composer.restore_draft(String::new(), empty_revision + 2, None, cx);
    });
    harness.update(|plait, window, cx| plait.open_thread_models(window, cx));
    harness.capture(&format!("composer-v3-model-picker-{suffix}"));
    harness.update(|plait, window, cx| plait.close_overlay(true, window, cx));
    harness.update(|plait, window, cx| plait.open_thinking(window, cx));
    harness.capture(&format!("composer-v3-thinking-picker-{suffix}"));
    harness.update(|plait, window, cx| plait.close_overlay(true, window, cx));
    harness.change_snapshot(|snapshot| {
        snapshot.status = ThreadStatus::Running;
        snapshot.queued = [QueueMode::FollowUp, QueueMode::Steer]
            .into_iter()
            .enumerate()
            .map(|(i, mode)| Queued {
                id: format!("request-queue-{i}"),
                request_id: format!("queue-{i}"),
                text: if i == 0 {
                    "Then check the edge cases in the parser.".into()
                } else {
                    "Keep the existing public API unchanged.".into()
                },
                mode,
                approval: ApprovalMode::Ask,
            })
            .collect();
    });
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let recorded = events.clone();
    harness.update(move |plait, _, cx| {
        let composer = plait.composers[THREAD].clone();
        cx.subscribe(&composer, move |_, _, event, _| {
            if let ComposerEvent::Submit {
                text,
                revision,
                steer,
            } = event
            {
                recorded
                    .borrow_mut()
                    .push((text.clone(), *revision, *steer));
            }
        })
        .detach();
        plait.sync_composer(cx);
    });
    harness
        .composer(|composer, window, cx| composer.type_text("Sample follow-up draft", window, cx));
    harness.capture(&format!("composer-v3-queued-{suffix}"));
    harness.keystroke("enter");
    harness.draw();
    harness.keystroke("alt-enter");
    assert_eq!(
        events.borrow().len(),
        2,
        "both busy gestures emit without a host"
    );
    assert!(!events.borrow()[0].2);
    assert!(events.borrow()[1].2);
    assert_eq!(events.borrow()[0].0, "Sample follow-up draft");
    assert_eq!(events.borrow()[0].1, events.borrow()[1].1);
    harness.change_snapshot(|snapshot| snapshot.status = ThreadStatus::Interrupted);
    harness.update(|plait, _, cx| plait.sync_composer(cx));
    harness.capture(&format!("composer-v3-held-{suffix}"));
    harness.update(|plait, _, cx| {
        let pending = plait
            .host
            .submissions
            .begin_with(
                THREAD,
                "Sample message with an unconfirmed response".into(),
                Some(ApprovalMode::Ask),
                Some(QueueMode::FollowUp),
                Some(900),
            )
            .unwrap();
        plait
            .host
            .submissions
            .uncertain(&pending.request_id, "Sample disconnected socket".into());
        plait.sync_composer(cx);
    });
    harness.capture(&format!("composer-v3-uncertain-{suffix}"));
    harness.update(|plait, _, cx| {
        let pending = plait
            .host
            .submissions
            .pending(THREAD)
            .unwrap()
            .request_id
            .clone();
        plait.host.submissions.start_send(THREAD, &pending);
        plait.sync_composer(cx);
    });
    harness.capture(&format!("composer-v3-sending-{suffix}"));
    harness.update(|plait, _, _| {
        for queued in &plait.host.snapshot.as_ref().unwrap().queued {
            plait.host.submissions.restore_accepted(
                THREAD.into(),
                plait_core::host_state::PendingSubmit {
                    request_id: queued.request_id.clone(),
                    text: queued.text.clone(),
                    approval: Some(queued.approval),
                    when_busy: Some(queued.mode),
                    draft_revision: Some(100),
                },
            );
        }
    });
    harness.composer(|composer, _, cx| composer.restore_draft(String::new(), 1000, None, cx));
    harness.capture(&format!("composer-v3-edit-queued-{suffix}"));
    harness.change_snapshot(|snapshot| {
        snapshot.status = ThreadStatus::Running;
        let sample = snapshot.queued[0].clone();
        snapshot.queued = (0..16)
            .map(|index| Queued {
                request_id: format!("full-{index}"),
                id: format!("request-full-{index}"),
                ..sample.clone()
            })
            .collect();
    });
    harness.update(|plait, _, cx| plait.sync_composer(cx));
    harness.capture(&format!("composer-v3-full-queue-{suffix}"));
    harness.update(|plait, _, _| plait.host.submissions = Default::default());
    harness.change_snapshot(|snapshot| {
        snapshot.queued.clear();
        snapshot.status = ThreadStatus::Idle;
        snapshot.usage = Some(Usage {
            input: 1000,
            output: 500,
            cache_read: 8500,
            cache_write: 0,
            reasoning: Some(300),
            total: 10000,
            context_used: Some(10000),
            context_window: Some(200000),
        });
    });
    harness.update(|plait, _, cx| plait.sync_composer(cx));
    harness.capture(&format!("composer-v3-context-low-{suffix}"));
    harness
        .change_snapshot(|snapshot| snapshot.usage.as_mut().unwrap().context_used = Some(160000));
    harness.capture(&format!("composer-v3-context-high-{suffix}"));
    harness
        .change_snapshot(|snapshot| snapshot.usage.as_mut().unwrap().context_used = Some(190000));
    harness.capture(&format!("composer-v3-context-full-{suffix}"));
    harness.update(|plait, _, _| plait.thread_controls.usage_open = true);
    harness.capture(&format!("composer-v3-context-details-{suffix}"));
    harness.update(|plait, _, _| plait.thread_controls.usage_open = false);
    harness.update(|plait, _, cx| {
        plait
            .host
            .status
            .as_mut()
            .unwrap()
            .models
            .iter_mut()
            .for_each(|m| m.authenticated = false);
        plait.sync_composer(cx);
    });
    harness.capture(&format!("composer-v3-model-unavailable-{suffix}"));
    harness.update(|plait, _, cx| {
        plait.host.connected = false;
        plait.sync_composer(cx);
    });
    harness.capture(&format!("composer-v3-disconnected-{suffix}"));
    harness.update(|plait, _, cx| {
        plait.session.required = true;
        plait.session.loading = true;
        plait.sync_composer(cx);
    });
    harness.capture(&format!("composer-v3-recovery-{suffix}"));
    harness.update(|plait, _, _| {
        plait.session.required = false;
        plait.session.loading = false;
    });
    harness.connect(repository, false);
}

fn stored_page(
    items: Vec<plait_core::host::HistoryItem>,
    before: Option<&str>,
) -> plait_core::history::Page {
    plait_core::history::prepare_page(
        plait_core::host::HistoryPage {
            items,
            before: before.map(str::to_owned),
        },
        None,
        |_, _| unreachable!("fixture has inline text"),
    )
    .unwrap()
}

fn install_history(plait: &mut Plait, cache: plait_core::history::History) {
    let snapshot = cache.project(plait.host.snapshot.as_ref().unwrap());
    let stored = snapshot.tools.iter().map(|tool| tool.id.clone()).collect();
    plait.formatted = Arc::new(plait_core::markdown::Transcript::update_stored(
        &plait.formatted,
        &snapshot,
        &stored,
    ));
    let previous = plait.history.display.replace(Arc::new(snapshot));
    plait.history.cache = Arc::new(cache);
    plait.history.enabled = true;
    plait.host.status.as_mut().unwrap().protocol_version = 3;
    plait.reconcile_live_list(previous.as_deref());
}

fn stored_message(
    id: &str,
    role: MessageRole,
    text: &str,
    reasoning: &str,
) -> plait_core::host::HistoryItem {
    plait_core::host::HistoryItem::Message {
        id: id.into(),
        role,
        text: text.into(),
        reasoning: reasoning.into(),
        more: vec![],
    }
}

fn history_walk(harness: &mut Harness, repository: &Path, suffix: &str) {
    use super::selection::Owner;
    use plait_core::{history::History, host::HistoryItem};
    harness.connect(repository, false);
    harness.change_snapshot(|snapshot| {
        snapshot.status = ThreadStatus::Idle;
        snapshot.messages = vec![Message {
            id: "h-answer".into(),
            role: MessageRole::Assistant,
            text: "The saved reply is complete.".into(),
            reasoning: String::new(),
        }];
        snapshot.tools.clear();
        snapshot.live_text.clear();
    });
    let items = || {
        vec![
        stored_message("h-user", MessageRole::User, "Review the queue and describe the change.", ""),
        stored_message("h-think", MessageRole::Assistant, "", "Check the queue ownership first.\n\nThen follow the empty-queue transition and verify cancellation."),
        HistoryItem::Tool { id: "h-tool".into(), name: "edit".into(), status: ToolStatus::Done, input: "{\"path\":\"src/queue.rs\",\"old_text\":\"notify();\",\"new_text\":\"notify_when_ready();\"}".into(), output: "Replaced one occurrence.".into(), more: vec![] },
        stored_message("h-answer", MessageRole::Assistant, "The saved reply is complete.\n\nThe queue now notifies only after the next item becomes runnable. The reader keeps its place while earlier history arrives.", ""),
    ]
    };
    harness.update(|plait, _, _| {
        let mut cache = History::default();
        cache.latest(stored_page(items(), None));
        install_history(plait, cache);
        plait.live_scroll.scroll_to(ListOffset::default());
    });
    harness.capture(&format!("history-reasoning-collapsed-{suffix}"));
    harness.update(|plait, _, cx| plait.toggle_reasoning("h-think", cx));
    harness.capture(&format!("history-reasoning-expanded-{suffix}"));
    let position = harness
        .window
        .update(&mut harness.cx, |plait, _, _| {
            plait.text_point(&Owner::Reasoning("h-think".into()), 0, 2)
        })
        .unwrap()
        .expect("reasoning visible");
    harness.pointer(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position,
        click_count: 2,
        ..Default::default()
    }));
    harness.pointer(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position,
        click_count: 2,
        ..Default::default()
    }));
    harness.keystroke("cmd-c");
    harness.update(|plait, _, cx| {
        assert_eq!(plait.selected_text().as_deref(), Some("Check"));
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("Check")
        );
    });
    harness.capture(&format!("history-reasoning-selection-{suffix}"));
    harness.update(|plait, _, _| {
        plait.clear_reading_selection();
        let mut cache = History::default();
        let long = format!("Beginning of the complete reply.\n\n{}\n\nEnd of the complete reply.", "This paragraph remains in stored history even when the live snapshot no longer contains its beginning.\n\n".repeat(65));
        cache.latest(stored_page(vec![stored_message("h-answer", MessageRole::Assistant, &long, "")], None));
        install_history(plait, cache); plait.live_scroll.scroll_to(ListOffset::default());
        assert!(plait.conversation_snapshot().unwrap().messages[0].text.len() > 4096);
    });
    harness.capture(&format!("history-full-message-{suffix}"));
    harness.update(|plait, _, _| {
        let text = "This is the first displayed part of a much larger stored message. Each part is explicit, and copying copies only the part on screen.";
        let item = HistoryItem::Message { id: "h-answer".into(), role: MessageRole::Assistant, text: text.into(), reasoning: String::new(), more: vec![plait_core::host::More { field: plait_core::host::HistoryField::Text, reference: "fixture-part".into(), offset: text.len() as u64, length: 500_000 }] };
        let entry = plait_core::history::Entry::from_inline(item, None).unwrap();
        let mut cache = History::default(); cache.latest(plait_core::history::Page { entries: vec![Arc::new(entry)], before: None });
        install_history(plait, cache);
    });
    harness.capture(&format!("history-text-parts-{suffix}"));

    harness.update(|plait, _, _| {
        plait.history.loading = Some(super::history::Load::Older);
    });
    harness.capture(&format!("history-loading-{suffix}"));
    harness.update(|plait, _, _| {
        plait.history.loading = None;
        let mut cache = History::default();
        cache.latest(stored_page((10..35).map(|i| stored_message(&format!("history-{i}"), if i % 2 == 0 { MessageRole::User } else { MessageRole::Assistant }, &format!("Sample history message {i}. This is stored fixture content, not an agent run."), "")).collect(), Some("earlier")));
        install_history(plait, cache); plait.live_scroll.scroll_to(ListOffset { item_ix: 10, offset_in_item: px(5.) });
        // No host request from the fixture's scroll callback.
        plait.history.loading = Some(super::history::Load::Older);
    });
    harness.capture(&format!("history-scrolled-{suffix}"));
    harness.pointer(PlatformInput::Scroll(ScrollEvent {
        position: point(px(500.), px(200.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(100.))),
        ..Default::default()
    }));
    harness.draw();
    harness.update(|plait, _, _| {
        let before = plait.live_scroll.logical_scroll_top();
        let mut cache = (*plait.history.cache).clone();
        cache.prepend(stored_page(
            (0..10)
                .map(|i| {
                    stored_message(
                        &format!("history-{i}"),
                        MessageRole::Assistant,
                        &format!("Earlier fixture message {i}."),
                        "",
                    )
                })
                .collect(),
            None,
        ));
        install_history(plait, cache);
        let after = plait.live_scroll.logical_scroll_top();
        assert_eq!(after.item_ix, before.item_ix + 10);
        assert_eq!(after.offset_in_item, before.offset_in_item);
        plait.history.loading = None;
    });
    harness.capture(&format!("history-prepended-{suffix}"));
    harness.keystroke("home");
    harness.capture(&format!("history-beginning-{suffix}"));
    harness.update(|plait, _, _| assert_eq!(plait.live_scroll.logical_scroll_top().item_ix, 0));
    harness.keystroke("end");
    harness.update(|plait, _, _| {
        assert_eq!(
            plait.live_scroll.logical_scroll_top().item_ix,
            plait.live_count
        )
    });
    harness.connect(repository, false);
    harness.change_snapshot(|snapshot| { snapshot.live_text.clear(); snapshot.live_reasoning = "Checking how cancellation interacts with the next queued input.\n\nThe writer lease must be released before another run is admitted.".into(); });
    harness.update(|plait, _, _| {
        plait.history.enabled = true;
    });
    harness.capture(&format!("history-live-reasoning-{suffix}"));
    for (name, state, error) in [
        ("waiting", ThreadStatus::Running, None),
        ("finished", ThreadStatus::Idle, None),
        ("stopped", ThreadStatus::Stopped, None),
        ("interrupted", ThreadStatus::Interrupted, None),
        (
            "failed",
            ThreadStatus::Error,
            Some(HostError {
                code: "runtime_error".into(),
                message: "The provider did not complete the request. Try another message.".into(),
            }),
        ),
        (
            "turn-limit",
            ThreadStatus::Error,
            Some(HostError {
                code: "turn_limit".into(),
                message: "Limit".into(),
            }),
        ),
    ] {
        harness.change_snapshot(|snapshot| {
            snapshot.live_reasoning.clear();
            snapshot.live_text.clear();
            snapshot.tools.clear();
            snapshot.status = state;
            snapshot.error = error;
        });
        harness.capture(&format!("history-{name}-{suffix}"));
    }
    harness.change_snapshot(|snapshot| {
        snapshot.status = ThreadStatus::Running;
        snapshot.error = None;
        snapshot.retry = Some(plait_core::host::Retry {
            attempt: 2,
            at: 5000,
        });
    });
    harness.update(|plait, _, _| {
        plait.history.now = 2000;
    });
    harness.capture(&format!("history-retry-{suffix}"));
}

#[core::prelude::v1::test]
fn delayed_history_cannot_land_after_a_thread_switch() {
    if !isolated("delayed_history_cannot_land_after_a_thread_switch") {
        return;
    }
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
    };
    let scratch = std::env::temp_dir().join(format!("plait-history-guard-{}", std::process::id()));
    let repository = fixture_repository(&scratch);
    let mut harness = Harness::new(&repository, None, 900., 600.);
    harness.connect(&repository, false);
    let store = PathBuf::from(std::env::var_os("PLAIT_HOST_STORE").unwrap());
    std::fs::create_dir_all(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = store.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(socket.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(line.contains("history"));
        std::thread::sleep(Duration::from_millis(10));
        socket.write_all(b"{\"ok\":true,\"result\":{\"items\":[{\"type\":\"message\",\"id\":\"old\",\"role\":\"assistant\",\"text\":\"Wrong thread\",\"reasoning\":\"\"}],\"before\":null}}\n").unwrap();
    });
    harness
        .window
        .update(&mut harness.cx, |plait, window, cx| {
            plait.history.enabled = true;
            plait.load_history(super::history::Load::Latest, window, cx);
            // The request was scheduled for this thread; switch before its reply.
            plait.host.select(Some("other-thread".into()));
            plait.history = Default::default();
        })
        .unwrap();
    harness.settle();
    server.join().unwrap();
    for _ in 0..10 {
        harness.settle();
        std::thread::sleep(Duration::from_millis(5));
    }
    harness.update(|plait, _, _| {
        assert!(!plait.history.cache.loaded);
        assert!(plait.history.display.is_none());
    });
    drop(harness);
    std::fs::remove_file(socket).unwrap();
    std::fs::remove_dir_all(scratch).unwrap();
}
