# Arthur development handoff

## Goal and boundary

Heath is moving development to Arthur through Rex on his phone. Prepare the
source workspace and Pi development agent first. Do not confuse this with a
working remote Plait host: the current Plait execution sandbox is macOS-only.
Do not remove safety checks to make Linux tests pass.

Read `AGENTS.md`, `README.md`, `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`,
`docs/HOST.md`, and `docs/VERIFICATION.md` before changes.

## Workspace

- Arthur: Ubuntu 26.04.1 LTS, x86-64, user `heath`.
- Intended checkout: `/home/heath/code/plait`.
- Development baseline: `main` after the M2 merge. Use a new focused branch
  for the next change; `feat/local-agent-loop` is the completed checkpoint.
- Mac source: `/Users/heath/code/plait`, on Zaphod.
- Node 26.13.0, npm 11.20.0, Pi 1.5.2, Git, Rustup, C compiler, CMake,
  pkg-config, Bubblewrap, and unshare were found on Arthur.
- Capacity observed: 4 CPUs, 7.2 GiB RAM, 131 GiB free disk.
- Use existing Arthur authentication. No Mac credentials or agent stores are
  part of the source transfer. Do not print credentials.
- Keep existing Rex services, sessions, and unrelated work. Run long jobs in
  named panes in the existing tab, with the agent pane focused. No new tabs.

Heath approved pushing the checkpoint and merging it into `main`, then continuing
development on Arthur. Clone `main` from `https://github.com/heath0xFF/plait`.
For an existing clean checkout, switch to `main` and pull with `--ff-only`.
Check `git status --short --branch` and
`git log -3 --oneline` before starting work. Do not reset either checkout.
Keep later synchronization explicit; do not overwrite work on the other machine.

## Verified on the Mac

- Independent Rust daemon and pinned Pi Durable 1.5.2 sidecar.
- Native composer, exact model selection/default, real threads and approvals.
- Read-only Git UI, protected dirty work, one writer per canonical checkout.
- Durable accepted input and exact postimage ownership; no crash replay.
- Formatting, strict clippy, 45 Rust tests, release workspace build, TypeScript
  checking, and all 39 runtime tests passed, without skips.
- Full sandboxed Plait workspace tests passed with a fresh private cache
  (33.285 seconds) and a reused cache (1.026 seconds).
- Native edit and command completion passed. Closing/reopening the test window
  during execution preserved the same thread and tool/request identities.
- Native Stop cancelled an observed active command; its process exited and
  pending approval cleared. The old human history and fixture were unchanged.
- Two independent safety review rounds cleared the implementation.

See `docs/VERIFICATION.md` for remaining native checks. A real agent-authored
Plait source change, live keyboard IME, and full VoiceOver remain unverified.
That live-model validation used the desktop-only bundle. The installer branch
now builds separate client and host/Node packages. Mac CI run `43782943308`
passed actual installation, 58 packaged runtime tests, active-update lifetime,
saved-state activation, and native launch. Its app image shows the local host
connected. Personal-Mac approval/authentication and an installed live-model turn
remain unverified. See `docs/MACOS.md` and the current verification record.

The older human test conversation and drafts remain untouched on the Mac under
`/tmp/plait-desktop-check.oaAqzp/`. Do not reset or copy that host store as a
Linux installation. The temporary logs and screenshots are not in this checkout.

## First work on Arthur

Start with a bounded portability review and agree on the next milestone with
Heath. Preserve the finished M2 behavior while making the headless test boundary
independent of desktop dependencies:

1. The headless split is now implemented in `crates/plait-core`. It owns the
   existing models, state, local client, and read-only Git tests without GPUI.
   `crates/plait` remains the native desktop and default Cargo member. Keep this
   crate boundary instead of adding platform conditionals throughout the UI.
2. Inspect `crates/plait-host/src/fingerprint.rs`, `files.rs`, and `command.rs`
   before choosing Linux equivalents. macOS descriptor paths and Seatbelt
   behavior must not be assumed to work on Linux.
3. `runtime/src/command/platform.ts` intentionally rejects non-macOS commands.
   Platform policy is now separate from common admission/persistence, and the
   fixed Mac Cargo test path is removed. `apple-toolchain.ts` remains Apple-only.
   Add real Linux containment and cancellation tests before enabling commands.
4. Keep desktop/Metal builds and UI inspection on a Mac or macOS CI. Arthur
   cannot verify the native macOS renderer.
5. A remote Plait connection needs authenticated transport and machine-scoped
   checkout state. Do not expose the existing local protocol over a public TCP
   listener. Do not add remote execution as an unreviewed shortcut.

A Pi CLI session on Arthur is a development tool, not the Plait sandbox. Its
ordinary filesystem/process permissions are broader. Review its proposed work
accordingly. Do not report Linux host execution as working just because Pi runs.

## Arthur headless checkpoint

The checkpoint is pushed as `ae8cab2` on `refactor/headless-core`, draft PR #9.
Linux CI passed. Mac CI exposed an Xcode 16 Metal layout that the existing
fixed-tool validator rejects; no sandbox assertions were removed.
Current installer work is on `feat/macos-client-host-installer`.

- Moved the six plain Rust modules and their tests into `crates/plait-core`.
  Production module contents were unchanged in that first extraction. The later
  socket checks below change the client transport. Desktop changes remain import
  renames only; the UI still owns all GPUI code.
- Kept the desktop as the default Cargo member and kept all dependency versions
  pinned. `Cargo.lock` changes only the local package graph.
- Added a Linux test for real non-UTF-8 Git filenames, including untracked
  previews, tracked diffs, and unchanged index/worktree contents after reads.
- Added `.github/workflows/headless.yml`: formatting, a no-GPUI dependency check,
  strict clippy, tests, and release builds for `plait-core` and `plait-host`.
  See CONTRIBUTING.md and VERIFICATION.md for commands and local results.
- No host protocol, runtime policy, persistence, or sandbox behavior changed.
  Native Mac checks are still required; a Linux build is not renderer validation.

The Linux containment investigation is recorded in [LINUX-SANDBOX.md](LINUX-SANDBOX.md).
Bubblewrap namespaces work on Arthur, but a disposable probe confirmed outside
pathname-socket access through a read-only mount. Arthur reports Landlock ABI 8;
pathname-socket restrictions start at ABI 9. The mechanism is still open. Keep
Linux commands disabled while building the common daemon safety and lifecycle
layer; do not treat a newer ABI as the only permitted future design.

### Portable daemon checkpoint

Heath's goal is the same Plait development workflow on macOS and Linux, with
independent daemons and clients that can disconnect and reconnect.

- `bash scripts/check.sh` is now the shared development entry point and is used
  by both CI workflows. It includes Rust, runtime installation/type checking,
  scripted daemon tests, and release builds. See CONTRIBUTING.md for coverage.
- `local_socket.rs` now verifies peer identity on both platforms. The daemon,
  desktop client, and host status/start checks use the same implementation.
- Platform command policy moved out of common file/checkout safety code.
  `runtime/src/command/macos.ts` preserves the original Seatbelt/environment
  body. `command/supervisor.ts` owns shared process lifecycle and bounded output.
- Spawn errors now clear timers/listeners. Final output drains after exit, with
  a one-second bound for pipes held by unsupported detached processes.
- The Linux development run passes 34 headless Rust tests and 39 runtime tests.
  The full Rust workspace also passes 49 tests and strict clippy on Arthur.
  Release core/host builds pass. No tests were skipped. These are scripted
  providers and lifecycle fixtures, not Linux sandbox or live-model acceptance.
- Original Mac command safety cases remain in explicit macOS test files,
  including command-crash recovery and command-backed postimage recovery.
  The fixed `/Users/heath/.../cargo` test path was removed.
- Both `bash scripts/check.sh --require-command-sandbox` and runtime
  `npm run test:sandbox` refuse Linux full-sandbox acceptance. Runtime and direct
  Rust guardian entry points also deny Linux commands before execution.

Next: re-run the full Mac checks and native client on a Mac, then select and test
a Linux containment design. Linux file-helper tests now cover root replacement,
postimages, protected files, aliases, and crash recovery, but do not establish
all filesystem-race or command-containment guarantees. No remote transport or
protocol/schema change was added. Do not add an unsandboxed fallback or expose
the local socket over TCP.

The installed `/usr/bin/rex` is an unrelated Perl task runner. The live Rex server
still uses its original, now-deleted executable. This session used a temporary
copy at `/tmp/plait-dev-tools.jRZw9R/rex` to run the `plait-checks` pane in the
existing tab. No services were restarted and no system binary was replaced.
That temporary path is not a permanent CLI repair; preserve the live sessions.

## Product direction

Plait remains native Rust + GPUI, Project → Machine → Thread, independent
hosts, first-class Git, no embedded terminal, no mandatory worktrees, and no
hidden orchestration. BB is the primary look-and-feel reference, not a source
of copied code or assets. See `DESIGN.md` and `docs/UI-REFERENCES.md`. The reference
checkout is on the Mac at `/Users/heath/code/bb`; clone it separately if needed.

## Current limits

Commands have a 15-minute deadline, no external network, read-only checkout and
source caches, and private per-thread build caches. Scratch test repositories
and local IPC are permitted. Cache size is not bounded; eviction is manual.
Detached daemons are unsupported. Ignored checkout contents can be read and
sent to the model provider. Do not weaken these boundaries during the port.
