# Product roadmap

These are planned milestones, not a list of shipped features. See the
[README](../README.md) for the current preview and [Verification](VERIFICATION.md)
for acceptance evidence and open checks. **M2.5 shipped in v0.1.0-preview.8.
Current work is M8: the person's own Pi skills, instructions, and tools, then
M7.**

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
| M7 — Any agent harness | Threads run by other agent harnesses (Claude Code, Codex, Pi, omp, Antigravity, and others) beside Pi, with the same conversation, approval, Git, and navigation views | Direction agreed; does not depend on M3. Protocol details need agreement before implementation |
| M8 — Skills, tools, and CLIs | Pi threads use the skills, prompt templates, global instructions, MCP servers, and command-line tools the person already set up for their agent, with each one visible in Settings and every call under Plait's approvals | Current work. Step 1 (skills and user instructions in the runtime) is in source; the Settings list, prompt templates, CLIs, and MCP follow |

Do not build the whole roadmap at once. Each change must compile, run, and have
its own checks. Protocol, storage, and shared-contract changes need agreement
before implementation.

**Agreed for M2.5:** protocol version 3 and additive storage changes. Version 3
is negotiated per request, as version 2 is; version 1 and 2 replies stay
byte-for-byte unchanged and existing stores keep loading. The trusted command
mode is opt-in per project; the current sandbox stays the default. The
[host contract](HOST.md) records each addition when it lands.

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
writers must coordinate around existing dirty work. There is no embedded terminal,
mandatory agent orchestration, automatic swarm, or built-in cloud compute service.
Existing checkouts can be opened as projects.

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
Git read and lists them above the composer; Settings → View hides the list.
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
installs are found and pinned. Compare other multi-agent workspaces by outcomes
only, per the originality policy.

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

Next, each agreed before it starts: step 2 reports what was found (a version 3
request listing skills, templates, and problems) for a Settings list and
composer completion, and adds prompt templates; step 3 adds named CLIs for
sandboxed projects; step 4 adds MCP servers.

Use existing tools for interactive shells and debugging. A future Open in Rex
integration and native mobile companion are possibilities, not current features.

Build original application code and design. Compare other products by outcomes,
not as source or layout templates. See [the originality policy](ORIGINALITY.md).

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
