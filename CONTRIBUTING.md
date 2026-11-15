# Contributing to Plait

Plait is early-stage software. M1 runs on Apple Silicon macOS. The current work
is M2: local, persistent agent execution. Read [README.md](README.md),
[DESIGN.md](DESIGN.md), and [the architecture notes](docs/ARCHITECTURE.md) first.

## Start here

```sh
git clone https://github.com/heath0xFF/plait.git
cd plait
cargo build --locked --release
cargo run --locked --release -- /absolute/path/to/a/repository
```

Install full Xcode and its Metal toolchain. Rustup uses the pinned
`rust-toolchain.toml`. See the README for setup details. Linux and Intel macOS
are not verified desktop targets. Rex is useful for long jobs but is not required
to build. The desktop does not need Node; the isolated runtime probe does.

## Work together

- Pick an issue and comment before starting, so two people do not do the same work.
- Use a branch and a small pull request. Forks work without collaborator access.
- Keep `main` buildable. Worktrees are optional.
- State what changed, how you checked it, and what you did not check.
- Include screenshots for UI changes. Do not commit credentials, private
  transcripts, local configuration, dependency directories, or build output.
- Do not copy code from reference apps. Dependencies keep their own licenses.

## M2 work split

These are parallel tasks, not features already present in the desktop.

### Native composer — open for a contributor

Own `crates/plait/src/ui/composer.rs` and its focused tests. Build a standalone
GPUI input component; do not connect it to a model or start a process.

Component boundary:

- The component owns the editable draft, selection, focus, and IME composition.
- It emits `ComposerEvent::Submit(String)` and `ComposerEvent::Stop`.
- The parent sets enabled/busy state. Offline means disabled, with a reason.
- Enter submits; Shift-Enter inserts a line break. Enter during IME composition
  must not submit. Empty or whitespace-only drafts must not submit.
- A submit event does not clear the draft. The parent clears it only after the
  host accepts it, and only if the draft has not changed since submission.
- Support clipboard, undo/redo, keyboard focus, accessible labels, and the
  900-point minimum window. Use GPUI's native text-input APIs, not a fake field.

Expose only the component methods the parent needs. Coordinate any changes to
`ui/mod.rs`, `conversation.rs`, or shared models before making them. The preview
composer must not claim to send messages until the host is connected.

### Local host and runtime — maintainer/agent track

Own the Rust host, TypeScript Pi Durable sidecar, and their local protocol.
Keep runtime and persistence independent of GPUI. Start with one local checkout.

- One host owns the sidecar and the store's writer lock.
- The local connection must be owner-only; expose no public network listener.
- Create/resume/list threads with stable IDs and deduplicated submissions.
- Stream typed state without blocking the UI. Bound messages and queued output.
- A client disconnect stops its subscription, not the agent run.
- Keep credentials on the host. Load project instructions explicitly.
- Permit only one writing run per checkout. Existing dirty files must survive.
- Test reconnect, abort, interrupted tools, malformed input, and host restart.

The [probe](experiments/pi-durable/README.md) validates parts of the runtime. It
is not the host implementation. Regular Pi extensions are not automatically
compatible with Pi Durable.

### Join the two tracks

Agree on the host/client contract before connecting the component. The first
end-to-end acceptance test is:

1. Create a local thread and submit a task in a disposable repository.
2. See real assistant and tool activity, then inspect the Git changes.
3. Close the desktop while work is active; the host continues.
4. Reopen the desktop and reconnect to the same thread without duplicate work.

Remote hosts, orchestration, Git-write controls, and an embedded terminal are
outside this slice.

## Required checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --release
```

For UI changes, inspect wide and narrow native windows. With Accessibility
permission already granted, close Plait and run `bash scripts/smoke-macos.sh`.
Do not inject global keystrokes into another person's desktop.

For the runtime probe:

```sh
cd experiments/pi-durable
npm ci --ignore-scripts
npm test
```

This test uses a scripted provider and temporary files. Live model checks are
explicit opt-in and use your own account; see the probe README. No shared API
keys are needed to contribute.

## License

Contributions to Plait are made under the [MIT license](LICENSE).
