# Product roadmap

These are planned milestones, not a list of shipped features. See the
[README](../README.md) for the current preview and [Verification](VERIFICATION.md)
for acceptance evidence and open checks. **M2.5 shipped in v0.1.0-preview.8.
M8 steps 1 and 2 (skills, prompt templates, and user instructions) shipped in
v0.1.0-preview.9. M7 steps 1 (choosing a harness, and Codex) and 2 (each
harness's own permissions, and Claude Code threads) shipped in
v0.1.0-preview.10. Ways out, branches, and search in a conversation shipped in
v0.1.0-preview.13. M9 step 1 (per-turn undo) shipped in v0.1.0-preview.15. M8
step 4's tool calls (MCP servers that need no browser sign-in), images in
messages, and the first part of M9 step 2 (a run's closing line opens exactly
what that run changed) shipped in v0.1.0-preview.17. Thread tabs shipped in
v0.1.0-preview.18. A second thread in a busy project, in its own worktree,
and any number of New thread tabs shipped in v0.1.0-preview.20. The rest of
M9 steps 2 and 3, MCP browser sign-in, and M8 step 3 are the current work.**

## Milestones

| Milestone | Scope | Status |
| --- | --- | --- |
| M1 — Native foundation | GPUI desktop, navigation, conversations, read-only Git status and diffs | Foundation implemented |
| M2 — Local agents | Real agent events, approved edits and commands, persistent threads, safe Stop and reconnect | Shipped in the local preview; hardening and live installed acceptance continue |
| M2.5 — Daily driver | Complete, paged conversation history; projects and threads across checkouts with live status; thread titles, rename, and archive; model and thinking changes within a thread; follow-up and steering messages during a run; provider sign-in from the app; selectable, highlighted conversation text and readable tool results; a command palette; optional per-thread worktrees; an opt-in trusted command mode | Shipped in v0.1.0-preview.8; installed acceptance continues ([Verification](VERIFICATION.md#v010-preview8-acceptance)) |
| M3 — Remote execution | Authenticated host connections, remote project discovery, activity/Git updates, network recovery | Planned; Linux execution requires tested containment first |
| M4 — Development tools | Git staging, commits and history; file browser/editor; review feedback; process and log views | Git shipped ahead of M3 (staging, hunks, commits, history, push, fetch, fast-forward pull); a read-only file viewer shipped in v0.1.0-preview.8; the rest planned |
| M5 — Previews | Live HTML, Markdown and MDX, automatic refresh, and selected-content feedback to the agent | Planned |
| M6 — Multiple machines | Unified project navigation across independent hosts, including disconnected/reconnecting states | Planned |
| M7 — Any agent harness | Threads run by other agent harnesses (Claude Code, Codex, Pi, omp, Antigravity, and others) beside Pi, with the same conversation, approval, Git, and navigation views | Current work. Steps 1 and 2 (harness choice, Codex, each harness's own permissions, and Claude Code; protocol version 4) shipped in v0.1.0-preview.10; an ACP client follows, agreed first |
| M8 — Skills, tools, and CLIs | Pi threads use the skills, prompt templates, global instructions, MCP servers, and command-line tools the person already set up for their agent, with each one visible in Settings and every call under Plait's approvals | Steps 1 (skills and user instructions) and 2 (prompt templates, Settings → Agent, and `/` completion) shipped in v0.1.0-preview.9; step 4's MCP tool calls shipped in v0.1.0-preview.17, with browser sign-in still planned; step 3 (named CLIs) waits until a sandboxed project needs it |
| M9 — Turn records | The host keeps its own record of the checkout before and after every turn, in every harness: per-turn undo first, then committing by turn, review of what changed since the person last looked, and checks the host runs itself | Step 1 shipped in v0.1.0-preview.15; steps 2 and 3 are agreed and are the current work, ahead of M7's ACP client; the later steps are proposed |

Do not build the whole roadmap at once. Each change must compile, run, and have
its own checks. Protocol, storage, and shared-contract changes need agreement
before implementation.

**Agreed for M2.5:** protocol version 3 and additive storage changes. Version 3
is negotiated per request, as version 2 is; version 1 and 2 replies stay
byte-for-byte unchanged and existing stores keep loading. The trusted command
mode is opt-in per project; the current sandbox stays the default. The
[host contract](HOST.md) records each addition when it lands.

**Changed after M7 step 2:** Pi's commands are trusted by default, as Codex
and Claude Code run out of the box with Full access. The sandbox is opt-in per
project in the project's settings. Existing stores keep their trusted projects;
projects at the old default become trusted. No protocol or schema change.

M2.5's self-hosting work is done in source and host tests: ranged file reads,
retained command output with explicit limits (`read_output`), automatic
compaction that keeps full history across a host restart, and the
[development/install/continue workflow](SELF-HOSTING.md). What remains is
installed acceptance, listed in [Verification](VERIFICATION.md#v010-preview8-acceptance).

The release installer must install the prebuilt app and host without development
tools, preserve user data, and produce a per-user installation eligible for
normal in-app updates. Existing package installs migrate with one administrator
confirmation; active work must finish before migration or host replacement.

## Product structure

**Project → Machine → Thread.** A project can have a checkout on more than one
machine. Each machine owns its repositories, runtime, processes, and durable
state. A thread is a persistent conversation in one of those checkouts.

The client is an interface, not the owner of execution. Hosts must remain
independent, with no required central coordinator. Remote work needs authenticated
private-network transport; the planned Tailscale integration does not exist yet.
Do not expose the current local socket protocol over a public TCP listener.

Multiple threads do not imply isolated files. Worktrees stay optional, and
writers must coordinate around existing dirty work. Each thread is one agent
the person directs; Plait does not orchestrate agents or run swarms. There is
no embedded terminal or built-in cloud compute service.
Existing checkouts can be opened as projects.

**Built around the person's checkout.** An agent works in the checkout the
person already uses, beside their uncommitted work, their build output, and
their tools. Worktrees stay optional: safety must not depend on working in a
copy. What makes the checkout safe is the host, not the harness. The host keeps
its own record of the checkout around every turn (M9), so it can show what a
turn changed, and take it back, without relying on what the agent reports.

Plait also leaves the person's agent as they set it up. In Codex and Claude
Code threads it adds no instructions and no tools, and writes nothing into the
harness's configuration. It calls no model on its own to write titles, branch
names, or commit messages, and sends no telemetry. Running many agents at once
on copies of one repository is a different product, and Plait does not aim at
it: a feature that needs a fleet, a copy of the checkout, or a model call the
person did not ask for is out of scope.

**Agreed for M4 Git, ahead of M3:** Plait builds Plait, so committing reviewed
work without leaving the app comes before remote execution. The desktop may
stage, unstage, and commit, and show commit history and each commit's patch.
Reads keep their hardening (no filters, hooks, or optional locks). An explicit
write runs with the repository's own configuration, so staging runs clean
filters and committing runs hooks, exactly as the person's own Git would. One
write runs at a time. Push, fetch, discard, branch switching, amend, rebase,
and other history rewriting are not part of this step.

**Agreed next for M4 Git:** push, publish, and fetch, then hunk staging and
discard. Push runs as the person's own `git push` would; there is no force push.
Publish pushes a branch with no upstream to `remote.pushDefault`, `origin`, or
the only remote, and sets that as its upstream. Fetch is explicit, never on a
timer. Ahead and behind counts come from the status read Plait already makes.
Credentials stay with Git: the person's helpers and SSH keys apply, and the
GitHub CLI's credential helper is offered after them for that command only,
so Plait never reads a token or changes Git configuration. Discard always asks
for confirmation first. Pull is fast-forward only: Plait fetches, then moves
the branch to its upstream, and refuses a diverged branch or a pull that would
overwrite uncommitted changes. Merge and rebase are not part of this step.

**Agreed for M2.5 changed files:** a version 3 `changed_files` operation
returns the paths a thread's file tools wrote, from the host's existing
provenance records. The desktop counts their uncommitted lines with a hardened
Git read and lists them above the composer; Settings → Appearance hides the list.
Snapshots are unchanged, so older clients are unaffected.

**Agreed for M2.5 appearance:** light and dark themes in the desktop only.
Preferences gain an appearance (System, Light, or Dark) and a theme name for
each; older files load with the defaults. Base16 scheme files are read from a
themes folder beside the desktop preferences, and a few MIT-licensed schemes ship
with their license. Nothing changes in the host or the protocol.

**Shipped in v0.1.0-preview.20 for optional worktrees (Stage 7):** the
desktop requests one durable
host-owned worktree thread, with the chosen harness and model. Its branch is
`plait/<first-12-UUID-hex-digits>` (extended on collision), from an explicit
captured commit, under `<main-repository>.worktrees/`. New worktrees start cold.
The host retains creation receipts through retries, disconnection and restart.
Imported desktop associations group threads but cannot authorize removal.
Manual removal preserves branches/history, requires the recorded identity and
fresh lease-protected preservation checks, and never forces removal. Work
returns by commit, push and pull request; no integration command is added.
A busy project's choice of a worktree, and the project setting that makes it
automatic, are under [Thread tabs](#several-threads-in-one-project). The
checkout queue is planned.

**Agreed for M4 file viewer:** open any file in a checkout read-only, with
line numbers and the same syntax highlighting as the conversation, from a
quick-open picker and from paths the desktop already shows (changed files,
tool rows, diffs). Find within the file. Reads stay inside the checkout without
following symlinks, run off the UI thread, and are bounded in size; binary and
oversized files say so instead of drawing. An open file reloads when it changes
on disk. No editing in this step. Sidelit (same maintainer, MIT) is a source
for the reading, walking, and find logic; Plait's own views stay native GPUI.

**Direction for M7, any agent harness:** a thread may be run by another agent
harness instead of Pi: Claude Code, Codex, omp, Antigravity, and others with a
machine-readable interface. Plait's value is the workspace around the agent
(projects across machines, Git review, durable threads, approvals in one
place), so the agent itself should be the person's choice.

- **Adapters live in the host.** The client keeps one thread model: messages,
  reasoning, tool calls, approvals, status, usage. Each adapter runs one
  harness through its documented programmatic interface, at a pinned version
  checked against upstream before use, and translates its events into that
  model. Screen-scraping a terminal UI is out.
- **Each harness keeps its own identity.** Its authentication, configuration,
  and session files stay its own; Plait never reads its credentials. A thread
  records which harness and version ran it, and continues in that harness.
- **Say what protects the checkout.** Plait's command sandbox and approval
  policy govern Pi threads. Another harness runs under its own permission and
  sandbox model, which the thread names plainly; where the harness offers a
  permission callback, its requests appear as Plait approvals. Plait never
  implies a protection it does not enforce.
- **Durable on Plait's side.** The host stores the translated events, so
  history, search, and changed files work the same for every harness, and a
  restarted host shows the thread as it was without replaying work.
- **Faster than running it elsewhere.** Measure against the harness run
  directly: added latency to first token and per event, memory per idle
  thread, and frame cost while streaming. Targets: under 5 ms added per event,
  no full-snapshot resend per event (adapters send increments), idle threads
  holding no process, and the same 120 Hz reveal for every harness.

Before implementation: agree the protocol additions (harness choice when a
thread starts, capability flags, increments), the order of harnesses, and how
installs are found and pinned. Compare other agent workspaces by outcomes
only, per the originality policy.

**Agreed for M7 step 1, choosing a harness and Codex:** protocol version 4,
additive: older clients' replies are unchanged. A version 4 `harnesses`
request lists each harness with its state (ready, not installed, unsupported
version, signed out) and how to fix it, its version and supported range, its
capabilities (whether a message sent during a run joins it or waits, per-call
approvals, thinking levels, usage), and its models; Pi is always listed.
`create_thread` takes an optional `harness`, Pi by default; a thread stays in
its harness and records the harness and version of each run. Codex runs
through the person's installed `codex` (login-shell PATH or a path set in
Settings), checked against the version Plait's message types are generated
from: older is refused, newer runs with an untested note. One
`codex app-server` runs while any Codex thread runs, and none when all are
idle. Codex keeps its own sign-in, configuration, MCP servers, and
instructions; Plait reads none of its credentials. Plait sets Codex's sandbox
from the project's command policy (sandboxed: Codex workspace-write with no
network, so commands can write the checkout; trusted: full access) and its
approvals from the approval mode (ask: each request is a Plait approval;
auto: never asks), and says plainly that Plait's guard on already-modified
files does not apply. Streaming text, reasoning, tool rows, approvals, Stop,
steering, usage, model, and thinking level map to the same thread view. The
host stores translated events as they finish, so history, full text, and
changed files work as for Pi; an interrupted Codex run is never resumed or
repeated automatically. Clients keep today's snapshot updates; a replayed
Codex event stream is measured against the 5 ms per event and frame-cost
targets, and updates become increments only if those miss. The desktop groups
models by harness when a thread starts, names the harness, version, and
protection in the composer line, and lists harnesses in Settings. A scripted
fake app-server tests runs, approvals, steering, Stop, version checks, and
restart without a paid model.

**Agreed for M7 step 2, harness modes and Claude Code:** in Codex and Claude
Code threads the harness's own permission modes replace Plait's ask and auto,
and Plait's project command policy does not apply; Pi threads keep both. Codex
offers Read only, Ask (workspace sandbox), Approve for me, and Full access
(Codex's `--yolo`: never asks, no sandbox), Full access by default. Claude Code
offers Ask, Accept edits, Plan, Auto, and Bypass, Auto by default. Each
harness's default is a host setting in Settings → Agents. A thread's mode can
change at any time: Claude Code switches during a run, Codex from its next
turn. The composer shows the mode the harness reports, not the one requested,
and says plainly when nothing asks or nothing is sandboxed. Whatever a harness
asks becomes a Plait approval. Claude Code runs as the person's installed
`claude` through a pinned Agent SDK, loading their own settings, `CLAUDE.md`
files, skills, slash commands, hooks, and MCP servers; Plait never offers or
handles a Claude sign-in. Plait's releases do not include the SDK, whose license
reserves all rights: setting up Claude Code threads in Settings → Agents has the
host download that exact version from the npm registry and check it against
the pinned hash before use. Whatever sign-in the person's `claude` uses, a
Claude subscription, an API key, or a cloud provider, is accepted. Features
are checked from what the installed version
reports. Steering, Stop, usage, and stored history work as for Codex, and each
run resumes the thread's Claude session. This replaces step 1's mapping from
command policy and approval mode for Codex, before it is released.

Next, agreed before it starts: step 3, an ACP client, Antigravity first.

**Direction for M8, skills, tools, and CLIs:** today a Pi thread in Plait sees
only Plait's five tools (read, read_output, edit, write, command) and the
project's `AGENTS.md`. The skills, prompt templates, user-level instructions,
MCP servers, and CLIs a person set up for Pi are invisible to it, so the agent
is less capable in Plait than in a terminal. M8 closes that gap without
weakening what protects the checkout.

- **Skills, as the [Agent Skills](https://agentskills.io/specification)
  format defines them.** Discover `SKILL.md` directories in the locations Pi
  reads: the Pi agent directory, `~/.agents/skills`, and paths in Pi's
  settings, then the project's `.agents/skills` and `.pi/skills`. Only each skill's name, description, and
  location enter the instructions; the model reads the full `SKILL.md` and its
  bundled files when a task calls for it, through a read-only, bounded,
  no-symlink read limited to skill directories. `/skill:name` in the composer
  loads one explicitly. Skills that ship inside a repository are inert until
  the person trusts that project, as trusted commands are today.
- **Global instructions and prompt templates.** The person's user-level
  instructions (`AGENTS.md` or `CLAUDE.md` in the Pi agent directory) join the
  project's, under the same size bound. Prompt templates
  appear as `/` commands in the composer.
- **CLIs the agent knows about.** In trusted command mode, commands already run
  with the login-shell `PATH`, network, and real `HOME`, but the agent is not
  told which tools exist. Skills close most of that gap, since they name the
  CLIs they use. For sandboxed projects, the person can name CLIs that may run
  outside the sandbox (for example a deployment or search tool that needs the
  network and its own credentials); each call is still an approval showing the
  exact command, and the checkout stays read-only to it.
- **MCP servers before Pi extensions.** MCP servers from the person's Pi
  configuration run as separate processes, and their tools join the thread
  under Plait's approval policy, shown as their own tool rows. Large catalogs
  are found through tool search rather than listed in every request. Pi
  TypeScript extensions run arbitrary code in the runtime and have a different
  replay model, so they come after MCP, if at all. External tools are never
  repeated automatically after a host restart.
- **Visible and switchable.** Settings lists every skill, template, MCP server,
  and allowed CLI with where it came from, whether it loaded, and why not, as
  the themes list does. Each can be turned off per project. Plait never reads
  or copies a tool's credentials; they stay in the tool's own configuration.
- **Cheap.** Discovery runs on the host off the request path, is cached, and is
  rescanned when a thread starts only if its sources changed. Counts and file
  sizes are bounded. Only names and descriptions cost context until used.
- **Shared with M7.** Other harnesses load their own skills and MCP servers
  natively; Plait shows what they report rather than injecting its own.

**Agreed for M8 step 1, skills and user instructions:** runtime only, with no
client protocol change. Discovery uses Pi's own package resolution and skill
loader, never installs a package, and takes project skills only from projects
set to trusted commands. Skill files are read through the existing bounded,
no-symlink helper; sandboxed commands may read skill folders and never write
them. `/skill:name` sends Pi's block. Recorded in the
[host contract](HOST.md#skills-and-user-instructions).

**Agreed for M8 step 2, show what was found, and prompt templates:** a
version 3 `agent_resources` request returns, for one checkout, each skill and
template with its source and path, the user instructions file's path and
size, problems, and whether the project is trusted, never file contents, with
bounded lists and descriptions. It uses the same discovery as runs. `/name`
expands a prompt template with Plait's own expander following Pi's documented
argument rules; project templates need a trusted project, and the expanded
text is what the conversation shows, as in Pi. Typing `/` in the composer
lists skills and templates after Plait's own commands, which win a name
clash; choosing one inserts its text and runs nothing. The project's settings
list them read-only, with Open folder and Refresh; turning one off stays in Pi's
settings. Recorded in the [host contract](HOST.md#skills-and-user-instructions).

Step 4 now adds configured MCP tool calls; browser sign-in remains planned. Its details are agreed under "Agreed next,
for daily use". Step 3, named CLIs for sandboxed projects, is agreed and
waits until a sandboxed project needs it.

**Direction for M9, turn records:** the host records the checkout around every
run, for Pi, Codex, and Claude Code in every permission mode. This protection
does not depend on the harness reporting its edits. Step 1, per-turn undo,
shipped in v0.1.0-preview.15: recording, the host's undo operations, and their
desktop controls. None of it has been checked in the installed app yet; the
[verification record](VERIFICATION.md#turn-undo-desktop-controls) says what
was. Steps 2 and 3 are not built.

- **A run is the unit.** One submitted message, steering messages that join it,
  and all harness turns inside it share one before/after pair. A queued
  follow-up starts a new run and gets its own pair.
- **Kept by the host.** Records are commits in a private Git store, separated
  by object format, with a private index for each checkout. Only a run's after
  commit has its before commit as a parent. Runs are ordered in SQLite, not
  linked into an ever-growing Git history.
- **Read-only checkout.** Records never write the person's files, index,
  objects, refs, or Git metadata, including modification times. On macOS the
  recorder runs under a sandbox denying these writes. The store and checkout
  must not overlap. Repository objects may be borrowed for reads; writes go
  only to the host's store.
- **Exact content.** Records hold working-tree bytes, executable bits, and
  symlink targets with filters and line-ending conversion disabled. The host
  keeps its own copy of the before-content of each changed path. Other
  permissions, extended attributes, and hard-link relationships are not saved
  for restoration.
- **Cheap and off the UI thread.** A warm record of an unchanged 50,000-file
  checkout targets under 300 ms on Apple silicon. The
  [verification record](VERIFICATION.md#checkout-recording)
  separates measured costs from this target.

**Agreed for M9 step 1, per-turn undo:** the host makes a before record before
starting any harness and an after record after every outcome, including Stop
and failure, while retaining the checkout lease. The difference lists the
paths the run changed and lines added and removed per path; binary counts are
unknown. A created-then-deleted file leaves no difference.

- **Failure.** If the before record fails, the run ends with `record_failed`
  before its harness starts and its queue stays held. A failed after record
  makes the run unavailable for undo. After a host crash, recovery takes a
  missing after record in the background under the checkout lease, or at the
  next leased run if the checkout was busy. That record is marked “after
  restart”: it may include changes made while the host was down.
- **Undo this run.** Restore changed and
  deleted files and remove created files, only when each path still matches
  the run's after record. Paths changed since are never overwritten; the
  person chooses “Undo the rest” or “Cancel”. An undo is recorded and can
  itself be undone. Undo changes files, not Git history.
- **Presentation.** A finished run shows recorded file changes and
  line counts, including command writes, with Undo on its last closing line.
  Steering shares that control; a queued follow-up gets its own. Undone runs
  offer Redo. Changed-since conflicts open an inline panel with Undo the rest
  and Cancel; coverage and restart/HEAD warnings are explained on hover.
  The changed-files card uses record paths. Headless scenes cover these
  states; installed-app acceptance remains outstanding.
- **Order and release.** Shipped in v0.1.0-preview.15, ahead of M7's ACP
  client. Earlier releases record nothing, so a run made with an older host
  cannot be undone.

Agreed for implementation:

- **Coverage.** Tracked and untracked checkout files are recorded, including
  untracked directories file by file. Individually ignored files are included
  up to 1 MiB each, at most 2,000 files and 64 MiB total per record. A single
  file over 100 MiB is excluded. Ignored directories, nested repositories,
  submodules, and unreadable or unsupported files are not covered. Every
  omitted path or directory is listed, with its reason. Files observed changing
  during capture are also marked unavailable for undo.
- **Beyond files.** Outside-checkout effects, commits, branch changes, pushes,
  and effects beyond the machine are not undone. HEAD and its branch are
  recorded so movement can be reported.
- **Retention.** Keep pairs for 30 days, including archived threads; there is
  no thread deletion. Prune at startup and daily, with recording paused during
  pruning. Remove expired refs, stale private indexes, and unreachable private
  objects, including packed objects. No expired before record is removed while
  its run is active or its after record is still within retention.
- **Protocol.** Versions 1–4 keep their existing snapshots. Their only
  visible addition is the failed-before error (`runtime_error` to versions 1
  and 2). Listing records and undo are operations in protocol version 5,
  documented in the [host contract](HOST.md#protocol-version-5).
  Lists page within the frame; applied undos deduplicate by request ID, and
  interrupted operations are never replayed. The 10-second budget can leave a
  partial undo if a failure happens after restoring starts. The checkout lease
  excludes host writers; external-editor races remain an explicit limit. Older
  hosts offer no undo, and the desktop must say so.

Use existing tools for editing, interactive shells, and debugging; "Ways out"
below is how Plait hands off to them. A native mobile companion is a
possibility, not a current feature.

Build original application code and design. Compare other products by outcomes,
not as source or layout templates. See [the originality policy](ORIGINALITY.md).

## Agreed next, for daily use

Agreed on 2026-11-18, to make Plait usable for a full day's work without
another agent app beside it. Built in this order after M9 step 1, each on its
own and released as it lands. Pi, Codex, and Claude Code are the harnesses for
this work; the ACP client (M7 step 3) follows it.

**A native acceptance check.** A script drives a separate development copy of
Plait, with test data and its own host store, through what headless tests
cannot see: moving the window by its bar, double-click and full screen,
selecting text and following a link, typing and streaming, the approval
shortcuts, and the settings pages. It takes over the pointer, so a person runs
it when away, before a release. It is a development tool, not part of the app.

**M9 step 2, commit by turn — the first part shipped in v0.1.0-preview.17.** A
finished run's closing line now opens that exact record's saved before/after
change. Recorded Changes includes every retained path, even committed or clean
files, and remains bound after later runs and undo. Recorded/Current and
Original/Restored distinguish the comparisons. Additive host operations supply
immutable diffs without changing protocol versions 1–5.

Origins and staging by turn are not yet built. Their planned work is to show
which current changes match a run and stage independently safe changes while
preserving other work. Authorship wording and review semantics remain
unsettled; the recorded view displays no origin or review control. A recorded
interval by itself does not establish who wrote its bytes.

**M9 step 3, since you last looked.** Plait remembers, for each file, the record
the person last reviewed, and a file's diff can show only what changed since
then. A file that a later turn changes again is marked as changed, not hidden.

**Ways out.** Plait is not an editor or a terminal, so leaving for one is one
action. From a file, a diff, or a changed-file row: open it in the person's
editor at that line, or show it in Finder. From a project: open a terminal at
its checkout. The editor and the terminal are each a command the person sets
once in Settings, with `{file}`, `{line}`, and `{project}` filled in; ready
choices include the system default and Neovim in a Rex tab. Plait starts the
command and nothing more: it does not wait for it, read its output, or depend
on Rex.

**Branches.** From Changes: create a branch from the current commit and switch
to it, or switch to an existing local branch. Uncommitted changes are carried
as Git carries them, and a switch that would overwrite them is refused with
Git's own message. There is no force, no merge, no rebase, and no branch
deletion. These two are added to the Git writes Plait may make.

**Images in messages.** A PNG or JPEG pasted or dropped into the message box is
sent with the message when the thread's model accepts images, and shown in the
conversation. The host stores it with the thread. A model or harness that
cannot take images says so before sending. Shipped in v0.1.0-preview.17, in
[protocol 6](HOST.md#protocol-version-6): removable draft
thumbnails, a larger conversation view, private persisted originals, and
bounded transfers outside snapshots. PNG/JPEG only; four images, 5 MiB each,
5 MiB per message, 8,192 pixels per side and 16,777,216 pixels per image.
The pinned clipboard reader pastes text when text and an image coexist and
refuses TIFF-only images; the [Mac guide](MACOS.md#images-in-messages)
gives alternatives. Claude Code capability is unknown, so harness refusals
remain run errors. [Scripted and offscreen checks](VERIFICATION.md#images-in-messages)
cover the implementation; installed-app and live-model acceptance remain open.

**Search in a conversation.** Find in the open conversation, as Find works in
a file: matches in loaded messages, next and previous, and a count. Text that
has not been loaded from the host is not searched, and the bar says so.

**M8 step 4, MCP servers.** Pi threads use the MCP servers the person already
set up for Pi (`~/.pi/agent/mcp.json`, and a project's `.pi/mcp.json` only
when its commands are trusted). Plait starts only those servers, each for one
run, and lists them on the project's settings page with a switch for that
project.

- **One call per tool.** The agent finds a tool by search and each call is its
  own row in the conversation with its own approval, as MCP calls from Codex
  and Claude Code already are. Pi's code mode, where one script calls several
  tools, is not used.
- **Approval.** In a trusted project a call follows the thread's approval
  mode. In a sandboxed project a server is off until turned on for that
  project, and every call asks. A project-defined or replaced server is off
  even in a trusted project until the person reads its configuration and
  turns it on; the choice is bound to the unresolved definition, and a change
  switches it off again. Project overrides only narrow the user's enablement
  and tool visibility until explicitly enabled with the same binding. Once a
  server starts it can read the project and, even when sandboxed, use the
  network before a call is approved: approval gates calls, not server behaviour.
  What a server says about itself never skips an approval.
- **Sign-in.** A server that needs a key from the config file, or none, works
  at once. One that needs a browser sign-in is listed as needing a sign-in
  with Pi; once that is done Plait uses the same saved token. Plait has no
  sign-in of its own for MCP servers yet.
- **Never.** Plait does not start a server the person did not configure, does
  not send a server's secrets to the model or the desktop, declines a server's
  requests to sample the model or question the person, and does not repeat a
  call after a host restart. A configuration supplied by a repository is not
  the person's choice until they explicitly enable its reviewed definition.

**Built for step 4:** bounded configuration discovery and project switches;
run-scoped, guardian-owned stdio and header-key/anonymous HTTP connections;
deferred search and one row per tool; exact-call approval under both project
policies; bounded output; secret redaction; sampling/elicitation refusal; and
no replay after restart. Settings shows observed counts and connection problems.
Browser OAuth sign-in, reuse/refresh of Pi's tokens, and provider-backed `auth`
remain planned. Verification uses scripted stdio and loopback HTTP fixtures,
not real servers or the installed app.

**M8 step 3, named CLIs, is agreed but waits.** It matters only in a project
the person has sandboxed: a trusted project, the default, already runs any
tool as the person. It is built when a sandboxed project needs a tool with
network access or the person's own credentials.

## Thread tabs

**Shipped in v0.1.0-preview.18:** one thread per tab, a 40-point top row that stays visible with the
sidebar hidden, a 36-point row for the thread's Conversation, Changes, History
and Files views, and a state-grouped overflow list. Sidebar clicks open or
activate a thread's tab. Closing never stops a run or removes a worktree.
Closing the last tab leaves an empty workspace. Settings and Project settings
open as window modals over the unchanged tab rows and content. Tab controls
remain inactive until the modal closes.

**Shipped in v0.1.0-preview.20:** every New tab or New thread action appends
an independent, empty draft, including in the same checkout; creating its
thread replaces that draft in place. There is no ceiling on open tabs. Only
visible tab-strip and overflow rows are drawn.

Cmd-1 through Cmd-8 select positions; Cmd-9 selects the last tab. Cmd-W closes
a tab, Cmd-Shift-W closes the window, Cmd-Shift-T reopens one of the last twenty
closed tabs, and Cmd-Shift-[ / Cmd-Shift-] cycle. Sidebar jumps use
Cmd-Option-1 through Cmd-Option-9. Tabs can be dragged or moved with
Control-Shift-Left / Control-Shift-Right.

Only the active workspace retains loaded transcripts, projections, files,
diffs, images and its snapshot subscription. Cold tabs keep bounded bookmarks;
editor history spills to private records. Background reads retain their owner
and visibility guards, while accepted mutations finish into their owner ledgers.
Tab order, active identity and bookmarks are saved separately in `tabs.json`,
with debounced atomic writes and total decoding after host and checkout
verification. Drafts and submissions remain in the existing recovery journal.

### Several threads in one project

**Shipped in v0.1.0-preview.20:** stages 6a, 6b, 7 and 8 below. A checkout
still has one writer at a time. A new thread in a busy project is offered its
own worktree, and a project can make that automatic.

**Stage 6a, write exclusion.** Every Git write Plait makes (staging
and unstaging by file or hunk, committing, discarding and restoring a discard,
creating and switching branches, pushing, publishing, fetching, pulling, turn
undo and redo, adding and removing a worktree) and every run take one ordered
set of OS locks: a per-user lifecycle lock for each checkout (with a reader
class tested with a fake reader, no reader run shipped), the existing writer
lock on the checkout directory, and a lock on the repository's shared Git
metadata so linked worktrees take turns. One change is visible: a desktop Git
write while a thread is running in the same checkout is now refused with a
note (staging and committing used to go ahead with a warning; branch controls
were already locked), and a message sent during such a write is refused as
busy. This stage added no control and no Git verb.

**Stage 6b, cleanup confirmation.** Run and undo admission persist inode-keyed cleanup markers; unconfirmed cleanup
blocks both runs and desktop Git changes across host restarts and stores.
Command/MCP/record guardians verify normal-group absence, Claude cleanup waits
for actual process exit, and a Codex interrupt timeout retains exclusion until
a terminal acknowledgement or verified server exit. Desktop Git writers and
lent worktree children keep separate crash records; orphaned groups block new
work, and a verified group can be stopped explicitly. Recording adds no helper
process or fsync to a Git write. Late evidence clears the
state automatically; otherwise the person uses **Stop unconfirmed** → **Confirm
stopped** after checking remaining processes. Deliberately detached writers in
trusted and external-harness runs remain an explicit limitation. Clean runs
are unchanged; unrelated checkouts keep working. Scripted process tests and
native offscreen scenes cover this behavior; live harness and installed-window
acceptance are separate. See [the cleanup contract](HOST.md#cleanup-confirmation-and-quarantine).

**Stage 7, host-owned worktrees.** The host owns explicit worktree creation and removal. A creation request fixes
its parent directory identity, common Git directory identity, base commit and
harness/model/permissions before any checkout is made. Its durable request ID
allocates one thread UUID; the branch is `plait/` plus the first twelve UUID hex
digits, extended on a name collision. The checkout lives beside the main
repository in `<repository>.worktrees/`, including when requested from a linked
worktree. It starts cold, with no build artifacts or dirty files copied.

Creation, status, listing and removal use additive operations, leaving protocol
versions 1–6 unchanged. Older hosts explicitly refuse the new operations.
Desktop worktree associations remain imported presentation metadata and never
grant deletion authority. Removal is manual, preserves branches and history,
and refuses changed identities, active leases, uncommitted or unknown ignored
files. Unpushed or unknown publication status requires an explicit preservation
exception. No force removal, branch deletion, integration verb or automatic
cleanup is added.

**Stage 8, busy-project choice and capacity.** New drafts negotiate placement before the first run. A busy project asks each
time whether to use an own worktree; the project modal can authorize that choice
automatically for that project. Wait is visibly unavailable until the durable
checkout queue ships. Existing threads remain bound to their checkout and can
explicitly start a linked continuation with a fresh harness session. The original
history and draft remain intact. Capabilities are additive and strictly probed.

Worktree creation retains the agreed `plait/<UUID-prefix>` branch naming and
explicit HEAD commit base. It excludes original dirty files and build outputs,
shows the actual checkout and branch, and never removes worktrees automatically.
Commit, push and pull request remain the integration path. No merge, rebase,
cherry-pick, shared live reader, artifact cloning or fake queue is added.

The fixed eight-checkout cap is removed. There is no default run-count ceiling;
a host can set `PLAIT_MAX_ACTIVE_CHECKOUTS`. Before-record preparation is
serialized by filesystem and resource exhaustion fails explicitly with cleanup.
Scripted harness tests cover twenty simultaneous independent writers, same-root
exclusion, preparation cancellation, lost replies and resource failures. Live
provider limits and native focus/accessibility behavior still need separate
acceptance; headless tests do not claim those results.

**Built next: Worktrees.** One project graph per window is the first tab content
that is not a thread. It opens from the sidebar project menu, command palette,
Project settings and a worktree thread's context line. The graph presents actual
commit ancestry, host-owned preservation facts, bounded cached storage accounting
and manual safe removal. External and missing worktrees remain visible; removed
worktrees retain their private cache attribution. Stage 12's visibility and
manual checkout removal are built; automated cleanup and cache deletion are not.
Headless scenes, lifecycle tests and a fifty-worktree frame case cover this view;
installed-window acceptance and performance timings remain separate verification.

**Still planned:** further tab content kinds, a durable checkout queue (Stage
9), enforced Pi read-in-place (Stage 10), copy-on-write artifact seeding
(Stage 11), and further worktree storage management (Stage 12).


**Agreed concurrency decisions (built in the stages named below):**

- **Busy-project choice (Stages 8 and 9):** when a project is busy and a new
  thread is started there, Plait offers a choice between its own worktree or
  waiting in a checkout queue; a per-project switch can make the worktree
  automatic, never automatic everywhere. Stage 8 shipped the worktree choice
  and the switch; waiting is shown, disabled, until Stage 9.
- **Reading in place (Stage 10):** sharing a busy checkout to read while another
  thread writes comes later, and only as a host-enforced mode for Pi. Until
  then, all runs are writers (with the lifecycle lease prepared and tested for a
  reader class, shipping none).
- **Worktree build artifacts, cleanup, and returning work (Stages 11 and 12):**
  a new worktree's build output is cloned copy-on-write only from a per-project
  approved list; worktree cleanup is manual first (Stage 12a), then an opt-in
  rule (Stage 12b). This view supplies the manual part; work returns by commit,
  push, and pull request, with no
  merge, rebase, or cherry-pick added.

The agreed v2 recovery index/blob layout remains planned, with its own
compatibility review and retained v1 backup. The shipped v1 journal is limited
to 4 MiB serialized and 256 KiB per draft. Its additive `draft_tabs` field keeps
independent drafts and pending first sends; legacy snapshots stay readable by
older builds and retain their 512-record bound. The new field decodes at most
10,000 drafts; tab preferences decode at most 10,000 entries and 16 MiB. Twenty maximal drafts
and a separate 4 MiB resident submission cache are not supported capacity
promises. At a limit, the last good journal remains intact; required saves
refuse submission or closing instead of discarding current work.

- **Agreed proposed host capabilities (for the concurrency stages).** Use additive operations that omit `protocol_version` where existing reply
  shapes are unchanged, with explicit capability flags
  (`worktree_threads_v1`, `checkout_queue_v1`, `execution_class_v1`,
  `worktree_inventory_v1` (built), `worktree_storage_v1`, and per-harness `shared_reader`), preserving
  backward compatibility with older hosts and single-writer checkout behavior
  until each capability is complete.

## Agreed next: Linear

Native Linear integration is agreed and ordered in three steps. Nothing is
visible in the desktop app yet, and nothing has been tried against Linear's
live API. See [Linear integration](LINEAR.md).

1. **Host foundation (built in `runtime/`).** Additive host operations for
   pasted API-key sign-in and sign-out (stored in a private `0600` file beside
   `host.sqlite`, never in the database or any reply), listing teams and
   workflow states, binding a project checkout to a team, bounded paged issue
   lists and details with SQLite caching and offline fallback, local
   thread-to-issue links, and two explicit person-initiated writes (post a
   comment on an issue, and move an issue to another workflow state in its
   team). Nothing writes automatically, no agent, tool, or MCP server can call
   these host operations, there is no background polling, and the protocol
   version is unchanged. Browser OAuth sign-in is planned for later.
2. **First desktop step (planned, after thread tabs).** Enter the Linear API
   key in Settings, pick an issue to start a thread from, and show the linked
   issue on its thread as a label. There is no Linear tab in this step.
3. **Linear view and issue-triggered runs (planned, rules not yet agreed).** A
   dedicated Linear view arrives together with issue-triggered runs, as the
   place where unattended work waits for review. The rules for issue-triggered
   runs are not agreed yet—which issues are eligible, what a run may do with
   nobody there, and where it stops—and they require rewording Plait's rule
   against agent orchestration, which has not been done.

An agent can still use Linear's own MCP server through Plait's MCP support,
but MCP is not the path Plait's native Linear features take.

## Proposed, not yet agreed

The later steps of M9, each agreed before it starts. All of them read the turn
records described under "Direction for M9" above. None adds a write to the
person's repository beyond the staging and committing Plait already does.

**Step 4, checks the host runs.** After a turn, the host can run a check the
person named for the project, such as a build or a test run, and show its
result on the turn beside whatever the agent said. It runs in the command
sandbox, so it can read the checkout and cannot write it or reach the network.
It is off until the person turns it on for a project, runs at low priority and
never in a frame, and is cancelled when a new turn starts. Nothing is sent back
to the agent unless the person sends it.

**Later, from the same records:**

- **Line to turn.** From a line in the file viewer or a diff, to the turn that
  wrote it and the message that asked for it.
- **A fuller account of each turn.** The branches and `HEAD` it moved, with a
  way to put them back; each command's exit status; processes it left running;
  and files that changed outside the checkout while it ran.
- **A shelf.** An undone turn can be put back later, and a message can be tried
  again with another model before the person keeps one result.
- **Turn bisect.** Which turn broke a check, found by running it on recorded
  trees in the sandbox while the checkout stays as it is.
- **Run the proof.** A command that shows, on the person's own Mac, the sandbox
  refusing what it says it refuses and an undo restoring identical files.

**Ideas, not yet shaped.** Wanted, but none is designed or agreed, and each
names what has to be settled first.

- **Threads, files, and Git side by side.** One window that shows a
  conversation, a file, and the working tree's changes together, instead of
  the tabs that replace the conversation today. To settle: what it shows at
  900 points, and whether it replaces the Git inspector or grows from it.
- **Hosts that Plait runs for you.** A host in the cloud, reached like any
  other machine, for people who do not want to run their own. It could be how
  Plait pays for itself. It needs remote hosts first (M3), and a position on
  what a hosted checkout may hold and who can read it.
- **Automations.** A thread that starts by itself: first on a schedule, later
  also when something happens in Git or when an issue arrives. An automation
  is created with everything its runs need (the project, the harness and
  model, the message, and its permissions), because nobody is there to approve
  a step. The host would start it, since the host already outlives the window.
  Today nothing starts by itself, and the
  [host contract](HOST.md#internal-authorization-boundary) requires a
  permission design before that changes. To settle: where a run's results wait
  for the person and how they are told; what a run does when a step needs more
  than the permissions it was created with; what happens to a run whose time
  came while the Mac was asleep or the host was down; and how a run that
  starts in a checkout the person is working in shares it with them.
- **A browser.** See a page the work produced, or one the agent needs, without
  leaving the window. Previews (M5) are the planned first step. To settle:
  whether the agent can drive it, which changes what a thread can reach.
- **Rendered Markdown.** Read a Markdown file as a document, not as source.
  Part of previews (M5).
- **Closer work with Rex.** Opening a file or a terminal in Rex is built. What
  more would help is not known yet. Candidates: open a file in a Neovim that
  is already running instead of a new tab; send a thread's command to a named
  pane; a link from a Rex pane back to its thread.
- **Usage.** A page that says how much the agents have used: at first only
  the number of tokens over the last so many days, for all threads together.
  Later, perhaps by harness, model, project, or thread. It is counted on the
  host from what each harness reports, kept on the person's machine, and sent
  nowhere. To settle: what Pi, Codex, and Claude Code each report and whether
  the counts can be compared; where the host keeps a daily total, which is a
  change to what it stores and reports; and whether to show a cost, which
  needs prices that change.
- **An editor.** Edit a file in Plait, not only read it. The planned file
  editor (M4) is not designed. To settle: how much editing is wanted beyond
  the hand-off to the person's own editor; and whether it is native, built on
  the file viewer, or a web editor component, which would bring a web view
  into an app whose interface has none.

## Performance goals

Targets from the product brief. Headless frame-cost measurements, which
exclude GPU and the installed app, are in
[Verification](VERIFICATION.md#measuring-frame-cost); installed-app results
are not yet measured:

- 60 FPS or better for normal local interaction.
- Cached thread switching and Git views below 50 ms.
- Warm launch below 250 ms.
- Incremental remote events without blocking input.

Keep filesystem, Git, and network work off the UI thread. Use cached state,
background work, and virtualized views rather than unnecessary polling or full
view rebuilds. The [ADEMark plan](ADEMARK.md) records the measurement boundary;
Plait does not have an ADEMark score.

Results are published only from the installed app, with the conditions they
were measured under; goals and headless figures are not presented as results.
ADEMark measures Mac agent apps with Claude Code at full access, which a
release with Claude Code threads makes comparable, so entering it is planned.
It also records what an app adds to the agent's request, the model calls it
makes on its own, what it writes into the agent's configuration, and what
keeps running after quit. Plait means to keep the first three at nothing. The
host is meant to keep running, so its memory after quit is reported, not
hidden.
