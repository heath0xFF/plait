# Architecture

## Scope

The local preview (M2.5) uses a native desktop and an independent host. The workspace
contains `crates/plait` (native desktop), `crates/plait-core` (headless models,
client state, local host client, and Git), and `crates/plait-host`
(local daemon and filesystem/process helpers). `runtime/` contains the pinned
Pi Durable TypeScript sidecar. There is no remote execution,
embedded terminal, or editor. Git writes are limited to
staging, unstaging, committing, pushing (or publishing a branch), fetching,
fast-forward pulls, discarding unstaged changes after saving a copy for undo,
and adding or removing an optional per-thread worktree.

This remains an early local preview. Linux command execution and remote
transport are not implemented. [Verification](VERIFICATION.md) records versioned
Mac installation, sandbox, and recovery evidence, with the remaining gaps.

```text
crates/plait/src/
main.rs                 native app lifecycle, window, startup
ui/                     GPUI layout, actions, focus, lists, theme
  mod.rs                UI state and background-task dispatch
  actions.rs            every application action: ID, title, menu, default key
  commands.rs           command palette sources and the overlay above the workspace
  palette.rs            search field over a ranked list; ranking off the UI thread
  model_picker.rs       model catalog as a palette; default, favorites, new thread
  text_field.rs         single-line input: IME, selection, clipboard, undo
  navigation.rs         virtualized projects and threads, filtering and management
  navigation_host.rs    cross-checkout listing, live summaries, and notifications
  host.rs               host connection, thread watch, creation, and request guards
  conversation.rs       no-thread state and the Git inspector
  live_conversation.rs  real host messages, ordered tools, and approvals
  history.rs            paged history reads off the UI thread, and their parts
  markdown.rs           drawing precomputed conversation blocks and code
  selection.rs          text selection, copy, and link clicks in the conversation
  tool_card.rs          one line per tool call, its details, and grouped runs
  thread_controls.rs    model, thinking, context, run settings, and queued inputs
  thread_files.rs       files the thread changed, above its composer
  changes.rs            changed-file list and diff view
  commit_panel.rs       staging buttons, commit message, and write results
  commits.rs            the History tab: commit list and a commit's patch
  file_view.rs          the Files tab: quick open, virtualized lines, find, reload
  panels.rs             sidebar and inspector widths and their drag handles
  jump.rs               Cmd-1 to Cmd-9 and the desktop preferences file
  session.rs            draft and submission recovery, and save scheduling
  settings.rs           the Settings page and its sections
  settings_keyboard.rs  the shortcut editor
  settings_policy.rs    the checkout's command policy, read from the host
  settings_providers.rs provider sign-in, API keys, and custom endpoints
  update.rs             release checks and in-app update installation
  worktree.rs           starting a thread in its own worktree, and removing it
  theme.rs              the current palette and the small original controls
  appearance.rs         light and dark themes, the themes folder, and their settings
  snapshot.rs           headless scene snapshots of the root view (tests only)
  snapshot/showcase.rs  README screenshots over a sample project (tests only)
  composer/             native input, IME, clipboard, undo, and its events
    draft.rs            text, selection, undo, acceptance guard, prompt recall
    history.rs          prompts already sent in the thread
    input.rs            wrapped layout, auto-growing height, painting
    trigger.rs          `@` and `/` detection and the inserted mention text
    completion.rs       reusable suggestion list: state and rendering
    mentions.rs         background file ranking and its stale-result guard
    commands.rs         the local command table
    parent.rs           what the window supplies and performs for a composer
  host.rs               background host requests and event subscriptions
  history.rs            bounded history reads, projection dispatch, and reading controls
  session.rs            desktop restore/save coordination and submission guards
  markdown.rs           native conversation blocks: prose, code, tables, diffs
  settings.rs           host status and the Models, Approvals, Keyboard, About sections
  changes.rs            real local status and diff views
  theme.rs              current palette, original controls, embedded glyphs
  snapshot.rs           windowless scene snapshots on macOS and Linux (tests only)

crates/plait-core/src/
host.rs                 bounded Unix-socket framing and requests; background only
host/watch.rs           cancellable reader and bounded async snapshot delivery
host_state.rs           host selection, request identity, and stale-result guards
transcript.rs           stable row identity and incremental list-change plans
history.rs              stored/live merging, text parts, memory limits, run wording
local_socket.rs         owner-only paths and OS peer-identity checks
state.rs                selection, collapse state, request queue, cache
model.rs                plain Rust data, no GPUI
machine.rs              this machine's display name
preferences.rs          desktop-only preferences and Cmd-number order
fuzzy.rs                subsequence matching and scoring for pickers
navigation.rs           project grouping, activity tracking, filtering, and jump order
session.rs              private desktop journal, identity checks, writer lock
markdown.rs             plain text blocks and style ranges, parsed off render
reveal.rs               frame-paced reveal of streaming text and cut blocks
theme.rs                palettes, Base16 scheme parsing, contrast floors
file_view.rs            bounded no-symlink file reads, line index, find, change stamps
highlight.rs            lexical syntax classes for fenced and tool code
diff.rs                 line diff of an edit's old and new text
review.rs               quoting picked diff lines into a message
tool_view.rs            what each tool call shows, and grouping of long runs
selection.rs            selection order, ranges, word bounds, and copied text
git.rs                  synchronous Git adapter: hardened reads; background only
  git/write.rs          stage, unstage, commit, push, pull, fetch, discard
  git/log.rs            commit history and one commit's patch (read-only)
  git/counts.rs         uncommitted line counts for named paths (read-only)
  git/worktree.rs       add and remove optional per-thread worktrees
fuzzy.rs                case-insensitive subsequence matching for pickers
```

The desktop imports `plait_core`; the core has no GPUI dependencies. Core and
host Rust tests can run on Linux without compiling the desktop. The default
Cargo member remains the macOS desktop. The macOS workspace checks still cover
all crates, including the native composer. See CONTRIBUTING.md for both commands.
The core uses Unix filesystem and socket APIs; it is not a Windows port.
Filesystem, socket, and Git operations run on the background executor.

A Linux build does not enable agent command execution. The runtime still rejects
non-macOS commands under either command policy, and the Rust command guardian
rejects both its sandboxed and trusted entries there. Linux
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

The host owns model discovery, Pi authentication, thread persistence, and the
default model. The desktop receives availability, not credentials. The host's
[provider sign-in operations](HOST.md#provider-sign-in) take a pasted key or
sign-in code from a client once and store it in Pi's files. Settings → Providers
calls these operations and refreshes the model catalog after changes.
Each thread keeps its exact provider/model pair and thinking level until a
client changes them between runs ([protocol version 3](HOST.md#model-and-thinking-level)).
Saving a default does not change an existing thread.

The host also owns each thread's title, timestamps, archive flag, and the queue
of inputs waiting behind a run. A snapshot is the bounded live tail of a thread.
The complete conversation is read on demand, in pages, from the thread's saved
conversation store; an idle thread's store is opened read-only for that, so a
history read cannot recover or run anything. The desktop loads stored history,
shows reasoning, and uses live summaries, host-owned titles, rename, and archive.
Composer controls configure models and thinking and submit follow-up or steering
messages through the same negotiated version 3 contract.

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
revision still matches the submitted draft. Each pending request retains its own
revision, queue mode, and approval mode, so out-of-order replies cannot erase a
newer draft or change a retry’s authorization. Each thread has its own composer.

Each run is submitted with an approval mode. In `ask` mode every file change and
command needs a one-call approval with full arguments; in `auto` mode, the
desktop default with a version 2 host, they run without a prompt. File helpers own checkout writes; command execution reads the checkout and uses
private scratch space for build output. A project can instead be set to the
opt-in trusted [command policy](HOST.md#command-policy), in which commands run
as the user and may write the checkout. Settings → Approvals offers the policy
with a deliberate confirmation of Trusted access. Stop revokes pending approvals and
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
Command containment is a property of the checkout's command policy, not of the
approval mode: a run reads the policy once at admission, and only a trusted
project takes the separate uncontained path. File-tool checks apply to both.

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

`command/policy.ts` owns the per-checkout command policy, its two host
operations, and the trusted wording for instructions, the tool description, and
approvals. `command/login-environment.ts` reads the account's login-shell
environment for trusted commands. `command/trusted.ts` runs a trusted command
through the same supervisor and guardian. Before the command it drops
ownership records that had already drifted; afterwards it refreshes the records
of this thread's files that the command changed and reports changes to
pre-existing dirty paths.
`tools.ts` chooses the path from the policy the run was admitted with.

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

The checkout's file list is a third job, started only when a composer asks for
it. It has its own queue slot, so it cannot displace a diff, and it runs after
status because it needs the root that status reports. The list is kept until
the epoch changes; a failed read is not cached, so the next request retries.
Git reads do not start SSH or network connections. Remote transport is not
implemented.

## Git boundary

Read operations are `status(path)`, `diff(root, change, mode)`, `files(root)`,
`log(root, skip, limit)`, and `show(root, id)`. The only write operation is
`write(root, op)`: stage, unstage, stage all, unstage all, or commit. Each
returns data or a user-facing error.

Reads and writes differ on purpose. A read must not run anything the
repository configures, so it neutralizes filters and never runs hooks. A write
is something the person asked for and must produce what their own `git add` or
`git commit` would, so it runs with the repository's configuration: clean
filters (such as LFS) run while staging, and hooks run while committing. Writes
pass paths on standard input (`--pathspec-from-file`), never open an editor,
and allow 60 seconds for staging and 120 for a commit. A held `index.lock` is
reported as another Git process at work. On an unborn branch, unstaging uses
`git rm --cached` because there is no HEAD to restore from. `state.rs` runs one
write at a time, ahead of reads, then refreshes status and keeps the same file
selected. `show` validates a full object ID and passes it after
`--end-of-options`.

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
- `files` lists tracked files plus untracked files that are not ignored, with
  one `ls-files` call under the same hardening and limits. It reads names, not
  contents. Files deleted from the worktree, sparse-checkout entries, and
  nested repositories are left out. A list over 8 MiB or 200,000 paths is
  clipped and marked, because a picker can still use part of it. Names that are
  not UTF-8 or contain control characters are counted and skipped: they cannot
  be typed into a message unchanged.
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
elsewhere). It holds what Cmd-1 to Cmd-9 select, the approval default, whether
the Git inspector is hidden, favorite and recently used models, and shortcut
overrides by stable action ID. `PLAIT_DESKTOP_PREFERENCES_FILE` selects an
absolute preferences path for an isolated development instance. The model
lists are provider and model identifiers only, bounded in length and count; the
file never holds a credential. The navigation section also stores opened and
hidden checkout paths, up to 1,000 observed thread activity stamps and unread
markers, and the notification preference. Older files load with empty navigation
state; thread content stays in the host and draft journal.

The file is read and written on the background executor. A missing file means
defaults. A malformed or oversized file is reported in Settings and left in
place; defaults apply. Saves replace the file atomically, and Settings says
"Saved" only after the write succeeds. Writes are serialized, with intervening
changes coalesced to the newest snapshot; only the newest save reports a result.
An unsafe or failed load disables writes for that window, preserving the file.
Unknown fields are ignored so a newer client's file still loads, and a file
written before a field existed loads with that field's default.

Cmd-1 to Cmd-9 count matching real host threads in project order on the local
machine. A machine or project selects its first matching thread; an empty row
does nothing. Jumping expands collapsed parents.

### Projects and thread activity

With protocol version 3, the desktop subscribes before paging the complete thread
list across checkouts. Live summaries replace their matching rows; old listings
are replaced on reconnect. The host watches its newest 200 threads. Older loaded
threads remain readable and explicitly refreshable, with this limit shown above
the list. A failed watch is reported as paused updates rather than current data.
No subscription is polled while idle.

Projects come from host checkout paths and opened folders. Hidden projects stay
in the host and can be reopened through the palette. Folder availability checks
run in the background. Successful Git status results normalize a selected
subdirectory or symlink to its checkout root. Switching projects preserves
per-thread composers and pending request IDs, changes the repository epoch, and
cancels only the selected snapshot subscription. It never sends Stop.

A completion, failure, interruption, stop, or approval change marks an unseen
thread until it is opened. Observations and unread markers persist in desktop
preferences. The initial listing updates markers but does not post notifications.
While another thread is visible or the window is inactive, later attention-worthy
changes request window attention and post a GPUI system notification. Opening a
notification selects its thread. Alerts off suppresses both external signals,
while in-app markers remain. GPUI checks for a bundle identifier before using
macOS UserNotifications and requests authorization on first delivery. Unbundled
builds and denied authorization retain in-app markers and the attention request.
No test posts a real system notification.

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

### Composer

The composer owns editable text and reports intent as events: Submit (text, edit
revision, and whether to steer), Stop, a
chosen local command, and a request for the checkout's files. The window owns
host requests and Git reads and performs the commands. `Draft` holds every
editing rule without GPUI, so the rules are unit-tested.

The input is one line when empty and grows with the wrapped draft to about a
third of the window height, with a floor of four lines, then scrolls. Its
height comes from a measured layout at the final width. The conversation list
is bottom-aligned, so it stays pinned to its newest row as the input grows.
Wrapped rows are kept between frames. Moving the cursor shapes nothing, and an
edit reshapes only the lines whose text changed, so a large paste stays
responsive. Undo keeps at most 100 steps and 4 MiB of draft text.

Up on the first visual line recalls prompts sent in the thread, newest first.
Down walks forward and finally returns the draft, its selection, and its undo
history unchanged. Recall adds no undo step; editing a recalled prompt makes
the previous draft one undo step. Every recall step advances the edit
revision, so an acknowledgement for an earlier submission cannot clear recalled
text. The saved draft is the one being written, not the prompt on screen.
Sending a recalled prompt unchanged returns to that draft. History comes from
the host snapshot's user messages. The host keeps only the tail of a long
prompt, so prompts sent from this composer are also remembered exactly, for
the life of the window.

Typing `@` at the start of a word lists checkout files; `/` as the first
character of an otherwise empty draft lists local commands. Both use one
suggestion list, `composer::Completion`: plain state and a render function
that never takes focus. The file list comes from `git::files` through the
repository state. Ranking runs on the background executor with
`plait_core::fuzzy`; a result is applied only if it belongs to the newest
query and the list is still open. Accepting a file replaces `@query` with the
checkout-relative path and a space, such as `src/main.rs `. A path with spaces
or other word-ending punctuation is wrapped in backticks. Nothing is attached
and no file is read: the agent receives the text a person would have typed.
Commands are a table in `composer/commands.rs`; each maps to an action the app
already has.

Escape closes the suggestion list, then leaves recall. While the agent works,
two Escape presses within 1.5 seconds stop the run; one press only arms the
gesture, and any edit or cursor movement cancels it.

The scene walk draws the composer empty, with several lines, past its height
limit, and with each suggestion list open, at both window sizes. One scene
types a mention, lists the fixture repository through Git, and accepts the
result. The scenes do **not** exercise key events, IME, pointer selection,
wheel scrolling, or the accessibility tree; those have had no check in a
native window. The editing rules behind them are unit-tested.

### Queue and per-thread controls

A version 3 composer can send during a run: Enter follows up, Option-Enter
steers, and Stop remains a separate control. The core `Submissions` collection
tracks each outstanding request independently. Request ID, full text, queue
mode, approval mode, and draft revision are written before socket I/O. A queued
snapshot item acknowledges a request just as its accepted message does. Sending
flags are transient; reopening and reconnecting never submit on their own.
The legacy journal `pending` field holds the first outstanding item;
`pending_submissions` holds the rest. Additive `queued_submissions` preserves
exact local text for cancelling a queued message back into the draft. Queues
owned by the host remain authoritative; this local cache starts no work.

`ui/thread_controls.rs` presents model, thinking, approval, context usage, and
the input queue. Model changes are refused during a run. Thinking choices come
from the catalog, and request replies are guarded by host epoch, selected
thread, and request sequence. The subscription owns snapshot order; control
replies do not replace a newer event. Saved thread identity is ID and checkout;
a model change preserves draft recovery. Pure usage formatting and thresholds
live in `plait-core::compose`, with no GPUI dependency.

### Native conversation text

`plait-core::markdown` projects CommonMark, with tables, strikethrough, and task
lists, into plain blocks and style ranges on the background executor.
`pulldown-cmark` is pinned to 0.13.0 with default features disabled; its
MIT-licensed source and event API were checked at that version. No HTML renderer
or web view is used. HTML stays text and an image is never fetched: its
description and address are shown as text. Unchanged rows retain their
projection while the live answer changes. Excessive block counts fall back to
the original text rather than dropping content, and an oversized table becomes
tab-separated text.

A link is data, not an action. Only an `http` or `https` destination without
whitespace or control characters becomes one, whether written as a Markdown link
or as a bare URL in prose; every other destination stays inert text beside its
label. The view underlines a link, shows its address at the edge of the
conversation while the pointer rests on it, and opens it in the default browser
only when a click starts and ends on it. Nothing opens on its own.

Fenced code and tool content are tokenized by `plait-core::highlight`, a small
original lexer with no dependency. It assigns lexical classes (comment, keyword,
string, number, type, function, key, punctuation) for Rust,
TypeScript/JavaScript, Python, shell, JSON, TOML, YAML, Markdown, diff, C/C++,
and Go; any other label stays plain. It has no grammar and cannot fail, so
partial code from a streaming answer is colored as far as it goes. A parser
with per-language grammars was not adopted: it would add a C build and several
megabytes for a view that only needs code to be scannable. The classes map onto
the existing palette; see [DESIGN.md](../DESIGN.md).

`plait-core::tool_view` prepares what each tool call shows from its bounded
arguments and result: `edit` as a line diff of old against new text
(`plait-core::diff`), `write` as the new file highlighted by its extension,
`command` as a prompt line and its output, and `read` as the first lines of the
file. Arguments that the host clipped or that are not the expected JSON are
shown as received, with a note. The same pass plans groups: a finished run of
five or more consecutive calls folds under one line with its count and kinds,
and the run a busy thread is still adding to keeps its latest three calls in
view. Version 3 stored tool fields replace bounded live previews; opened read results
show all loaded lines. Version 1 and 2 hosts keep the bounded-preview behavior.
Git refreshes after an observed busy-to-complete transition.

`plait-core::history` keeps a reading window of at most 600 stored items and
16 MiB of field text. The projected snapshot, Markdown blocks, and syntax spans
are additional bounded representations of that window. One page hydrates up to
2 MiB beyond its inline text. An oversized field is read in parts of roughly
262,144 UTF-16 units, with explicit Next part and Start over controls; copying
copies the displayed part. Every continuation must advance on the declared
UTF-16 boundary and finish at the declared length.

The host snapshot stays the authoritative live state for submissions and status.
A separate display snapshot combines stored items with its live tail, preserving
full stored fields and the newest tool status. Reading older pages evicts the
newer end when needed and labels the detached window; Latest reopens the live
end. Forward paging is not available. Ordinary forward scrolling works within
the loaded window. Evicted older pages can be loaded again, including when an
eviction cuts through a page. A missing history seam resets to the newest page
instead of implying that two separated ranges are contiguous.

History socket reads and Markdown/tool projection run on the background executor.
There is one history read and one projection in flight per view. New snapshots
coalesce while projection runs; host epoch, watch epoch, thread ID, and projection
revision reject stale delivery. Pages arriving after newer live state are merged
with that state. Prepending uses the existing stable-ID list splices, so the
current item and pixel offset survive. The scene harness verifies that invariant.

Reasoning has its own stable row identity and parsed blocks. Completed reasoning
starts collapsed; live reasoning has a compact, scrollable body. Disclosure and
selection survive ordinary snapshot updates. Replacing an oversized text part
clears selection because its offsets now refer to another part. Run-state copy
uses only host facts: no invented stop actor or per-turn duration. Completed
answers summarize observed tool steps and successfully edited or written paths.
A response still streaming can only show the host's bounded live tail; its full
stored text loads after the host saves it.

Conversation text can be selected with the pointer: drag, double-click for a
word, triple-click for a block or a line of code, and shift-click to extend,
within a message or across messages and opened tool details. Every piece of
text paints through one element that records its laid-out position for the
frame; pointer events map those records to a row, block, and byte offset using
GPUI's text layout, and `plait-core::selection` decides order, ranges, and the
copied text. The selection is drawn as a background on the text runs and is
kept by row identity, so it survives streaming and list changes. Cmd-C copies
it once a press in the conversation has moved focus there; the composer keeps
its own Cmd-C while it has focus. Copy controls for a message's source, a code
block, and a tool's details appear when the pointer is over their row or when
they take keyboard focus. Dragging past the top or bottom edge scrolls the list
one step for each pointer movement; it does not keep scrolling while the
pointer rests. Rows between the two ends are copied whether or not they are on
screen; a tool's details are copied only while they are open.

Conversation events use GPUI's variable-height `list`. User messages render as
right-aligned bubbles; agent replies remain open text. Tool details start
collapsed. Disclosure state belongs to the UI and clears on thread selection.
A call folded into a closed group keeps its list row and draws nothing, so row
identity and the scroll anchor do not depend on grouping; rows whose folding
changes are remeasured. A control to jump to the newest row appears while the
reader is elsewhere in the history, and says when content has arrived since.
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
`ui/theme.rs` owns the small original controls and the current palette, which
views read with `colors()` while drawing; `ui/appearance.rs` picks it from the
preferences and the macOS appearance and redraws when either changes. The
palette is per thread, so parallel UI tests with different themes stay apart. A control's tone
(ghost, selected, filled, primary) is chosen when it is built, because GPUI
accepts one hover style per element and reports a second one only at draw time.

The sidebar defaults to 272 points on wide windows and 240 on narrow windows.
Below 1150 points, the conversation's Git inspector (280 points by default) is
removed; Git remains available in the Changes tab. The minimum window is
900 × 600. Cmd-B hides the sidebar.

Both panels resize by dragging their inner edge (`ui/panels.rs`): the sidebar
between 200 and 480 points, the inspector between 220 and 560, and neither so
wide that the conversation has less than 420. A drag changes only the width on
screen and saves it to desktop preferences on release; double-clicking the
edge restores the default.

Cmd-L hides the inspector on wide windows and the choice is saved; on a
narrow window the same control says that Git is in Changes.

Controls have focus rings, tab stops, accessible names, and semantic roles.
Thread state has both a color and a text label.
No decorative animation is used.

### Actions and shortcuts

`ui/actions.rs` lists every application action once: a stable string ID, a
title, the menu it belongs to, and a default keystroke. `actions::bind` is the
only place application shortcuts are bound. It takes a map of action ID to
keystroke; an entry replaces the default, an empty entry unbinds, and an entry
that does not parse leaves the default in place. `actions::install` clears and
rebinds everything and rebuilds the menus, so a keymap can be applied again at
run time. Settings → Keyboard records shortcuts through GPUI's pre-dispatch
keystroke interceptor, detects conflicts, and rejects text-entry and common
system keys. Overrides load from desktop preferences; unknown IDs survive.
The menu bar, the command
palette's entries and shortcut hints, and the table in [MACOS.md](MACOS.md) come
from the same table, and a test compares the document with it. Editing keys that
belong to one control (the composer, the text field, the palette list) are bound
by that control.

### Command palette and model picker

`ui/palette.rs` is one view: a single-line search field over a list, drawn as a
centered overlay that blocks the workspace behind it. It knows nothing about
what its items do. A caller passes items (title, detail, section, state, and a
command) and receives the chosen command. The command palette lists threads,
the action table, and "New thread with" each signed-in model; the model picker
lists the host's catalog under Favorites, Recent, and each provider. Adding a
source is a function that returns items and a `Command` variant.

Ranking is a pure function from the search text and plain item data to rows. It
runs on the background executor, guarded by a query counter so only the newest
result is shown. Enter ranks synchronously when a result is still pending, so
the choice always matches the text in the field. Rows use `uniform_list`; only
visible rows are built, which matters for a catalog of well over a thousand
models. Focus stays in the search field and the selected row is reported to
assistive technology as the active descendant. Escape, or a click outside,
closes the overlay and returns focus to where it was.

Models that are not signed in are listed after every usable one, marked in
words, and cannot be chosen. The default model is marked wherever models are
listed. Choosing a default sends the host's `set_default_model` request at
once; the picker footer and Settings report Saving, the host's confirmation, or
the failure. Nothing is shown as saved before the host answers.

### Provider settings and command policy

`ui/settings_providers.rs` renders the host's catalog, subscription flow state,
API-key form, and custom endpoint editor. plait-core's `host/providers.rs` defines typed
requests and tolerant replies; its `host/sign_in.rs` projects each flow into its
available controls. Status polling runs only while its form is open and pending.
Host epochs and form generations reject stale replies. Closing an in-flight
start cancels the returned flow instead of reopening the form.

Secret fields omit accessibility values and clipboard copy, keep no undo history,
and move their value into a redacted request on send. Request buffers and owned
secret strings are overwritten on disposal. They never enter desktop preferences
or the recovery journal; this is not a claim to erase every allocator or OS copy.
The host remains the credential owner. No sign-in URL opens without a click.

`ui/settings_policy.rs` reads the canonical checkout's policy on connection or
checkout change. A response must match that checkout and its host epoch. Until
it arrives the UI says the policy is unknown, never promises a sandbox. A trusted
change needs explicit confirmation; the host's response determines the shown value.

### New threads

New Thread sends `create_thread` with the saved default model and focuses the
new thread's composer; there is no intermediate page. When no default can be
used, the model picker opens and the thread is created on selection. An
unconfirmed creation keeps its request ID: the workspace shows it with a retry
that reuses that ID, and a new request is not sent while it is pending. A model
is recorded as recently used only after the host confirms the thread.

The app records root-view construction time, peak construction time,
selection-handler-to-render time, and startup-to-first-render time. View →
Toggle Diagnostics (Cmd-Shift-D) shows them in a footer; it is hidden by
default. Git panels show background read durations. These are diagnostics,
**not** measurements of GPU frame time, input-to-display latency, or complete
launch time. Instrumentation does not write a log from the UI thread.

The [ADEMark plan](ADEMARK.md) tracks external performance goals and the local
transport probe. These probes do not measure native input-to-pixel latency.

### Scene snapshots without a window

`ui/snapshot.rs` is a test module for macOS and Linux. It opens the real root
view in GPUI's test platform, which has no native window, walks every view at
1380 × 860 and 900 × 600, and draws each one. `cargo test -p plait` runs that
walk without a renderer, so layout and paint errors fail the test.
`bash scripts/snapshots.sh <directory>` runs the same walk with a headless
renderer and writes one PNG per scene, at twice the logical size.

| | Renderer | Text |
|-|-|-|
| macOS | GPUI's Metal renderer, drawing into an offscreen texture | Core Text, with the system font and Menlo |
| Linux | GPUI's wgpu renderer on a software Vulkan driver | cosmic-text, with the fonts installed there |

On macOS the scene is drawn by the same renderer and shaped by the same text
system and fonts as the app, so glyphs, wrapping, and truncation come from the
code the app runs. On Linux they do not; those images check layout, density,
and color only.

Neither is the native window. The images do **not** show or check:

- the title bar, or the height it takes from a 1380 × 860 window;
- presentation through the window's layer, display color management, or a
  display that is not 2×;
- hover, pressed, and focus states that follow native pointer and key events,
  the text caret, IME, or scrolling by input. Most scenes set view state
  directly. The reading scenes deliver pointer and key events through GPUI's
  own dispatch to check text selection, copy, and link hover; that exercises
  hit-testing and handlers, not the operating system's event path;
- the accessibility tree, which the harness never reads;
- animation, frame timing, or latency. Each image is one frame.

Host data in these scenes is injected sample state held in small functions in
the test module. The repository is a disposable fixture. Building the root view
probes the host socket and reads desktop preferences, so each scene test
re-runs itself in a child process whose home directory and host store are empty
scratch directories. It cannot reach a running host or a user's saved state.
The images do show the machine's name and a temporary path, so they are
inspected locally, not committed.

Two tests in the same module send key events to that window instead of setting
state: Cmd-K, typed letters, the arrows, Return, Cmd-Return, Tab, and Escape
reach the palette and model picker through GPUI's key dispatch and text input
path, and New Thread keeps the request ID of an unconfirmed creation. They
check routing, focus return, and ranking. They do **not** check a native key
event, an input method, the caret, or pointer input.

The harness needs GPUI's `test-support` feature. It is a dev-dependency feature
for macOS and Linux test builds; `cargo build` and release builds do not
compile it.

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

### Self-hosting tool output

`tool-output.ts` stores bounded command logs in the host SQLite database. Tools
return thread-scoped IDs rather than host paths; `read_output` pages the stored
bytes without another command execution. Process capture is separate from the
bounded live preview. Ranged source reads use a descriptor-checked Rust helper;
edit admission inspects a complete fingerprint without transferring the whole
file across the helper pipe. Context summaries remain in Pi's durable store but
are omitted from the presentation projection, which retains original messages.
