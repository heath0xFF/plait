# Plait

**A fast, native development environment for AI coding agents.**

Your projects. Every machine. Every thread.

**Early local preview:** the native desktop has a local host client, composer,
model picker, and read-only Git views. Local edit, command, Stop, reconnect, and
full sandboxed workspace tests pass on macOS. Review every approved action;
Linux execution and remote Plait connections are not supported yet. See
[verification](docs/VERIFICATION.md) and the [Arthur handoff](docs/HANDOFF.md).

[Download previews](https://github.com/heath0xFF/plait/releases) · [Install and update](docs/MACOS.md) · [Build Plait with Plait](docs/SELF-HOSTING.md) · [Build and run](#run-the-desktop) · [Contribute](CONTRIBUTING.md) · [MIT license](LICENSE)

Plait is an agent-native development environment designed around persistent coding agents, distributed execution, and first-class Git workflows.

Its design joins clear agent conversations, first-class development tools, and independent machine hosts. Plait's application code and design are original; competitors are comparison points, not implementation sources.

Plait is not a terminal emulator, a traditional IDE, or an agent orchestration platform.

## Philosophy

Development environments should adapt to how developers work, not require developers to adopt an opinionated orchestration workflow.

Plait is built around several principles:

1. **Blazing fast.** The interface should feel instantaneous. Performance is a feature, not an optimization phase.
2. **Agent-native.** Conversations with coding agents are the primary interaction model.
3. **Distributed by default.** Execution happens on independent machines. Clients are interfaces, not owners of execution.
4. **Git is fundamental.** Changes, diffs, commits, history, and review are first-class capabilities.
5. **No mandatory worktrees.** A project is a repository or working directory, not a collection of isolated agent tasks.
6. **Small, composable tools.** Don't reproduce capabilities better handled by existing tools.
7. **Developer in control.** No mandatory agent orchestration, automatic swarms, or hidden execution.

## Core hierarchy

**Project → Machine → Thread**

### Project

A logical development project that may exist on multiple machines.

A project contains machine-specific checkouts, Git state, and agent conversations.

### Machine

An independent execution environment, such as a local MacBook, Linux workstation, or cloud VM.

Each machine manages its own repositories, agent runtimes, processes, and persistent state.

Machines operate independently. There is no mandatory central coordinator.

### Thread

A persistent agent conversation belonging to a project on a specific machine.

Threads survive client disconnections and should recover safely from process failures.

A thread contains its conversation history, tool execution, status, and associated development activity.

Multiple threads may exist within a project, but they are not automatically isolated into worktrees. Concurrent filesystem writes must be coordinated.

## Technology

| Component | Technology |
|---|---|
| Desktop client | Rust + GPUI |
| Host daemon | Rust |
| Agent runtime | Pi Durable (TypeScript sidecar) |
| Git | Rust interface to Git CLI initially |
| Networking | Authenticated RPC and event streaming over Tailscale |
| Persistence | Machine-local durable storage |
| Preview | Sidelit-inspired HTML, Markdown, and MDX |
| Mobile | Future native companion |

Dependencies should be pinned to tested versions or revisions.

Pi Durable is experimental; its APIs, authentication integration, and compatibility with regular Pi extensions must be validated before committing to its use.

## Application interface

### Sidebar

The primary navigation follows Project → Machine → Thread.

Projects are collapsible, machines show connection status, and threads show execution state.

### Main workspace

The main workspace supports:

- Rich, streaming agent conversations
- Structured tool calls and results
- Native file editing
- Git status, diffs, staging, commits, and history
- Inline review comments sent to the agent
- Process and log inspection
- Live HTML, Markdown, and MDX previews

The interface should make real work clear and easy to review. Do not copy competitor code, assets, or distinctive UI; compare task outcomes and measured performance instead.

### Git

Git is a foundational subsystem, not an optional plugin.

Support staged and unstaged diffs, inline and side-by-side views, partial staging, commits, branches, and history.

Git state belongs to each machine-specific checkout.

Never automatically create worktrees. Prefer one active writing agent per checkout, and account for existing dirty working trees.

### Terminal

Plait does not contain an embedded terminal.

Rex remains the preferred terminal, shell, and interactive debugging environment.

A future **Open in Rex** integration should open the appropriate machine and working directory.

Plait still supports process execution and structured logs through its host daemon.

### Orchestration

Orchestration is optional; coordination is foundational.

Plait's core abstraction is an independently executable, durable agent thread. Threads have stable identities, explicit ownership, structured lifecycle events, and addressable communication endpoints.

Future orchestration systems may coordinate threads across projects and machines without changing the underlying execution runtime or requiring a central controller.

Independent execution, Git safety, and user control must remain intact.

## Distributed architecture

Each machine runs an independent host daemon responsible for:

- Project and checkout discovery
- Agent execution and lifecycle
- Durable thread persistence
- Git operations
- Filesystem access
- Process execution and logs
- Authentication and authorization

Desktop clients connect directly to machine daemons.

A host must not depend on the desktop client remaining connected to continue agent work.

Remote disconnections must not interrupt agent execution.

Only approved private-network clients may access the host API.

## Performance requirements

Plait must prioritize perceived responsiveness.

Initial engineering targets, to be measured and refined:

- Local UI interactions: 60 FPS or better under normal load
- Cached project/thread switching: under 50 ms
- Cached Git views: under 50 ms
- Warm application launch: target under 250 ms
- Remote events: streamed incrementally without blocking interaction

All filesystem, Git, and network work must run asynchronously relative to the UI.

Use efficient rendering, virtualization for long conversations, cached state, and background synchronization.

No unnecessary polling, full-view rebuilds, or large synchronous operations on the UI thread.

[ADEMark](https://ademark.dev) is an external performance target. See
[our baseline work and remaining gaps](docs/ADEMARK.md). Plait has no ADEMark
score yet; internal probes are not native benchmark results.

## Development milestones

### M1 — Native desktop foundation

Build a functional GPUI application on macOS.

- Project → Machine → Thread navigation
- Polished dark interface
- Functional selection and navigation
- Agent conversation view with clearly labeled mock data
- Real, read-only Git status and diffs for a selected local repository
- Build and run successfully on macOS

### M2 — Local agent execution

Integrate Pi Durable on the local machine.

- Create and resume a thread
- Stream real agent events
- Persist conversation state
- Recover after client disconnection

### M3 — Remote execution

Run Plait's host daemon on Arthur.

- Connect from the Zaphod client
- Discover remote projects
- Start and resume remote threads
- Stream activity and Git changes
- Recover from network interruptions

### M4 — Native development tools

- Git staging, commits, and history
- File browser and editor
- Code review and agent feedback
- Structured process and log views

### M5 — Sidelit integration

- Live HTML, Markdown, and MDX previews
- Automatic refresh
- Select content and send it to the active agent

### M6 — Multi-machine experience

- Zaphod, Marvin, and Arthur
- Unified logical project navigation
- Independent host persistence
- Disconnected and reconnecting host states

## Explicit non-goals

Plait is not intended to include:

- An embedded terminal
- Mandatory worktrees
- Multi-agent swarms
- A general-purpose orchestration framework
- A centralized execution dependency
- An Electron or Tauri desktop shell
- A complete replacement for Rex
- A built-in cloud compute provider

## Development approach

Build small, functional vertical slices.

Never implement an entire roadmap in a single pass.

Every milestone must compile, run, and be independently testable.

Avoid speculative abstractions and unnecessary dependencies.

Maintain concise architecture documentation and explain major design decisions.

Use Codex and Claude interchangeably, with repository documentation as the shared source of truth.

**The current objective is safe local execution: create a thread, make an approved
change, run tests, and recover the same thread without duplicate execution.**

## Run the desktop

The desktop is implemented in `crates/plait`. It is a native macOS application,
not a web shell. Without a running local host, agent execution is disabled.

Requirements: macOS on Apple Silicon, Git, rustup, and full Xcode with the Metal
compiler. `rust-toolchain.toml` selects Rust 1.98.1. If Xcode reports a missing
Metal toolchain, run `xcodebuild -downloadComponent MetalToolchain`.

```sh
cargo build --locked --release
cargo run --locked --release -- /absolute/path/to/repository
```

Omit the path to restore the last checkout and thread, or inspect the current
directory on first launch. Drafts and unconfirmed submission IDs are saved locally;
reopening never sends a task automatically. Use **Open repository…** (Cmd-O)
to select another folder. The native picker accepts directories; Git validates
the selection in the background. Nothing is staged, committed, or discarded.

To build the native client, a combined client/host installer, independent
component installers, and a portable client ZIP:

```sh
bash scripts/build-macos.sh
open dist/Plait.app --args /absolute/path/to/repository
```

The host package includes pinned Node and runtime dependencies and installs a
per-user LaunchAgent. Client-only updates do not stop host work; host updates
are staged until you finish work and restart the host session. Preview installers
are unsigned and not notarized. There is no automatic updater. See
[installation, independent updates, and CI artifacts](docs/MACOS.md).

### Real versus example data

- **Real:** native input, local host connection, model choice and authentication
  status, saved defaults, host threads, and read-only Git status/diffs. Host
  execution is still subject to the M2 safety validation below.
- **Nothing is sample data.** The sidebar shows the opened checkout on this
  machine and its real host threads. Remote machines appear when remote
  execution exists (M3).
- **Not included:** remote execution, Git-write controls, a file editor, an
  embedded terminal, automatic updates, or mandatory worktrees.

### Keyboard controls

| Shortcut | Action |
|---|---|
| Cmd-O | Open a local repository |
| Cmd-R | Refresh local Git status and invalidate cached diffs |
| Cmd-1 … Cmd-9 | Jump to the Nth thread in the sidebar (Settings: threads, machines, or projects) |
| Cmd-Shift-1 / Cmd-Shift-2 | Conversation / Changes |
| Cmd-B | Show or hide the sidebar |
| Cmd-Shift-D | Show or hide render timing diagnostics |
| Cmd-comma | Open local host settings |
| Enter / Shift-Enter | Submit the composer draft / insert a line break |
| Cmd-Option-Up / Down | Previous / next thread |
| Option-Up / Down | Previous / next changed file |
| Tab / Shift-Tab | Move keyboard focus |
| Return / Space | Activate the focused control |
| Cmd-M / Cmd-W / Cmd-Q | Minimize / close / quit |

Scroll the conversation, navigation, file list, and diff with the trackpad or
mouse. Long diff lines scroll horizontally. On narrow windows, use Changes to
open Git; the optional conversation inspector is hidden.

### Development checks

Use the same development entry point on macOS and Linux:

```sh
bash scripts/check.sh
```

It checks pinned Rust dependencies, installs the runtime packages with
`npm ci --ignore-scripts`, checks TypeScript, runs scripted daemon tests, and
builds release binaries. macOS includes the native desktop and real command
sandbox tests. Linux checks the headless crates and explicitly verifies that
unsupported commands are denied. No provider credentials are needed.

This gives both systems a shared development workflow, not full product parity.
Linux command execution and native Linux UI validation remain outstanding.
See [the Linux sandbox decision](docs/LINUX-SANDBOX.md). To require a working
command sandbox, use `bash scripts/check.sh --require-command-sandbox`; it fails
on Linux until that backend is implemented and validated.

For focused Rust checks:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

On Linux, use `-p plait-core -p plait-host` instead of `--workspace` for clippy
and tests. These headless crates do not compile GPUI. This is a development test
target, not Linux agent execution or a Linux desktop. See
[the contributor checks](CONTRIBUTING.md#required-checks) for the full commands.

For a native UI smoke check, build the release binary, close Plait, then run
`bash scripts/smoke-macos.sh`. It needs macOS Accessibility permission for the
terminal. The check creates disposable repositories, launches its own app
processes, uses named accessibility controls (not global keystrokes), and checks
that the index and worktree remain unchanged. It stops its test processes and
does not change macOS permissions.

See [architecture and dependency research](docs/ARCHITECTURE.md) and
[verification, performance notes, and limits](docs/VERIFICATION.md).

### M2 local host — validation in progress

The independent Rust host and Pi Durable sidecar are in `crates/plait-host` and
`runtime/`. The desktop uses their owner-only local socket. Closing a window must
not stop a thread, and restarting a host must not replay interrupted commands.
See [the host contract](docs/HOST.md) for setup, permissions, and recovery rules.
Regular Pi extensions are not automatically compatible.

The opt-in acceptance check uses an existing Pi account and a disposable Rust
repository. It makes two turns in one thread, each with one exact source edit
and one `cargo test` command. It never gives the model this checkout. Run it
only after the scripted host safety suite passes:

```sh
node scripts/smoke-agent.mjs --self-test
PI_PROVIDER=YOUR_PROVIDER PI_MODEL=YOUR_MODEL node scripts/smoke-agent.mjs --live
```

The live check needs a built `target/debug/plait-host` and installed runtime
packages. `PLAIT_HOST_BINARY` can select another built host. Fixtures stay in a
private temporary directory for inspection. This check does not replace native
window-close/reopen, Stop, provider-failure, or process-crash tests.

## License

Plait's original code and documentation are available under the [MIT license](LICENSE).
Dependencies keep their own licenses, including GPUI's Apache-2.0 license.
