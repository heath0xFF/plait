# Product roadmap

These are planned milestones, not a list of shipped features. See the
[README](../README.md) for the current preview and [Verification](VERIFICATION.md)
for acceptance evidence and open checks. **M2.5 shipped in v0.1.0-preview.8.
M8 steps 1 and 2 (skills, prompt templates, and user instructions) shipped in
v0.1.0-preview.9. M7 steps 1 (choosing a harness, and Codex) and 2 (each
harness's own permissions, and Claude Code threads) shipped in
v0.1.0-preview.10. M9 steps 1 to 3, M8 steps 3 and 4, and the rest of
"Agreed next, for daily use" below are the current work.**

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
| M8 — Skills, tools, and CLIs | Pi threads use the skills, prompt templates, global instructions, MCP servers, and command-line tools the person already set up for their agent, with each one visible in Settings and every call under Plait's approvals | Steps 1 (skills and user instructions) and 2 (prompt templates, Settings → Agent, and `/` completion) shipped in v0.1.0-preview.9; steps 3 (named CLIs) and 4 (MCP) follow M7 |
| M9 — Turn records | The host keeps its own record of the checkout before and after every turn, in every harness: per-turn undo first, then committing by turn, review of what changed since the person last looked, and checks the host runs itself | Steps 1 to 3 are agreed and are the current work, step 1 first, ahead of M7's ACP client; the later steps are proposed |

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

**Agreed for M2.5 worktrees:** a thread may start in its own worktree, never by
default. The desktop runs `git worktree add` from HEAD on a new `plait/<name>`
branch, in a `<repository>.worktrees/` folder beside the repository, and starts
the thread with that folder as its checkout. The host treats the worktree as
any other checkout: its own lease, run limit, and sandboxed command policy.
Sandboxed commands there may read the main repository's Git directory, and
still cannot write it. Its threads list under the project it came from.
Removal asks first and uses `git worktree remove`, which refuses uncommitted
work; the branch and its commits stay. Merging the branch back is left to the
person's own tools.
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

Next: step 3 adds named CLIs for sandboxed projects; step 4 adds MCP servers.
Both are agreed to be built (see "Agreed next, for daily use"); the details of
each are agreed before it starts.

**Direction for M9, turn records:** Plait's guard on files that had changes,
its exact approvals, and its command sandbox protect a checkout only in Pi
threads. Codex and Claude Code write wherever their own permissions allow, and
most people run them without prompts. M9 gives every harness one protection
that depends neither on prompts nor on the harness: the host records the
checkout itself.

- **A record before and after every turn.** Each record is the working tree as
  Git would store it: every tracked and untracked file that Git does not
  ignore. Records are commits in a chain. The difference between a turn's two
  records is what that turn changed, whichever tool or command changed it. The
  difference between one turn's end and the next turn's start is what the
  person, or anything else, changed in between.
- **Kept by the host, not in the repository.** A private copy of the index
  takes the files, and the resulting objects are written to the host's store,
  which reads the repository's own objects without adding to them. The person's
  repository gains no objects and no refs; their index, `HEAD`, branches, and
  files are only read.
- **Undo first, then review.** The same records give per-turn undo (step 1),
  then staging and committing by turn, review of what changed since the person
  last looked, and checks the host runs itself. Each later step is agreed
  before it starts.
- **The same on every machine.** Records are made by whichever host runs the
  turn, so a remote host (M3) gives the same undo and review as a local one.
- **Cheap.** A record of an unchanged checkout costs about one pass over its
  file metadata. It is measured against an agreed target on a large checkout
  before shipping, and runs on the host's background work, never in a frame.

**Agreed for M9 step 1, per-turn undo:** any turn, in any harness and any
permission mode, can be undone, so the person's work is recoverable even when
an agent ran with full access. This is the same promise for every harness,
made by the host rather than the harness.

- **Before each turn,** the host records the checkout as above. The turn
  starts only after the record is made, and a turn whose record fails says so
  before it starts rather than running unrecorded.
- **After the turn,** the host records the tree again and keeps the list of
  paths that differ.
- **Undo this turn** puts those paths back as they were before the turn:
  changed files restored, created files removed, deleted files returned. A
  path the person or a later turn has changed since is not overwritten: the
  undo lists it and stops until the person chooses. An undo is recorded first,
  so it can itself be undone.
- **Shown** on each finished turn with its file list and size, beside changed
  files, for every harness. Changed files then includes what commands wrote,
  which Codex and Claude Code threads do not list today.
- **Not covered, and said so in the app:** files Git ignores (such as `.env`
  files and build output), anything outside the checkout, commits, branch
  changes, and pushes the agent made, and effects beyond the machine. A turn
  that moved `HEAD` says so; undo restores files, not history.
- **Writes.** Undo's restore of working-tree files is the only write to the
  person's checkout, and there is no Git write to their repository. Records
  are pruned with the thread and after an age to be agreed.
- **Order.** Undo is built before M7 step 3, so each harness added later starts
  with it.
- **Release.** Codex and Claude Code threads shipped in v0.1.0-preview.10
  without undo, and v0.1.0-preview.11 does not add it. Until undo ships, a
  turn in those threads cannot be undone.

Agreed for implementation:

- **Ignored files.** A file ignored one at a time, such as `.env`, is recorded
  when it is at most 1 MB. Ignored directories, such as build output, are
  not recorded, and a turn's record says which paths it does not cover.
- **Exact bytes.** A record holds the working-tree bytes as they are, with Git
  filters (such as LFS) and line-ending conversion off, so an undo writes back
  exactly what was there.
- **Its own copy.** The host's store holds its own copy of the earlier content
  of every path a turn changed, so pruning or rewriting history in the
  repository cannot break an undo.
- **Kept** until the thread is deleted, or 30 days after the turn, whichever
  comes first.
- **Cost.** Recording an unchanged checkout of 50,000 files takes under 300 ms
  on an Apple silicon Mac, measured by a test before it ships.
- **Protocol.** Listing a thread's records and undoing a turn are new host
  operations in a new protocol version, named in the
  [host contract](HOST.md) when they are built. An older host offers no undo,
  and the desktop says so.

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

**M9 step 2, commit by turn.** Changes shows, for each changed hunk, whether the
person made it or which turn did. A turn's changes can be staged on their own,
leaving the person's work in progress unstaged. A hunk that no longer matches
the turn's record is refused, as a hunk that moved is today. A finished turn's
closing line opens what that turn changed.

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
cannot take images says so before sending. This changes the host protocol,
recorded in the [host contract](HOST.md) when built.

**Search in a conversation.** Find in the open conversation, as Find works in
a file: matches in loaded messages, next and previous, and a count. Text that
has not been loaded from the host is not searched, and the bar says so.

**M8 steps 3 and 4, named CLIs and MCP servers.** As described under
"Direction for M8": Pi threads use the command-line tools and then the MCP
servers the person already set up for Pi, each listed in the project's
settings and each call under Plait's approvals.

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
