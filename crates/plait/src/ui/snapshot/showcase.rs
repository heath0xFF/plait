//! README screenshots: the real views over a small sample project.
//!
//! Every thread, model reply, and tool result here is written by hand, and
//! the README says so under each image. The Git status, diff, and file
//! contents are real reads of a disposable repository built below.
//!
//! ```sh
//! bash scripts/readme-screenshots.sh
//! ```

use super::{Harness, git, isolated};
use gpui::AppContext as _;
use plait_core::{
    host::{
        Approval, ApprovalKind, ApprovalMode, Message, MessageRole, Model, ModelChoice, Snapshot,
        Status, ThinkingLevel, ThreadStatus, ThreadSummary, Tool, ToolStatus, Usage,
    },
    model::DiffMode,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const THREAD: &str = "readme-retry";
/// Shown in place of this computer's own host name.
const MACHINE: &str = "studio";
/// Window size in points; images are twice this.
const SIZE: (f32, f32) = (1440., 900.);

const CLIENT_BEFORE: &str = r#"use crate::station::Station;
use reqwest::{Client as Http, StatusCode};
use serde::Deserialize;

/// Hourly readings for one weather station.
#[derive(Debug, Deserialize)]
pub struct Forecast {
    pub station: String,
    pub hours: Vec<Hour>,
}

#[derive(Debug, Deserialize)]
pub struct Hour {
    pub time: String,
    pub temperature: f32,
    pub precipitation: f32,
}

pub struct Client {
    http: Http,
    base: String,
}

impl Client {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            http: Http::new(),
            base: base.into(),
        }
    }

    pub async fn forecast(&self, station: &Station) -> Result<Forecast, reqwest::Error> {
        let url = format!("{}/stations/{}/forecast", self.base, station.id);
        let response = self.http.get(&url).send().await?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            log::warn!("rate limited fetching {url}");
        }
        response.error_for_status()?.json().await
    }
}
"#;

const RETRY: &str = r#"//! Retries for rate-limited requests.

use reqwest::{Response, StatusCode, header::RETRY_AFTER};
use std::{future::Future, time::Duration};

/// How many times to retry, and how long to wait between attempts.
#[derive(Clone, Debug)]
pub struct Policy {
    pub attempts: u32,
    pub base: Duration,
    pub ceiling: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            attempts: 4,
            base: Duration::from_millis(250),
            ceiling: Duration::from_secs(8),
        }
    }
}

impl Policy {
    /// The wait before retry `attempt`: doubling from `base`, capped at `ceiling`.
    pub fn delay(&self, attempt: u32) -> Duration {
        self.base.saturating_mul(1 << attempt.min(16)).min(self.ceiling)
    }
}

/// Send, and on 429 wait and send again, honoring the server's Retry-After.
pub async fn with_backoff<F, Fut>(policy: &Policy, mut send: F) -> reqwest::Result<Response>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = reqwest::Result<Response>>,
{
    let mut attempt = 0;
    loop {
        let response = send().await?;
        if response.status() != StatusCode::TOO_MANY_REQUESTS || attempt == policy.attempts {
            return Ok(response);
        }
        let delay = retry_after(&response).unwrap_or_else(|| policy.delay(attempt));
        tokio::time::sleep(delay.min(policy.ceiling)).await;
        attempt += 1;
    }
}

/// Retry-After in seconds. The HTTP-date form is rare here and falls back to backoff.
fn retry_after(response: &Response) -> Option<Duration> {
    let seconds = response.headers().get(RETRY_AFTER)?.to_str().ok()?;
    seconds.trim().parse().ok().map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_up_to_the_ceiling() {
        let policy = Policy::default();
        assert_eq!(policy.delay(0), Duration::from_millis(250));
        assert_eq!(policy.delay(2), Duration::from_secs(1));
        assert_eq!(policy.delay(9), policy.ceiling);
    }
}
"#;

/// A small Rust crate with one finished change, staged.
fn sample_repository(root: &Path) -> PathBuf {
    let repository = root.join("code/forecast");
    std::fs::create_dir_all(repository.join("src")).unwrap();
    // The second project in the sidebar; present, so it is not shown as missing.
    std::fs::create_dir_all(root.join("code/field-notes")).unwrap();
    let write = |path: &str, text: &str| std::fs::write(repository.join(path), text).unwrap();
    git(&repository, &["init", "-q", "-b", "main"]);
    git(&repository, &["config", "user.name", "Sample Author"]);
    git(
        &repository,
        &["config", "user.email", "sample@example.invalid"],
    );
    git(&repository, &["config", "commit.gpgsign", "false"]);
    write(
        "Cargo.toml",
        "[package]\nname = \"forecast\"\nversion = \"0.4.2\"\nedition = \"2024\"\n\n[dependencies]\nlog = \"0.4\"\nreqwest = { version = \"0.12\", features = [\"json\"] }\nserde = { version = \"1\", features = [\"derive\"] }\ntokio = { version = \"1\", features = [\"time\"] }\n",
    );
    write(
        "README.md",
        "# forecast\n\nHourly forecasts for weather stations.\n",
    );
    write("src/lib.rs", "pub mod client;\npub mod station;\n");
    write(
        "src/station.rs",
        "/// A weather station, by its network ID.\n#[derive(Clone, Debug)]\npub struct Station {\n    pub id: String,\n    pub name: String,\n}\n",
    );
    write("src/client.rs", CLIENT_BEFORE);
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-qm", "Fetch hourly forecasts"]);
    // The change a thread made, not yet committed.
    write(
        "src/lib.rs",
        "pub mod client;\npub mod retry;\npub mod station;\n",
    );
    write("src/retry.rs", RETRY);
    write("src/client.rs", &client_after());
    write(
        "README.md",
        "# forecast\n\nHourly forecasts for weather stations. Rate-limited requests\nare retried with exponential backoff, honoring `Retry-After`.\n",
    );
    // Staged and ready to commit, as after Stage all.
    git(&repository, &["add", "."]);
    repository.canonicalize().unwrap()
}

fn client_after() -> String {
    CLIENT_BEFORE
        .replace(
            "use crate::station::Station;\nuse reqwest::{Client as Http, StatusCode};",
            "use crate::{retry, station::Station};\nuse reqwest::Client as Http;",
        )
        .replace(
            "    base: String,\n}",
            "    base: String,\n    retry: retry::Policy,\n}",
        )
        .replace(
            "            base: base.into(),\n",
            "            base: base.into(),\n            retry: retry::Policy::default(),\n",
        )
        .replace(
            "        let response = self.http.get(&url).send().await?;\n        if response.status() == StatusCode::TOO_MANY_REQUESTS {\n            log::warn!(\"rate limited fetching {url}\");\n        }\n",
            "        let response = retry::with_backoff(&self.retry, || self.http.get(&url).send()).await?;\n",
        )
}

fn choice() -> ModelChoice {
    ModelChoice {
        provider: "anthropic".into(),
        model_id: "claude-opus-5-5".into(),
    }
}

fn status() -> Status {
    let model = |model_id: &str, name: &str| Model {
        provider: "anthropic".into(),
        model_id: model_id.into(),
        name: name.into(),
        authenticated: true,
        reasoning: Some(true),
        thinking_levels: Some(vec![
            ThinkingLevel::Off,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ]),
        context_window: Some(200_000),
        ..Default::default()
    };
    Status {
        protocol_version: 3,
        default_model: Some(choice()),
        models: vec![
            model("claude-opus-5-5", "Claude Opus 5.5"),
            model("claude-sonnet-5-5", "Claude Sonnet 5.5"),
        ],
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn message(id: &str, role: MessageRole, text: &str) -> Message {
    Message {
        id: id.into(),
        role,
        text: text.into(),
        ..Default::default()
    }
}

fn tool(id: &str, name: &str, input: String, output: &str, after: &str) -> Tool {
    Tool {
        id: id.into(),
        name: name.into(),
        status: ToolStatus::Done,
        input,
        output: output.into(),
        after: Some(Some(after.into())),
    }
}

fn json(value: serde_json::Value) -> String {
    value.to_string()
}

/// The retry thread, finished: plan, reads, edits, a test run, and a summary.
fn finished_thread(checkout: &Path) -> Snapshot {
    let messages = vec![
        message(
            "u1",
            MessageRole::User,
            "Forecast requests fail outright when the API rate-limits us. Retry 429s with exponential backoff, honor Retry-After, and add a test.",
        ),
        message(
            "a1",
            MessageRole::Assistant,
            "I'll read the client first to see where the request is sent.",
        ),
        message(
            "a2",
            MessageRole::Assistant,
            "`Client::forecast` logs a 429 and then fails in `error_for_status`. I'll put the retry loop in its own module so the policy is testable without a server:\n\n```rust\npub async fn with_backoff<F, Fut>(policy: &Policy, mut send: F) -> reqwest::Result<Response>\n```\n\nIt waits for `Retry-After` when the server sends one, otherwise doubles from 250 ms up to 8 s, and gives up after 4 retries.",
        ),
        message(
            "a3",
            MessageRole::Assistant,
            "Done. Rate-limited forecast requests are now retried.\n\n- **`src/retry.rs`** — `Policy` and `with_backoff`, with a test for the delay schedule\n- **`src/client.rs`** — `forecast` sends through `with_backoff`; the warning log is gone\n- **`README.md`** — one line on the retry behavior\n\nAll 9 tests pass. Nothing is committed; review it in **Changes**.",
        ),
    ];
    let tools = vec![
        tool(
            "t1",
            "read",
            json(serde_json::json!({"path": "src/client.rs"})),
            CLIENT_BEFORE,
            "a1",
        ),
        tool(
            "t2",
            "write",
            json(serde_json::json!({"path": "src/retry.rs", "content": RETRY})),
            "Created src/retry.rs.",
            "a2",
        ),
        tool(
            "t3",
            "edit",
            json(serde_json::json!({
                "path": "src/client.rs",
                "old_text": "        let response = self.http.get(&url).send().await?;\n        if response.status() == StatusCode::TOO_MANY_REQUESTS {\n            log::warn!(\"rate limited fetching {url}\");\n        }",
                "new_text": "        let response = retry::with_backoff(&self.retry, || self.http.get(&url).send()).await?;",
            })),
            "Replaced 1 occurrence.",
            "a2",
        ),
        tool(
            "t4",
            "edit",
            json(serde_json::json!({
                "path": "src/lib.rs",
                "old_text": "pub mod client;\n",
                "new_text": "pub mod client;\npub mod retry;\n",
            })),
            "Replaced 1 occurrence.",
            "a2",
        ),
        tool(
            "t5",
            "command",
            json(serde_json::json!({"command": "cargo test --offline"})),
            "   Compiling forecast v0.4.2\n    Finished `test` profile in 3.84s\n     Running unittests src/lib.rs\n\nrunning 9 tests\ntest retry::tests::delays_double_up_to_the_ceiling ... ok\ntest client::tests::retries_until_the_limit ... ok\ntest client::tests::honors_retry_after ... ok\n\ntest result: ok. 9 passed; 0 failed; 0 ignored",
            "a2",
        ),
    ];
    Snapshot {
        id: THREAD.into(),
        checkout: checkout.to_owned(),
        model: choice(),
        status: ThreadStatus::Idle,
        messages,
        tools,
        approval_mode: Some(ApprovalMode::Ask),
        title: Some("Retry rate-limited forecast requests".into()),
        created_at: Some(now() - 900_000),
        updated_at: Some(now() - 60_000),
        archived: Some(false),
        thinking_level: Some(ThinkingLevel::Medium),
        usage: Some(Usage {
            input: 41_200,
            output: 3_100,
            cache_read: 36_800,
            cache_write: 4_400,
            reasoning: None,
            total: 85_500,
            context_used: Some(48_600),
            context_window: Some(200_000),
        }),
        ..Default::default()
    }
}

/// The same thread earlier, waiting for approval to run the tests.
fn waiting_thread(checkout: &Path) -> Snapshot {
    let mut snapshot = finished_thread(checkout);
    snapshot.messages.truncate(3);
    snapshot.tools[4].status = ToolStatus::Running;
    snapshot.tools[4].output.clear();
    snapshot.status = ThreadStatus::AwaitingApproval;
    snapshot.updated_at = Some(now());
    snapshot.pending_approval = Some(Approval {
        id: "readme-approval".into(),
        tool_id: "t5".into(),
        kind: ApprovalKind::Command,
        // The host's own wording for a sandboxed command.
        summary: "Run sandboxed command".into(),
        details: json(serde_json::json!({"command": "cargo test --offline"})),
        dirty_paths: Vec::new(),
        risk: "Only this exact call is approved. Dirty and ignored paths are protected. Commands write only private scratch and a private per-thread build cache; no external network or checkout/Git writes. Tests may use scratch-local Git repositories and Unix sockets. Read tools and commands can read ignored checkout files and send their contents to the provider.".into(),
    });
    snapshot
}

/// The sidebar: this thread among others in two projects.
fn threads(checkout: &Path, current: ThreadStatus) -> Vec<ThreadSummary> {
    let site = checkout.with_file_name("field-notes");
    let now = now();
    let summary = |id: &str, checkout: &Path, title: &str, status, minutes: u64| ThreadSummary {
        id: id.into(),
        checkout: checkout.to_owned(),
        model: choice(),
        status,
        title: Some(title.into()),
        created_at: Some(now - 86_400_000),
        updated_at: Some(now - minutes * 60_000),
        archived: Some(false),
        pending_approval: Some(status == ThreadStatus::AwaitingApproval),
        harness: None,
    };
    vec![
        summary(
            THREAD,
            checkout,
            "Retry rate-limited forecast requests",
            current,
            1,
        ),
        summary(
            "readme-cache",
            &site,
            "Add a search page",
            ThreadStatus::Running,
            0,
        ),
        summary(
            "readme-units",
            checkout,
            "Add Fahrenheit output",
            ThreadStatus::Idle,
            42,
        ),
        summary(
            "readme-aggregate",
            checkout,
            "Explain the hourly aggregation",
            ThreadStatus::Idle,
            180,
        ),
        summary(
            "readme-links",
            &site,
            "Fix broken links in the install guide",
            ThreadStatus::AwaitingApproval,
            6,
        ),
        summary(
            "readme-feed",
            &site,
            "Generate an Atom feed",
            ThreadStatus::Idle,
            300,
        ),
    ]
}

/// Present a connected host with `snapshot` selected. No socket is opened.
fn stage(harness: &mut Harness, checkout: &Path, snapshot: Snapshot) {
    let checkout = checkout.to_owned();
    harness.update(move |plait, window, cx| {
        plait.host.connecting = false;
        plait.host.connected = true;
        plait.host.error = None;
        plait.host.status = Some(status());
        plait.host.settings_model = Some(choice());
        plait.host.new_model = Some(choice());
        plait.host.threads = threads(&checkout, snapshot.status);
        plait.host.selected = Some(THREAD.into());
        plait.history = Default::default();
        plait.host.snapshot = Some(Arc::new(snapshot));
        plait.formatted = Arc::new(plait_core::markdown::Transcript::update(
            &plait.formatted,
            plait.host.snapshot.as_ref().unwrap(),
        ));
        plait.settings_open = false;
        plait.new_thread_open = false;
        plait.sidebar = true;
        plait.tab = crate::ui::Workspace::Conversation;
        if !plait.composers.contains_key(THREAD) {
            let composer = cx.new(|cx| crate::ui::composer::Composer::new(window, cx));
            plait.composers.insert(THREAD.into(), composer);
        }
        plait.state.projects[0].machines[0].name = MACHINE.into();
        // In memory only, as every scene's preferences are.
        plait.preferences.value.ask_before_actions = true;
        plait.preferences.value.navigation = Default::default();
        plait
            .preferences
            .value
            .navigation
            .observe(&plait.host.threads, None, true);
        plait.rebuild_projects(cx);
        for project in &plait.state.projects {
            plait
                .navigation
                .available
                .insert(project.checkout.clone(), true);
        }
        plait.refresh_live_list(None);
        plait.sync_composer(cx);
    });
}

fn walk(output: &Path) {
    let home = PathBuf::from(std::env::var_os("HOME").expect("scene home"));
    let repository = sample_repository(&home);
    let mut harness = Harness::new(&repository, Some(output), SIZE.0, SIZE.1);
    let light = |harness: &mut Harness, light: bool| {
        harness.update(move |plait, _, cx| {
            plait.themes.forced = Some(light);
            plait.apply_theme(cx);
        })
    };

    stage(&mut harness, &repository, finished_thread(&repository));
    harness.capture("conversation");
    light(&mut harness, true);
    harness.capture("conversation-light");
    light(&mut harness, false);

    stage(&mut harness, &repository, waiting_thread(&repository));
    harness.capture("approval");

    stage(&mut harness, &repository, finished_thread(&repository));
    harness.update(|plait, window, cx| {
        let index = plait
            .state
            .repository
            .snapshot
            .ready()
            .expect("Git status")
            .changes
            .iter()
            .position(|file| file.path.ends_with("client.rs"))
            .expect("client.rs is changed");
        plait.select_file(index, DiffMode::Staged, window, cx);
        plait.commit_message.update(cx, |field, cx| {
            field.set_text("Retry rate-limited forecast requests", cx)
        });
    });
    harness.capture("changes");

    harness.update(|plait, _, cx| plait.open_file("src/retry.rs".into(), Some(33), cx));
    harness.update(|plait, window, cx| {
        plait.open_find(window, cx);
        let field = plait.file_viewer.find.field.clone().unwrap();
        field.update(cx, |field, cx| field.set_text("delay", cx));
    });
    harness.capture("files");
}

#[core::prelude::v1::test]
#[ignore = "writes README images; run scripts/readme-screenshots.sh"]
fn readme_screenshots() {
    if !isolated("showcase::readme_screenshots") {
        return;
    }
    let output = std::env::var_os("PLAIT_SNAPSHOT_DIR")
        .map(PathBuf::from)
        .expect("Set PLAIT_SNAPSHOT_DIR to an output directory.");
    std::fs::create_dir_all(&output).unwrap();
    walk(&output);
}
