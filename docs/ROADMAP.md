# Product roadmap

These are planned milestones, not a list of shipped features. See the
[README](../README.md) for the current preview and [Verification](VERIFICATION.md)
for acceptance evidence and open checks. **Current work stays within M2.**

## Milestones

| Milestone | Scope | Status |
| --- | --- | --- |
| M1 — Native foundation | GPUI desktop, navigation, conversations, read-only Git status and diffs | Foundation implemented |
| M2 — Local agents | Real agent events, approved edits and commands, persistent threads, safe Stop and reconnect | Current local preview; hardening and live installed acceptance continue |
| M3 — Remote execution | Authenticated host connections, remote project discovery, activity/Git updates, network recovery | Planned; Linux execution requires tested containment first |
| M4 — Development tools | Git staging, commits and history; file browser/editor; review feedback; process and log views | Planned |
| M5 — Previews | Sidelit integration for live HTML, Markdown and MDX; select content for agent feedback | Planned |
| M6 — Multiple machines | Unified project navigation across independent hosts, including disconnected/reconnecting states | Planned |

Do not build the whole roadmap at once. Each change must compile, run, and have
its own checks. Protocol, storage, and shared-contract changes need agreement
before implementation.

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
Use existing tools for interactive shells and debugging. A future Open in Rex
integration and native mobile companion are possibilities, not current features.

Build original application code and design. Compare other products by outcomes,
not as source or layout templates. See [the reference policy](UI-REFERENCES.md).

## Performance goals

Initial targets from the product brief, **not measured results**:

- 60 FPS or better for normal local interaction.
- Cached thread switching and Git views below 50 ms.
- Warm launch below 250 ms.
- Incremental remote events without blocking input.

Keep filesystem, Git, and network work off the UI thread. Use cached state,
background work, and virtualized views rather than unnecessary polling or full
view rebuilds. The [ADEMark plan](ADEMARK.md) records the measurement boundary;
Plait does not have an ADEMark score.
