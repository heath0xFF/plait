# Architecture

## Scope

The M2 local preview uses a native desktop and an independent host. The workspace
contains `crates/plait` (native desktop), `crates/plait-core` (headless models,
client state, local host client, and read-only Git), and `crates/plait-host`
(local daemon and filesystem/process helpers). `runtime/` contains the pinned
Pi Durable TypeScript sidecar. There is no remote execution,
embedded terminal, editor, Git-write interface, or worktree manager.

This remains an early local preview. Linux command execution and remote
transport are not implemented. [Verification](VERIFICATION.md) records versioned
Mac installation, sandbox, and recovery evidence, with the remaining gaps.

```text
crates/plait/src/
main.rs                 native app lifecycle, window, startup
ui/                     GPUI layout, actions, focus, lists, theme
  mod.rs                UI state and background-task dispatch
  navigation.rs         Project → Machine → Thread
  conversation.rs       no-thread state and the Git inspector
  live_conversation.rs  real host messages, ordered tools, and approvals
  composer.rs           native input, IME, clipboard, undo, submit/stop events
  host.rs               background host requests and event subscriptions
  session.rs            desktop restore/save coordination and submission guards
  markdown.rs           native conversation blocks and copy controls
  settings.rs           model choice, auth status, defaults, and reconnect
  changes.rs            real local status and diff views
  theme.rs              tokens, original controls, embedded glyphs
  snapshot.rs           Linux-only headless layout checks (tests only)

crates/plait-core/src/
host.rs                 bounded Unix-socket framing and requests; background only
host/watch.rs           cancellable reader and bounded async snapshot delivery
host_state.rs           host selection, request identity, and stale-result guards
transcript.rs           stable row identity and incremental list-change plans
local_socket.rs         owner-only paths and OS peer-identity checks
state.rs                selection, collapse state, request queue, cache
model.rs                plain Rust data, no GPUI
machine.rs              this machine's display name
preferences.rs          desktop-only preferences and Cmd-number order
session.rs              private desktop journal, identity checks, writer lock
markdown.rs             plain text blocks and style ranges, parsed off render
git.rs                  synchronous, read-only Git adapter; background only
```

The desktop imports `plait_core`; the core has no GPUI dependencies. Core and
host Rust tests can run on Linux without compiling the desktop. The default
Cargo member remains the macOS desktop. The macOS workspace checks still cover
all crates, including the native composer. See CONTRIBUTING.md for both commands.
The core uses Unix filesystem and socket APIs; it is not a Windows port.
Filesystem, socket, and Git operations run on the background executor.

A Linux build does not enable agent command execution. The runtime still rejects
non-macOS commands, and the Rust command guardian still requires Seatbelt. Linux
containment and cancellation need a separate safety review. Local peer identity
is checked with `getpeereid` on macOS and `SO_PEERCRED` on Linux.
Both platforms use the same local protocol (version 1, with opt-in versions 2
and 3) and the same ownership boundaries.

The Linux sandbox mechanism is an open decision. An unprivileged namespace
probe succeeded, but a read-only bind mount still permitted access to an outside
pathname Unix socket. See [the evidence, alternatives, and enablement gates](LINUX-SANDBOX.md).
Landlock ABI 9 is one option, not a fixed product requirement.

## Local agent boundary

The Rust daemon owns the sidecar, the owner-only Unix socket, and the store
writer lock. The desktop is a client, not the owner of an agent run. Window
closure drops subscriptions; it does not stop execution. See [HOST.md](HOST.md)
for the exact protocol, storage location, launch commands, and safety policy.

The host owns model discovery, existing Pi authentication, thread persistence,
and the default model. The desktop receives availability, not credentials.
Each thread keeps its exact provider/model pair and thinking level until a
client changes them between runs ([protocol version 3](HOST.md#model-and-thinking-level)).
Saving a default does not change an existing thread.

The host also owns each thread's title, timestamps, archive flag, and the queue
of inputs waiting behind a run. A snapshot is the bounded live tail of a thread.
The complete conversation is read on demand, in pages, from the thread's saved
conversation store; an idle thread's store is opened read-only for that, so a
history read cannot recover or run anything. The desktop core can speak these
operations; the desktop views do not use them yet.

The desktop uses bounded snapshot subscriptions. Runtime state changes signal
the host through a private inherited Unix socket, separate from request/reply
stdio. The host wakes matching watchers and reads the latest snapshot; idle
watches do not poll the sidecar. IDs coalesce under backpressure, and each client
has at most one pending wakeup. Watches register before the initial read and
wait on both the wakeup and client disconnect. Channel EOF replaces the host's
periodic child-process check. Client protocol version 1 and durable schemas are
unchanged; the internal channel ships with the host/runtime update unit.

A thread-list subscription uses the same wakeup path with one reserved
notification name, sent only when a thread's summary changes. The host repeats
the bounded listing and sends the summaries that differ, so streamed text costs
list watchers nothing.

The desktop owns a cancellation lease for its selected subscription. Selection,
reconnect, and view release drop that lease; socket shutdown runs on the background
executor and does not send Stop. A dedicated reader keeps idle socket waits off
GPUI's shared worker pool. A one-slot async inbox delivers the newest snapshot
without periodic UI wakeups or an unbounded event queue. Thread ID and
checkout are checked before coalescing, so an invalid frame cannot be hidden by
a later valid one. The model is not pinned: it can change between runs. The
thread-list reader shares this machinery and delivers the merged list. Initial replies and
partial frames retain absolute deadlines; an idle established watch has no timer.

A repository epoch, watch epoch, and thread ID guard each async result. An uncertain submission keeps its request
ID for an explicit retry. Reconnect loads state; it does not submit a new task.
The composer keeps its draft until acceptance and clears it only if its edit
revision still matches the submitted draft. Each thread has its own composer.

Each run is submitted with an approval mode. In `ask` mode every file change and
command needs a one-call approval with full arguments; in `auto` mode, the
desktop default with a version 2 host, they run without a prompt. File helpers own checkout writes; command execution reads the checkout and uses
private scratch space for build output. Stop revokes pending approvals and
cancels work. Interrupted work must not replay automatically after host failure.
These boundaries need the tests listed in HOST.md before valuable-repository use.

### Run authorization

`runtime/src/authorization.ts` owns a run's activation and revocation. Each new
admission gets a fresh authorization object; a saved thread does not carry
permission into another turn. Recovery keeps it inactive until old tasks are
stopped. Stop, completion, and host shutdown revoke it permanently.

`runtime/src/approval.ts` implements both policies. The interactive policy owns
the exact-call prompt, one-use answers, cancellation, and snapshot updates. The
automatic policy grants an active run's calls without a prompt. The tool
registry asks the run for authorization without owning the approval lifecycle.
Argument limits, dirty-path protection, checked file helpers, and command
containment remain mandatory in `tools.ts` and its helpers. Authorization is
checked again before an effect starts, so a late Allow cannot override Stop.

The submitting desktop chooses the mode for one run (see
[protocol version 2](HOST.md#protocol-version-2)). There is no persisted grant.
A version 3 input [queued behind a run](HOST.md#messages-during-a-run) keeps the
mode it was submitted with and, when it starts, is admitted like any submission
with a fresh authorization object. A steering input joins the active run and
uses that run's authorization. Stop clears the queue; a failed run or a restart
holds it. A model request that fails transiently is retried with backoff; tool
calls are marked unsafe to replay and are never repeated. Tests include a clearly
labeled permissive policy fixture to check that authorization cannot bypass file
protection or command containment. On Linux, the command check proves denial,
not a working sandbox. The automation requirement remains tracked in
[issue #13](https://github.com/heath0xFF/plait/issues/13).

## Portable runtime and development

The daemon uses the core's low-level local-socket checks, not its client state.
Both accepted connections and client connections verify the peer's effective
user ID. Socket and parent-directory ownership/modes are also checked before
clients connect. The wire protocol is unchanged.

`runtime/src/safety.ts` owns common checkout, file, and lease checks.
`command/platform.ts` selects only an implemented platform policy;
`command/macos.ts` owns Seatbelt and Apple build-environment setup.
`command.ts` owns scratch lifetime and root validation, while
`command/supervisor.ts` owns bounded output, cancellation, timeout, and guardian
cleanup. Spawn failures clear their timer/listener. Final output is drained, with
a one-second drain bound after guardian exit so an unsupported detached process
cannot keep a subscription open. Each output pipe decodes UTF-8 across read
boundaries. `runtime/src/text.ts` keeps text cuts on whole Unicode code points
within the existing UTF-16 budgets. Accepted-input previews, transcript fields,
and tool output use these helpers; the original input journal is not clipped.

`bash scripts/check.sh` is the common development entry point. Runtime test
selection is explicit: common daemon/file tests on both systems, real sandbox
tests on macOS, and command-denial tests on Linux. The full sandbox gate fails
on Linux instead of returning a partial acceptance result. Scripted guardian
tests check process lifecycle only; they do not establish containment.

## Git data flow

1. A UI action changes application state.
2. `RepositoryState::next_job` returns at most one pending job.
3. The UI dispatcher runs `Job::run` on GPUI's background executor.
4. The result returns through a weak entity handle.
5. State checks the repository epoch and selected diff key before displaying it.
6. `cx.notify()` schedules an update. There is no redraw loop or repository poll.

Only one Git job runs at a time. Rapid selections replace the pending job, not
the running job. A repository change or refresh increments the epoch. An old
result cannot overwrite the new repository. A stale file result can enter the
current snapshot's cache, but cannot replace another selected file.

Diffs are loaded on demand. Up to eight diffs are cached per status snapshot.
Refresh clears that cache. Threads within a checkout share its Git state.
Git reads do not start SSH or network connections. Remote transport is not
implemented.

## Git boundary

Public operations are `status(path)` and `diff(root, change, mode)`. Both return
data or a user-facing error. There are no write operations.

- Git CLI, not libgit2. Use porcelain v1 with NUL-delimited paths.
- Keep paths as `PathBuf`/OS bytes. Lossy text is for display only.
- Use literal pathspecs and an argument array, never shell interpolation.
- Separate index and worktree diffs. No combined “all changes” view that hides
  partially staged files.
- Disable optional index locks, fsmonitor, untracked cache, external diff, and
  textconv. Read configuration **key names only** and disable all configured
  clean/process filter drivers for each command. Git can otherwise execute a
  filter even during a read-only diff. No pager, prompt, hook invocation, or
  network command. Ignore dirty submodule contents; show gitlink changes only.
- Ignore inherited repository-location environment variables.
- Reap subprocesses. Bound each subprocess to 10 seconds, status/config keys to
  8 MiB, diff output to 512 KiB, and stderr to 8 KiB. Pipe readers drain
  concurrently. A job includes multiple subprocesses, so its total deadline can
  be longer than 10 seconds.
- Reject oversized status rather than showing an incomplete file list.
- Limit displayed patches to 10,000 lines and 2,000 characters per line.
  A clipped preview is labeled. Unified source line numbers are parsed in the
  background, as is the widest-line index used for horizontal scrolling.
- Untracked regular files get a bounded text preview. Binary/non-UTF-8 content,
  symlinks, directories, and special files are not followed as text.
- Untracked file opens walk directory descriptors with `openat`, `O_NOFOLLOW`,
  and `O_NONBLOCK`. This prevents a parent symlink swap from escaping the
  checkout or a special file from blocking a preview. The small `libc` dependency
  supplies platform constants and calls; it is already used by GPUI.

A status snapshot and its later diff are not a filesystem transaction. Another
program can change the checkout between reads. Refresh is explicit. No data is
staged, discarded, committed, or repaired in response to a read error.

## macOS installation boundary

The client and host are independent update units. A combined installer carries
both component packages; either package can also be installed alone. The host
unit includes Rust, pinned Node, and locked production runtime dependencies.
It runs as a per-user LaunchAgent, never as a root agent executor. The agent uses
standard service scheduling because it serves desktop requests, not discretionary
background work.

Host releases use content-derived directory names and distinct package receipts.
A shared receipt would let PackageKit remove the old code during an update.
The LaunchAgent label remains stable. The loaded job and sidecar
keep their original paths while a new package stages the next release. An
identical host reinstall is rejected before payload writes. Activation requires
an explicit session restart after work finishes. Client updates do not control
the host process. User state and credentials are not package payloads.

Per-user installs update themselves. `plait_core::update` reads GitHub
Releases, verifies an Ed25519-signed `update.json`, and checks each download's
signed digest. The app bundle carries its host as a data archive; an update
unpacks it as a new immutable release under the user's Library, rewrites the
per-user LaunchAgent, and restarts the host only when the store has no active or
waiting run. Releases other than the active one are then removed.
`plait-host update` applies the same steps on machines without the app.

This changes launch configuration, not the frozen protocol or durable thread
schema. The client accepts protocol versions 1 through 3. Future protocol
and storage changes need a separate compatibility review. Developer ID signing
and notarization are not implemented. See [Mac installation](MACOS.md) for
paths and operational limits.

## UI and performance

Use GPUI's native macOS platform, Metal renderer, system sans-serif font, and
Menlo for code. SVG assets are embedded strings, not runtime file reads. The
app shows no in-window logo; category glyphs use GPUI's tinted SVG path. Brand
files live in `assets/branding`. The development bundle and icon build are
described in [MACOS.md](MACOS.md).

### Desktop preferences

Model defaults and credentials belong to the host. Choices that belong to one
desktop installation live in `plait_core::preferences`, saved as JSON at
`~/Library/Application Support/Plait/desktop.json` on macOS
(`$XDG_CONFIG_HOME/plait/desktop.json` or `~/.config/plait/desktop.json`
elsewhere). Today it holds one setting: what Cmd-1 to Cmd-9 select.

The file is read and written on the background executor. A missing file means
defaults. A malformed or oversized file is reported in Settings and left in
place; defaults apply. Saves replace the file atomically, and Settings says
"Saved" only after the write succeeds. Only the newest save reports a result.
Unknown fields are ignored so a newer client's file still loads.

Cmd-1 to Cmd-9 count real host threads in sidebar order on the local machine.
A machine or project selects its first thread; a row with no thread does nothing.
Jumping expands collapsed parents.

### Desktop recovery

`plait-core::session` owns a private, versioned desktop journal. Its directory is
next to `desktop.json` under `session/`; `PLAIT_DESKTOP_STATE_DIR` can select an
absolute isolated directory for development. A lifetime writer lock prevents two
clients from overwriting each other's drafts. Malformed, oversized, future-schema,
or wrong-host state is left intact and blocks submission, not silently reset.

The final journal owner explicitly releases its writer lock, so a descriptor
inherited by a concurrent fork does not keep a released journal locked. Host and
command leases have different lifetimes and are not released this way.

The UI owns selection and composer changes. It queues immutable memory snapshots
and coalesces disk saves after 250 ms. Background writes are serialized and atomic,
with file and directory synchronization. Closing a window waits for its save;
normal app shutdown also flushes the journal. Force quit and power loss cannot
guarantee the last unsaved keystroke. No filesystem work runs in render.

The journal keeps the last checkout/thread, readable titles, draft text/edit
revisions, and unconfirmed submission IDs. It does not read or copy provider
credential stores. Draft text is private local JSON, not application-encrypted.
A submission's exact ID and text must be durable before the socket request starts.
Reopening never submits automatically. Explicit retry uses that same ID, and a
matching accepted-input event can resolve it. The composer revision guard survives
restart, so an acknowledgement cannot erase a subsequently edited draft. The host
protocol and durable conversation schema are unchanged.

### Native conversation text

`plait-core::markdown` projects CommonMark into plain blocks and style ranges on
the background executor. `pulldown-cmark` is pinned to 0.13.0 with default features
disabled; its MIT-licensed source and event API were checked at that version.
No HTML renderer or web view is used. HTML, image destinations, and links remain
text: no image fetch, script execution, or automatic URL opening. Unchanged
messages retain their parsed projection while the live answer changes. Excessive
block counts fall back to the original text rather than dropping content.

The native view draws headings, paragraphs, lists, quotes, emphasis, inline code,
and horizontally scrollable fenced code. Copy controls copy the message source,
code block, or tool details. These are still bounded host previews, not paginated
history. Git refreshes after an observed busy-to-complete transition.

Conversation events use GPUI's variable-height `list`. User messages render as
right-aligned bubbles; agent replies remain open text. Tool details start
collapsed. Disclosure state belongs to the UI and clears on thread selection.
Live transcript changes are planned in `plait-core::transcript`, without GPUI.
Ordered stable IDs retain existing rows, including after history-tail clipping.
Structural changes use `splice`; changed content uses `remeasure_items` so the
scroll anchor and measured unchanged rows survive streaming. Duplicate IDs or
a changed thread identity reset the list rather than reuse the wrong row.
Live tool disclosure and element IDs follow tool/message IDs, not row positions.
Expanding a live tool remeasures only that row. Changed files and diff
lines use `uniform_list`; only visible rows are built. Cached data uses `Arc`
rather than cloning large strings on every render. Long diff lines scroll
horizontally.

One 44-point header carries the thread title, its state, and the Conversation /
Changes switch; there is no separate tab strip. Navigation, changed-file, and
model rows are 28 to 36 points and one line. The composer is one raised box; the
Project, Machine, branch, and saved model sit on a context line beneath it.
`ui/theme.rs` owns the tokens and the small original controls. A control's tone
(ghost, selected, filled, primary) is chosen when it is built, because GPUI
accepts one hover style per element and reports a second one only at draw time.

The sidebar is 272 points on wide windows, 240 on narrow windows. Below 1150
points, the conversation's 280-point Git inspector is removed; Git remains available in
the Changes tab. The minimum window is 900 × 600. Cmd-B hides the sidebar.

Controls have focus rings, tab stops, accessible names, and semantic roles.
Thread state has both a color and a text label.
No decorative animation is used.

The app records root-view construction time, peak construction time,
selection-handler-to-render time, and startup-to-first-render time. View →
Toggle Diagnostics (Cmd-Shift-D) shows them in a footer; it is hidden by
default. Git panels show background read durations. These are diagnostics,
**not** measurements of GPU frame time, input-to-display latency, or complete
launch time. Instrumentation does not write a log from the UI thread.

The [ADEMark plan](ADEMARK.md) tracks external performance goals and the local
transport probe. These probes do not measure native input-to-pixel latency.

### Layout checks without a Mac

`ui/snapshot.rs` is a Linux-only test module. It opens the real root view in
GPUI's headless test platform, walks every view at 1380 × 860 and 900 × 600,
and draws each one. `cargo test -p plait` runs that walk without a renderer, so
layout and paint errors fail the test. With `PLAIT_SNAPSHOT_DIR` set and
`--ignored`, the same walk renders PNG files through GPUI's headless wgpu
renderer. Host data in these scenes is injected sample state; no socket is
opened and no agent runs. The repository is a disposable fixture.

The images use Linux fonts and a software renderer. They check layout, density,
and color. They are **not** a check of the macOS renderer, system fonts,
accessibility tree, or input. The harness needs GPUI's `test-support` feature,
enabled only for Linux test builds; release and macOS builds do not compile it.

## Dependency decision

GPUI and `gpui_platform` are pinned together at:

`596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14`

The toolchain is pinned to Rust 1.98.1. `Cargo.lock` pins the full
resolved graph. Only macOS features are enabled; `font-kit` is required for text.

Use the APIs at the pinned revision, not examples from a different release.
The app entry point uses `gpui_platform::application()`.

Upstream references:
- [GPUI site](https://gpui.rs/)
- [README](https://github.com/zed-industries/zed/blob/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/gpui/README.md)
- [Contexts](https://github.com/zed-industries/zed/blob/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/gpui/docs/contexts.md)
- [Key dispatch](https://github.com/zed-industries/zed/blob/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/gpui/docs/key_dispatch.md)
- [Examples](https://github.com/zed-industries/zed/tree/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/gpui/examples):
  hello_world, list_example, uniform_list, set_menus, a11y, tab_stop,
  focus_visible, and svg.

Plait uses the Apache-2.0 GPUI libraries, not the GPL-licensed Zed application UI
crate. Its controls use GPUI's focus and action APIs. Dependency notices are
included in release packages; see the [originality policy](ORIGINALITY.md).
