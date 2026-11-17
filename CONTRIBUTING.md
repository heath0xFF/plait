# Contributing to Plait

Plait is an early local macOS preview. Current work is **M2.5: the daily driver**
on local execution; see the [roadmap](docs/ROADMAP.md). Read the [README](README.md), [architecture](docs/ARCHITECTURE.md),
and [design guide](DESIGN.md) before making changes.

## Build from source

For the native desktop, use an Apple Silicon Mac with Git, rustup, and full Xcode
with Metal. Runtime work also needs Node 22.19+ and npm. Rustup selects the pinned
`rust-toolchain.toml`; keep dependency versions and lockfiles unchanged unless a
change explicitly requires an update.

If Xcode reports a missing Metal compiler, run
`xcodebuild -downloadComponent MetalToolchain`.

```sh
git clone https://github.com/heath0xFF/plait.git
cd plait
cargo build --locked --release
cargo run --locked --release -- /absolute/path/to/repository
```

Omit the repository argument to restore the last checkout and thread. Agent work
requires a running [installed host](docs/MACOS.md) or [development host](docs/HOST.md).
See [Mac packaging](docs/MACOS.md#build-and-verification) to build the app bundle
and installers, or run `scripts/update-installed-macos.sh` to
[install your checkout](docs/MACOS.md#from-a-source-checkout) over the installed app.
To publish a release that installed apps can update to, see
[Releasing](docs/RELEASING.md). Rex is optional development tooling, not a build dependency.

## Propose a change

- Check existing issues and discuss scope before starting substantial work.
- Use a focused branch and small pull request. Worktrees are optional.
- Describe the change, checks, and remaining limits. Include screenshots for UI work.
- Keep credentials, private transcripts, local settings, and build output out of Git.
- Write public-facing docs about behavior, setup, and limits. Use issues and pull
  requests for change history; keep personal machine and session notes out of docs.
- Build original application code and design; follow the [originality policy](docs/ORIGINALITY.md).
- Discuss changes to shared models, protocol, durable storage, or permission rules
  before implementation. Preserve compatibility and test the failure paths.

## Work split

| Area | Responsibility |
| --- | --- |
| `crates/plait` | GPUI presentation, input, focus, and background-work dispatch |
| `crates/plait-core` | Plain models, state transitions, local client, recovery, hardened Git reads and explicit writes; no GPUI |
| `crates/plait-host` | Independent host lifetime, peer identity, filesystem/process helpers |
| `runtime/` | Pi integration, approvals, durable execution, and platform command policy |

The composer owns editable text, selection, undo, and IME composition. It emits
Submit (including revision and steering intent)/Stop events; the parent owns host requests and acceptance. Enter during
IME composition must not submit. An acknowledgement clears only the draft
revision that was sent. Every pending message keeps that revision, exact request
ID, approval mode, and queue mode through journal recovery. Offline input has a
disabled state and a reason.

The host owns accepted work, not a window or event subscription. Closing a client
must not stop a run; restarting a host must not replay an interrupted effect.
Keep one active writer per checkout, preserve pre-existing dirty work, and retain
exact-call approval and sandbox checks. Models and Git must remain independent
of GPUI; filesystem and Git work must stay off the render thread.

Remote execution, Git writes beyond staging and committing (push, discard,
history rewriting), unattended orchestration, and an embedded terminal are
outside the current work. See the [host contract](docs/HOST.md) for
the complete safety boundary and the [roadmap](docs/ROADMAP.md) for later work.

## Required checks

The shared entry point is:

```sh
bash scripts/check.sh
```

It installs runtime packages from the lockfile without npm lifecycle scripts,
checks Rust and TypeScript, runs the selected platform tests, and builds release
binaries. Both CI workflows use it. A C compiler/linker is required on Linux.

| Check | macOS | Linux |
| --- | --- | --- |
| Rust checks and release build | Complete workspace | Core and host |
| Common daemon and file-safety tests | Yes | Yes |
| Scripted command lifecycle | Yes | Yes; not containment evidence |
| Real command sandbox | Seatbelt tests | Execution-denial tests only |
| Native window inspection | Separate check | Not validated |

Use `bash scripts/check.sh --require-command-sandbox` for the full sandbox gate.
It fails on Linux until a containment backend is implemented and validated.
Do not remove command rejection or skip a safety assertion to make a port pass.
See the [Linux sandbox decision](docs/LINUX-SANDBOX.md).

Focused macOS workspace checks:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --release
```

Focused Linux headless checks:

```sh
cargo fmt --all -- --check
cargo clippy --locked -p plait-core -p plait-host --all-targets -- -D warnings
cargo test --locked -p plait-core -p plait-host
cargo build --locked --release -p plait-core -p plait-host
```

The headless crates do not compile GPUI, though Cargo can fetch its pinned sources
while resolving the workspace. A Linux pass is not native UI or sandbox acceptance.
The Mac workspace and runtime safety suites remain required before merge.

For focused runtime work, build `plait-host`, then run `npm run check` and
`npm test` in `runtime/`. The runner labels its platform coverage.
`npm run test:sandbox` requires a real command sandbox and fails on Linux.
No provider credentials are needed for the scripted suites.

## UI checks

Build and inspect wide and narrow native windows after UI changes. With macOS
Accessibility permission already granted, close Plait and run:

```sh
bash scripts/smoke-macos.sh
```

The script uses disposable repositories and its own app processes. It finds
controls by accessible name/role, not global keystrokes. Keep those identities
stable or update the check. Do not alter OS permissions automatically.

To see the UI without opening a window, on macOS or Linux:

```sh
bash scripts/snapshots.sh /tmp/plait-snapshots
```

This draws every scene at 1380 × 860 (`-wide`) and 900 × 600 (`-narrow`) and
prints the PNG files it wrote. It needs no display, Screen Recording, or
Accessibility permission, and it does not contact a running host. Read the
images after a UI change.

- **macOS:** Metal and Core Text, as in the app, so glyphs, wrapping, and
  truncation come from the code the app runs.
- **Linux:** wgpu with Linux fonts. It needs a software Vulkan driver such as
  `mesa-vulkan-drivers`, and checks layout, density, and color only.

The images are not the native window. They leave out the title bar, real
pointer and key input, focus that follows it, the caret and IME, the
accessibility tree, animation, and timing. `scripts/smoke-macos.sh` and a
manual look at the app still cover those. Scenes use injected sample state, not
a live provider. See [how the harness works](docs/ARCHITECTURE.md#scene-snapshots-without-a-window)
and the [verification boundaries](docs/VERIFICATION.md#coverage-boundaries).

To add a scene for a new view, edit `crates/plait/src/ui/snapshot.rs`: write a
small function that builds or changes the sample state, set it on the harness,
and call `harness.capture("name-{suffix}")` from `walk` or a `*_walk` helper.
Sample state stays in that test module; no shipped view may contain it.

The README images come from `crates/plait/src/ui/snapshot/showcase.rs`, which
draws the same views over a disposable sample repository with hand-written
threads. Run `bash scripts/readme-screenshots.sh` after a visible change to
those views and commit the updated files in `docs/images`. Keep their captions
saying the threads are sample data.

## Runtime probes and live acceptance

The isolated, credential-free Pi integration probe is separate from host testing:

```sh
cd experiments/pi-durable
npm ci --ignore-scripts
npm test
```

See [the probe guide](experiments/pi-durable/README.md) for its scope.
After the host safety suite passes, an opt-in live check can use your own Pi
account in a disposable Rust repository. Run it from the repository root:

```sh
node scripts/smoke-agent.mjs --self-test
PI_PROVIDER=YOUR_PROVIDER PI_MODEL=YOUR_MODEL node scripts/smoke-agent.mjs --live
```

Build `target/debug/plait-host` and install runtime packages first.
`PLAIT_HOST_BINARY` can select another built host. The check makes model requests
on the selected account. It does not give the model this source checkout;
fixtures remain in a private temporary directory for inspection.

Neither probe replaces installed native, Stop, provider-failure, or crash checks.
See [manual release acceptance](docs/VERIFICATION.md#manual-release-acceptance).

## License

Contributions to Plait are made under the [MIT license](LICENSE).
Dependencies retain their own licenses and notices.
