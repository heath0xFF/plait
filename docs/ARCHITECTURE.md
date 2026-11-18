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
creating a branch at the current commit or switching to a local branch, and
adding or removing an optional per-thread worktree.

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
  branches.rs           the branch control and its palette: switch to or create a branch
  palette.rs            search field over a ranked list; ranking off the UI thread
  model_picker.rs       model catalog as a palette; default, favorites, new thread
  new_thread.rs         the new-thread draft: created on its first send, reply guards
  new_thread_view.rs    the draft's project, model, harness, permissions row, and menus
  harnesses.rs          agent harnesses: the host's list, setup, Settings → Agents, mode lines
  permissions.rs        the Permissions chip and menu of a harness's own modes
  text_field.rs         single-line input: IME, selection, clipboard, undo
  navigation.rs         virtualized projects and threads, filtering and management
  navigation_host.rs    cross-checkout listing, live summaries, and notifications
  host.rs               host connection, thread watch, creation, and request guards
  conversation.rs       no-thread state and the Git inspector
  live_conversation.rs  real host messages, ordered tools, and approvals
  history.rs            paged history reads off the UI thread, and their parts
  markdown.rs           drawing precomputed conversation blocks and code
  selection.rs          text selection, copy, and link clicks in the conversation
  conversation_find.rs  find in the conversation: background search and highlights
  tool_card.rs          one line per tool call, its details, and grouped runs
  thread_controls.rs    model, thinking, context, run settings, and queued inputs
  thread_files.rs       files the thread changed, above its composer
  turn_records.rs       guarded record pages, closing-line undo, conflict panel
  changes.rs            changed-file list and diff view
  commit_panel.rs       staging buttons, commit message, and write results
  commits.rs            the History tab: commit list and a commit's patch
  file_view.rs          the Files tab: quick open, virtualized lines, find, reload
  panels.rs             sidebar and inspector widths and their drag handles
  window_bar.rs         the traffic lights' place, header inset, and window dragging
  jump.rs               Cmd-1 to Cmd-9 and the desktop preferences file
  session.rs            draft and submission recovery, and save scheduling
  settings.rs           the Settings page (the app and host) and its tabs
  settings_project.rs   a project's own settings page, from its sidebar menu
  settings_keyboard.rs  the shortcut editor
  settings_editor.rs    Settings → Editor: what opens files and terminals
  ways_out.rs           Open in Editor, Show in Finder, Open Terminal Here; starts them
  settings_policy.rs    the checkout's command policy, read from the host
  settings_providers.rs provider sign-in, API keys, and custom endpoints
  update.rs             release checks and in-app update installation
  worktree.rs           starting a thread in its own worktree, and removing it
  theme.rs              the current palette and the small original controls
  appearance.rs         light and dark themes, the themes folder, and their settings
  snapshot.rs           headless scene snapshots of the root view (tests only)
  snapshot/showcase.rs  README screenshots over a sample project (tests only)
  snapshot/ways_out.rs  which file and line the ways out pick; Settings → Editor (tests only)
  snapshot/find.rs      find in the conversation: scenes and tests (tests only)
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
  settings.rs           Settings: host status, its tabs, and New threads, Appearance, About
  settings_project.rs   a project's settings page: its command policy and Pi setup
  changes.rs            real local status and diff views
  theme.rs              current palette, original controls, embedded glyphs
  snapshot.rs           windowless scene snapshots on macOS and Linux (tests only)

crates/plait-core/src/
host.rs                 bounded Unix-socket framing and requests; background only
host/watch.rs           cancellable reader and bounded async snapshot delivery
host_state.rs           host selection, request identity, and stale-result guards
transcript.rs           stable row identity and incremental list-change plans
history.rs              stored/live merging, text parts, memory limits, run wording
history/records.rs      complete record pages, run membership and closing-line projection
local_socket.rs         owner-only paths and OS peer-identity checks
state.rs                selection, collapse state, request queue, cache
model.rs                plain Rust data, no GPUI
machine.rs              this machine's display name
preferences.rs          desktop-only preferences and Cmd-number order
handoff.rs              editor and terminal commands: parsing, escaping, starting
fuzzy.rs                subsequence matching and scoring for pickers
navigation.rs           project grouping, activity tracking, filtering, and jump order
session.rs              private desktop journal, identity checks, writer lock
markdown.rs             plain text blocks and style ranges, parsed off render
reveal.rs               frame-paced reveal of streaming text and cut blocks
theme.rs                palettes, Base16 scheme parsing, contrast floors
file_view.rs            bounded no-symlink file reads, line index, find, change stamps
highlight.rs            lexical syntax classes for fenced and tool code
diff.rs                 line diff of an edit's old and new text
patch.rs                which rows a Git patch draws, each mapped to its patch line
review.rs               quoting picked diff lines into a message
tool_view.rs            what each tool call shows, and grouping of long runs
selection.rs            selection order, ranges, word bounds, and copied text
conversation_find.rs    case-insensitive matches in drawn conversation blocks
git.rs                  synchronous Git adapter: hardened reads; background only
  git/write.rs          stage, unstage, commit, push, pull, fetch, discard
  git/branch.rs         list local branches; create and switch (via write)
  git/log.rs            commit history and one commit's patch (read-only)
  git/counts.rs         uncommitted line counts for named paths (read-only)
  git/worktree.rs       add and remove optional per-thread worktrees
  git/record.rs         private checkout records, coverage, changed paths, pruning (M9)
  git/undo.rs           checked working-file restoration and durable undo journal (M9)
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

## Checkout recording and undo

The TypeScript runtime owns run boundaries for every harness. `records.ts`
records before entering a harness and after releasing it, keeping the checkout
lease until the latter completes. Steering joins the active pair; queued
follow-ups get new pairs. The `records` and `record_paths` tables in
`host.sqlite` hold ordering, states, exact changed entries, line counts, and
coverage exclusions. Before-record failure prevents harness startup and holds
the queue; after-record failure disables future undo for that run. Startup
recovery marks a late after record as possibly including changes while the host
was down, and uses the checkout lease.

The Rust host exposes internal `record` and `prune` helpers, running the headless
core's Git recorder in a macOS Seatbelt process. Private Git stores are separated
by object format and have one private index/HEAD per checkout identity. The
person's repository is read-only, including its object timestamps. Only a
run's before and after commits are linked, so expiring old pairs can release
objects. Startup and daily pruning pause recording and reclaim expired refs,
private caches, loose objects, and packs after 30 days, including for archived
threads. The [host contract](HOST.md#checkout-records)
describes the layout, limits, exact-byte policy, recovery, and platform boundary.
A directory inventory excludes unreadable folders, unsupported entries, and
embedded repositories, including bare Git directories. Private records keep the
filesystem's name spelling and reject equivalent aliases in changed trees.
A supervisor holds a lifetime pipe to a worker whose process group contains
Git and sandbox descendants. Helper death closes the pipe; worker death or a
240-second record/prune deadline kills the group and releases inherited locks.
Git commands have at most 120 seconds within that budget; the runtime's outer
record/prune timeout is 300 seconds.

Protocol version 5 adds record listing and undo.
`record-views.ts` pages complete paths, coverage and request membership within
the client frame. Snapshot `records_revision` changes when records change;
older snapshot shapes are unchanged. The desktop draws the closing-line
control from these. `history/records.rs` assembles page fragments at their offsets and maps
run request IDs, including steering, to the last loaded message or tool. It
follows undo links to the usable record and prepares file names, counts and
hover text off the UI thread. Detached history does not claim its final run
has ended; missing requests do not produce an invented step count.

`ui/turn_records.rs` fetches on selection and `records_revision` changes. Thread,
connection epoch, watch epoch, revision and fetch generation reject stale pages.
The conversation projection prepares a map; render only looks up its visible
rows. Changed records and conflict panels remeasure affected list rows, including
rows whose message-based summary disappeared because steering joined a run.
The changed-files card merges complete record paths with existing provenance
paths, including when retained records are empty. Separate request generations
keep record refreshes from invalidating provenance replies; thread and connection
guards still reject stale replies. Git supplies current uncommitted counts.
Hover and conflict-panel previews are prepared before rendering, show at most
12 entries per list, and report omitted counts. Undo requests use version 5 only; older hosts keep their existing
summaries without controls. An explicit retry of an unconfirmed undo keeps its
request ID and flags; an interrupted undo is refreshed, never replayed.
Cmd-Option-U is the Thread action Undo Last Turn and uses the same gate as the
visible control, with settings and palette visibility checks and key-repeat
suppression. Failed undo records marked `no_effect` are skipped by action-chain
projection, so the original target stays available without replaying a request.

`plait-host undo` holds the checkout lease and runs the headless restoration
code under a sandbox that allows working files but denies Git-directory
writes. It takes U0, checks each target against the run's after entries and
current no-follow reads, then restores eligible files using descriptor-relative
temporary files and renames. Default conflicts apply nothing; `leave_changed`
selects only safe paths. A final U1 and its private commit note claim only
completed restores, excluding unrelated edits observed during undo. The pair
can itself be undone. Content, executable bits and symlinks are restored;
other permissions, extended attributes and hard links are outside that promise.

The runtime budgets undo at 10 seconds within the serialized sidecar's
20-second request limit. `undo_operations` stores applied-result deduplication
and pending/interrupted identities; `record_garbage` defers removal of abandoned
captures to pruning. A private durable journal precedes writes and records
completed paths. Startup recovers a missing after capture without replaying
writes and marks it after restart. Failures after applying starts can be
partial; callers refresh records and inspect the checkout. The lease excludes
host writers, not editors or other external processes. Immediate revalidation
and exclusive creation protect detected changes, but replacing/deleting an
existing path still has a check-to-mutation race. See the
[version 5 contract](HOST.md#protocol-version-5)
for paging, recovery, deduplication and these verification limits.

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
desktop default with a version 2 host, they run without a prompt. File helpers own checkout writes. By default a project uses the trusted
[command policy](HOST.md#command-policy), in which commands run as the user and
may write the checkout. A project can instead be sandboxed, in which commands
read the checkout and use private scratch space for build output. The
project's settings page (its sidebar menu → Project settings…) offers the
policy, with a deliberate confirmation of Trusted access. Stop revokes pending approvals and
cancels work. Interrupted work must not replay automatically after host failure.
These boundaries need the tests listed in HOST.md before valuable-repository use.

### Pi MCP configuration, without execution

`runtime/src/mcp/config.ts` reads bounded Pi configuration and saved sign-in
metadata. It uses `@earendil-works/pi-mcp` **1.5.2** validation and follows the
same revision's `extensions/mcp/config`, `core/mcp-servers`, and MCP OAuth key
rules. It implements no transport and resolves no environment or shell values.
`skills.ts` obtains Pi's built-in MCP enabled state through the existing
package resolver, then includes MCP metadata only for a settings report;
normal run discovery does not read MCP configuration or register its tools.

`mcp/policy.ts` keeps each project switch in the host settings table, keyed by
canonical checkout and server name and checked against the directory identity.
`agent_resources` and `set_mcp_server` return the same sanitized report. The
latter is an additive version 3 operation; snapshots and protocol negotiation
are unchanged. The [host contract](HOST.md#mcp-server-listing-and-project-switches)
defines the fields, defaults, merge rules, and unknown sign-in state.

The core decodes metadata with no GPUI dependency and accepts an absent list
from older hosts. `settings_agent.rs` prepares row text on the background
executor and draws the MCP section only on the project's settings page. A
switch waits for the host reply; connection and checkout guards reject stale
results. `snapshot/mcp.rs` injects sample rows for wide and narrow scenes.
The next stage still needs server lifetime, tool discovery, approvals, calls,
cancellation, and refusal to replay effects; this stage performs none of them.

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
`log(root, skip, limit)`, `show(root, id)`, and `branches(root)`. The only
write operation is `write(root, op)`: stage, unstage, stage all, unstage all,
commit, push, publish, fetch, pull, a hunk, discard, restore, create a branch,
or switch branches. Each returns data or a user-facing error.

Branches are read with `for-each-ref` over `refs/heads/`, newest committer
date first, at most 500, when the branch palette opens, never on a status
refresh; a newer request supersedes an older reply. Creating runs
`git switch -q -c <name>` and switching runs `git switch -q --no-guess
--end-of-options <name>`, both with the repository's configuration, so
checkout filters and the post-checkout hook run, within 120 seconds. Before
creating, Plait refuses a leading dash, `@{`, and `@`, then asks
`git check-ref-format --branch` and requires it to print the name unchanged,
then refuses an existing branch. A switch must name an existing
`refs/heads/` entry, so `-`, `@{-1}`, commits, and remote branches are refused.
Both refuse while the checkout's `index.lock` exists, since creating a branch
does not touch the index and Git would not notice. Git's own refusal (changes
that would be overwritten, a branch checked out in another worktree) is the
result. A switch reloads history; a new branch does not, as its commits are
the same.

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
  background, as is the widest-row index used for horizontal scrolling.
- A diff keeps every patch line but draws rows (`plait-core::patch`), worked
  out with the parse. A file's diff in Changes leaves out the header lines
  that only repeat its path (`diff`, `index`, `---`, `+++`), since the bar
  above names it; a commit's patch replaces them with one heading row per
  file, named from `+++`, `rename to`, `---`, or the `diff` line in that
  order. Lines before the first `diff` line, and header lines about modes,
  renames, similarity, or binary content, are always drawn. Each row holds
  the index of the patch line it stands for, so picking lines, hunk buttons,
  and the patch-line count use the patch, and staging still rebuilds its
  patch from Git's own output.
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
the system's monospaced font for code. That font is SF Mono, which macOS does
not install under a public name, so it is asked for by its Core Text family
name, `.AppleSystemUIFontMonospaced`. A family chosen in Settings → Appearance
replaces it only after the installed families have been read and the family
has been loaded and measured, both in the background. GPUI draws a missing
family in a proportional fallback, so a name that is not installed is not
drawn; and file and diff views size their sideways scrolling from the line
with the most characters, so a family whose characters differ in width is
refused. The installed families are read again each time Settings opens. SVG assets are embedded strings, not runtime file reads. The
app shows no in-window logo; category glyphs use GPUI's tinted SVG path. Brand
files live in `assets/branding`. The development bundle and icon build are
described in [MACOS.md](MACOS.md).

### Desktop preferences

Model defaults and credentials belong to the host. Choices that belong to one
desktop installation live in `plait_core::preferences`, saved as JSON at
`~/Library/Application Support/Plait/desktop.json` on macOS
(`$XDG_CONFIG_HOME/plait/desktop.json` or `~/.config/plait/desktop.json`
elsewhere). It holds what Cmd-1 to Cmd-9 select, the approval default, whether
the Git inspector is hidden, favorite and recently used models, shortcut
overrides by stable action ID, what opens files and terminals (see "Ways out"
below), and the harness new threads start in (Pi unless
chosen; a harness this client does not know reads as Pi). A draft starts in
that harness while the host lists it as ready, and in Pi otherwise. `PLAIT_DESKTOP_PREFERENCES_FILE` selects an
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

### Ways out

Open in Editor, Show in Finder, and Open Terminal Here are the only places the
desktop starts a program of the person's choosing. `ui/ways_out.rs` picks the
file and line on screen (the Files tab's current find match or first visible
line; in Changes the picked line or the diff's first change, mapped to the
file's own line numbers) and the open checkout. `plait_core::handoff` turns the
choice saved in Settings → Editor into a program and arguments:

- The system default is `/usr/bin/open <file>`; Finder is `/usr/bin/open -R`;
  Terminal is `/usr/bin/open -b com.apple.Terminal <checkout>`.
- Neovim in Rex and a Rex tab are AppleScript run by `/usr/bin/osascript`,
  addressed to Rex by bundle ID (`com.superlogical.rex`). The editor's script
  runs `nvim +<line> -- <file>` with `run command … at <checkout>`; the
  terminal's opens the checkout with `open`. Each path is escaped twice, as a
  POSIX shell word in single quotes and then as an AppleScript string, by pure
  functions with tests.
- A custom command is one line split into words with shell quoting rules
  (single and double quotes, backslash) and `{file}`, `{line}`, and `{project}`
  filled inside each word. It never goes through a shell, so nothing in it is
  expanded. A command that does not parse, or lacks `{file}` for files, or uses
  anything but `{project}` for terminals, is refused before it is saved.

The program starts on the background executor with no input, its output
discarded, and its error output kept only until it exits. The desktop checks it
every 50 ms for two seconds without blocking a thread; an exit with an error
in that time becomes one sentence in the notice line. After that the process is
left to run, and a thread empties its error output and reaps it when it exits.
Plait does not depend on Rex: without it, `osascript` fails at once, and the
sentence says Rex is not installed. Test builds record the command instead of
starting it.

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
uses only host facts: no invented stop actor or per-turn duration. A finished
turn's record counts observed tool steps and names successfully edited or
written paths (`plait-core::history::TurnSummary`). A turn with no tool calls
has a record with nothing to count. A turn whose request is outside the loaded
rows has none, because the steps before the first loaded row cannot be counted.
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

Find in the conversation (`conversation_find.rs`, with the matching in
`plait-core::conversation_find`) searches the parsed blocks the view draws,
not the Markdown source, so each match is a byte range of one block. The
search runs on the background executor over the display snapshot and its
projection, both shared and replaced rather than changed, so starting one
copies nothing on the UI thread. One search runs at a time. A new query and
new rows (a reply, a history page, another thread) bump a generation; a result
is shown only if its generation and its projection are still current, and
otherwise the search runs again. Results come grouped by message ID, so drawing
a row looks up only its own matches and restyles them with the same run
splitting as the selection. Moving to a match scrolls the list to its row, then
places the match in view on the next frame from where its text was drawn,
since only drawn text knows where a match sits inside a tall row. A new result
for the same query keeps the current match by its message, block, and range and
leaves the scroll position alone. Requests and replies are searched; the
streaming reply, reasoning, tool rows, and turn records are not, and pages that
are not loaded are never read for a search.

Conversation events use GPUI's variable-height `list`. The conversation is
drawn as a log of turns: a request heads its turn at a ring on the turn line,
and replies and tool calls are open text beside it. Each row draws its own
piece of that line across its full height, padding included, so the pieces
meet; a row needs only its own kind and position, so no pass over the
transcript runs per frame. An earlier turn's record is part of its last
answer's row. The latest
turn's record is drawn with the run state below the list, where the line ends.
When a new request arrives, the answer before it takes its record into its own
row; that row is measured again, since its text has not changed. While the
list is scrolled back from its newest row, or older history is being read, the
rows on screen stop short of the latest turn, so the line is broken above the
state. Reading older history, no record is drawn there either.
Tool details start collapsed. Disclosure state belongs to the UI and clears on thread selection.
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
Changes switch; there is no separate tab strip. The header is also the window's
title bar (`ui/window_bar.rs`). The window opens with a transparent, untitled
title bar, so macOS draws only the traffic lights, placed by
`TitlebarOptions::traffic_light_position` to center on the 44-point row 14
points in. The window keeps its title, Plait, for Mission Control and the
Window menu. The rail starts with an empty 44-point strip under the lights;
with the rail hidden, the header's left padding clears them, except in full
screen. The window is opened with `app_owns_titlebar_drag`, so AppKit neither
moves it from the title bar area nor delays clicks there. Instead the strip
and the header move it themselves: a left press that no control took (a
control prevents default when it takes focus) arms a move, the first drag
calls `Window::start_window_move`, and any release disarms it. A second press
calls `Window::titlebar_double_click`, which follows the system's
double-click setting. The strip stops short of the rail's resize handle. Navigation, changed-file, and
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
| macOS | GPUI's Metal renderer, drawing into an offscreen texture | Core Text, with the system font and the system's monospaced font |
| Linux | GPUI's wgpu renderer on a software Vulkan driver | cosmic-text, with the fonts installed there |

On macOS the scene is drawn by the same renderer and shaped by the same text
system and fonts as the app, so glyphs, wrapping, and truncation come from the
code the app runs. On Linux they do not; those images check layout, density,
and color only.

Neither is the native window. The images do **not** show or check:

- the traffic lights, which macOS draws over the rail's top strip; scenes
  show that strip empty;
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
