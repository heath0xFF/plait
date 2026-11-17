# Product roadmap

These are planned milestones, not a list of shipped features. See the
[README](../README.md) for the current preview and [Verification](VERIFICATION.md)
for acceptance evidence and open checks. **Current work is M2.5 and the Git half
of M4.**

## Milestones

| Milestone | Scope | Status |
| --- | --- | --- |
| M1 — Native foundation | GPUI desktop, navigation, conversations, read-only Git status and diffs | Foundation implemented |
| M2 — Local agents | Real agent events, approved edits and commands, persistent threads, safe Stop and reconnect | Shipped in the local preview; hardening and live installed acceptance continue |
| M2.5 — Daily driver | Complete, paged conversation history; projects and threads across checkouts with live status; thread titles, rename, and archive; model and thinking changes within a thread; follow-up and steering messages during a run; provider sign-in from the app; selectable, highlighted conversation text and readable tool results; a command palette; optional per-thread worktrees; an opt-in trusted command mode | Current work; not shipped until the README says so |
| M3 — Remote execution | Authenticated host connections, remote project discovery, activity/Git updates, network recovery | Planned; Linux execution requires tested containment first |
| M4 — Development tools | Git staging, commits and history; file browser/editor; review feedback; process and log views | Git staging, commits and history in progress, ahead of M3; the rest planned |
| M5 — Previews | Live HTML, Markdown and MDX, automatic refresh, and selected-content feedback to the agent | Planned |
| M6 — Multiple machines | Unified project navigation across independent hosts, including disconnected/reconnecting states | Planned |

Do not build the whole roadmap at once. Each change must compile, run, and have
its own checks. Protocol, storage, and shared-contract changes need agreement
before implementation.

**Agreed for M2.5:** protocol version 3 and additive storage changes. Version 3
is negotiated per request, as version 2 is; version 1 and 2 replies stay
byte-for-byte unchanged and existing stores keep loading. The trusted command
mode is opt-in per project; the current sandbox stays the default. The
[host contract](HOST.md) records each addition when it lands.

The remaining M2.5 self-hosting work is ranged file reads, retained command
output with explicit limits, long-session compaction and restart coverage, and
the complete development/install/continue workflow. Additive runtime tool
schemas and private output storage are in scope; existing client protocol
versions and the default sandbox policy remain compatible.

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
