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
The macOS bundle contains the desktop, not a complete host/Node installation.

The older human test conversation and drafts remain untouched on the Mac under
`/tmp/plait-desktop-check.oaAqzp/`. Do not reset or copy that host store as a
Linux installation. The temporary logs and screenshots are not in this checkout.

## First work on Arthur

Start with a bounded portability review and agree on the next milestone with
Heath. Preserve the finished M2 behavior while making the headless test boundary
independent of desktop dependencies:

1. The plain Rust modules do not import GPUI, but `crates/plait/Cargo.toml`
   still depends on GPUI. Separate the headless core/client test target cleanly
   instead of adding unrelated platform conditionals throughout the UI.
2. Inspect `crates/plait-host/src/fingerprint.rs`, `files.rs`, and `command.rs`
   before choosing Linux equivalents. macOS descriptor paths and Seatbelt
   behavior must not be assumed to work on Linux.
3. `runtime/src/safety.ts` intentionally rejects non-macOS commands.
   `apple-toolchain.ts` is Apple-specific. Some runtime safety tests also pin a
   Mac Cargo path. Separate platform policy from common admission/persistence
   logic; add real Linux containment and cancellation tests before enabling it.
4. Keep desktop/Metal builds and UI inspection on a Mac or macOS CI. Arthur
   cannot verify the native macOS renderer.
5. A remote Plait connection needs authenticated transport and machine-scoped
   checkout state. Do not expose the existing local protocol over a public TCP
   listener. Do not add remote execution as an unreviewed shortcut.

A Pi CLI session on Arthur is a development tool, not the Plait sandbox. Its
ordinary filesystem/process permissions are broader. Review its proposed work
accordingly. Do not report Linux host execution as working just because Pi runs.

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
