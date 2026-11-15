# Verification

## Client/host installer work — Mac results pending

The installer branch adds combined and independent component packages. Linux
checks passed with 50 workspace Rust tests, strict Clippy, and the portable
runtime suite (40 tests). Ten portable package-planning tests pass. Mac build, actual
installation, native launch, and installed-runtime acceptance remain pending.

The headless checkpoint is pushed as `ae8cab2` (PR #9). Its Linux CI passed.
Its Mac CI failed at Metal lookup. Xcode 16.4 uses a versioned application
path, and the macOS 15 image's compiler files and toolchain root are owned by
UID 501, not root. The installer branch handles canonical aliases and known
Metal layouts without removing the compiler/root ownership or write-mode checks.
CI now uses macOS 26 and validates tool discovery before the long build. No system
ownership or permissions were changed. This still needs real Mac confirmation;
it is not a sandbox pass yet.

The new CI install test uses a labeled upgrade fixture with unchanged program
code. It checks install/activation lifetime and durable state, not compatibility
with an unknown future runtime or storage migration. It does not use real model
credentials. Personal-Mac Gatekeeper approval, authentication, and live-model
acceptance of the installed package are still required before travel use.

## Portable daemon development checkpoint

Verified on Arthur after the local-socket and runtime boundary changes:

- `bash scripts/check.sh` passed: formatting, strict headless clippy, 34 Rust
  tests (30 core and 4 host), a debug host build, pinned runtime installation,
  TypeScript checking, 39 runtime tests, and core/host release builds.
- Full workspace strict clippy and all 49 Rust tests also passed on Linux.
- No selected tests were skipped, ignored, or filtered out.
- Both full command-sandbox gates correctly returned an error on Linux before
  running tests. Linux runtime tests explicitly verify denial at both the
  TypeScript and Rust helper entry points.
- Workflow `actionlint`, shell syntax, JavaScript syntax, and `git diff --check`
  passed. The Mac policy/environment body matches the pre-refactor source.

The 39 Linux runtime tests are **not** the older 39-test Mac sandbox suite below.
They test independent daemon start, socket protocol limits, persistent models and
request IDs, client disconnect/reconnect, approved file edits, Stop approval
revocation, checkout leases, exact postimages, dirty-file protection, and crash
recovery. Common guardian tests use a clearly labeled subprocess fixture to
check cancellation, output drainage/bounds, spawn failure, and deadline cleanup.
That fixture is not a containment mechanism.

The test runner selects real command-sandbox and command-crash cases on macOS,
and command-denial cases on Linux. All original Mac command tests remain. Mac
checks, native UI inspection, and live-model acceptance have not been repeated
for this refactor. Hosted CI results are not yet observed. Linux command
execution and remote Plait connections remain unsupported.

## Arthur headless core checkpoint (initial extraction)

Verified on Arthur (Ubuntu x86-64, pinned Rust 1.98.1) after the core crate split:

- `cargo fmt --all -- --check`
- `cargo clippy --locked -p plait-core -p plait-host --all-targets -- -D warnings`
- `cargo test --locked -p plait-core -p plait-host` — 31 tests passed:
  28 core and 3 host tests. No tests were ignored or filtered out.
- `cargo build --locked --release -p plait-core -p plait-host`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo test --locked --workspace` — 46 tests passed, including the 15
  desktop/composer tests. No tests were ignored or filtered out.
- The headless dependency graph contains no GPUI packages. The default workspace
  member is still the desktop. Existing dependency versions are unchanged.
- `actionlint` passed for both workflows; `git diff --check` passed.

The new Linux-only Git test creates a non-UTF-8 filename and verifies real status,
preview, and tracked diff reads without changing the index or worktree. The
existing 27 core tests moved with their production modules. Those six production
modules are byte-for-byte unchanged; desktop source changes only rename imports
from `plait` to `plait_core`.

The full workspace test target also compiles on Arthur, but it does not validate
a native window or the macOS renderer. The smaller headless CI job does not
compile GPUI. Cargo can still fetch desktop dependency sources while resolving
the workspace. The new hosted CI job has not been observed running yet.

No Mac build, native UI inspection, TypeScript runtime suite, or live model run
was repeated for this split. The native validation below records the earlier
Mac checkpoint. Linux agent execution and remote connections remain unsupported;
no sandbox checks were removed. See [HANDOFF.md](HANDOFF.md) for the remaining
platform boundaries and the temporary Rex CLI workaround.

## Arthur Linux containment investigation

On 2026-11-15, an unprivileged Bubblewrap 0.11.1 namespace probe succeeded on
Arthur. The Landlock ABI query returned 8. A second disposable probe confirmed
that a process in `--unshare-all` namespaces could connect to an outside Unix
server through a `--ro-bind` fixture directory. It received only a fixed test
message. The server recorded one connection, and the fixture was removed.

Runtime `npm ci --ignore-scripts` and `npm run check` also passed. The runtime
safety suite was not run in this investigation, and no Linux command backend was
enabled. No system security settings were changed. See
[LINUX-SANDBOX.md](LINUX-SANDBOX.md) for the open design decision and test gates.

## M2 validation — in progress

The full workspace passes formatting, warning-free clippy, 45 Rust tests
(27 library, 15 desktop/composer, and 3 host), and the release build. Runtime
installation, TypeScript checking, and all 39 runtime tests pass after the final
build-environment changes. No tests were skipped. A second independent safety
review cleared the cache, toolchain, and sandbox changes.

PID-scoped native checks used an isolated store and a disposable checkout:

- Loaded the real Pi catalog and selected an exact provider/model.
- Saved a default only after host acknowledgement.
- Created a real thread without starting a task.
- Entered text through native accessibility input; blank Send was disabled.
- Changed the default and confirmed the existing thread kept its saved model.
- Disconnected the host and confirmed the disabled composer retained its draft.
- Inspected wide and 900 × 600 content-size windows. AeroSpace was temporarily
  set to float only the test window, then its layout was restored.

The checks used direct accessibility actions by element identity. GPUI's
coordinate-based fallback could activate a neighboring model row after a resize;
new host controls and Send/Stop now use explicit identity-bound actions.
No global keystrokes were sent, and the existing user app was left untouched.

- [Native settings and acknowledged default](screenshots/m2-settings.png)
- [Retained draft while offline at 900 points](screenshots/m2-composer-offline-900.png)

The real-provider acceptance script passed with `openai-codex/gpt-6-astra` in
a disposable Rust repository: two exact approved edits, two sandboxed Cargo test
runs, same-thread follow-up, observer reconnect, create/submit deduplication,
and an unchanged Git index. Independent review cleared the core safety fixes.

The real Plait checkout now builds and passes all 45 workspace tests inside the
sandbox: 33.285 seconds with an empty private thread cache, then 1.026 seconds
with the same cache. Fixed installed Metal/Git lookup, stable private Cargo
paths, and scratch-only test fixtures resolved the earlier build failures.
Negative tests cover outside process signals, sockets, TCP, and checkout writes.

Native Send, accepted-draft clearing, exact edit approval, and command completion
passed. The test window was closed during an observed sleep process and reopened
on the same running thread. Host execution continued, request/tool identities
were unchanged, and the real final response arrived. Native Stop then cancelled
an approved active command: its sleep process exited and pending approval cleared.
The old human conversation and fixture remained byte-for-byte unchanged.

**An agent-authored change to Plait itself is not yet verified.** Real keyboard
IME use and full VoiceOver coverage are not verified. That validation used the
desktop-only development bundle; current installer work is recorded above.
Linux Plait execution is not supported yet; see [the Arthur handoff](HANDOFF.md).

The following sections record the historical M1 baseline, not M2 acceptance.

## M1 environment

Built and run on Zaphod: Apple Silicon, macOS 26.6.2, Rust 1.98.1.
GPUI revision: `596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14`.

The first build found a missing Xcode Metal compiler. Installed it with
`xcodebuild -downloadComponent MetalToolchain`, then rebuilt successfully.
The pinned Rust toolchain was also installed. The system default Rust toolchain
was not changed.

## Automated checks

Passed:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo test --locked --workspace` — **16 tests**
- `cargo build --locked`
- `cargo build --locked --release`
- `bash -n scripts/smoke-macos.sh`
- Native AppleScript compilation and `bash scripts/smoke-macos.sh`

Rust tests cover:

- NUL-delimited status, spaces/newlines, rename order, invalid relative paths,
  and byte-preserving non-UTF-8 filename parsing.
- Unborn and clean repositories, missing paths, detached HEAD, merge conflicts,
  renames, deletion, binary files, and literal Git pathspecs.
- Separate staged and unstaged diffs, correct unified line numbers, empty
  untracked files, and no staged preview for untracked content.
- Unchanged index bytes and working-file content after reads; no index lock.
- Disabled external diff, textconv, and clean/process filters. A regression test
  first reproduced Git executing a configured filter, then passed with filters
  disabled.
- Symlink/special-file refusal, parent-symlink containment, bounded output and
  long lines, subprocess timeout and child reaping.
- Repository epoch checks, stale file results, queued latest selection, cached
  switching, valid thread navigation, and revealing collapsed parents.

macOS rejects invalid UTF-8 filenames on this filesystem. That case is checked
at the parser boundary, not by creating such a file on disk.

## Native checks

Launched debug and release binaries in a named Rex pane. Inspected rendered
windows, application output, and the macOS accessibility tree.

The final native smoke check uses **named accessibility controls only**. It
creates its own temporary repositories and app processes, then verifies:

- Project and machine collapse/expand.
- Thread selection, remote disclosure, and empty conversations.
- Native folder picker opening and cancellation.
- Staged versus unstaged content, including rapid alternating selections.
- Clean repository and invalid-directory states, with a recovery control.
- Unchanged test index and working file after all UI operations.

Earlier keyboard-driven automation could lose focus and send text to another
application. It was removed. The final test has no global keystroke injection.
A focus-loss bug found when hiding the sidebar was fixed by retaining root
focus before removing a focused child.

Manually checked the native picker selecting a directory, Conversation/Changes
shortcuts, thread navigation, and close/quit. Visually inspected wide windows,
1000 × 680, and the 900 × 600 content-size floor. The conversation inspector
hides at narrow widths; Git remains available in Changes. Long paths truncate
without pushing the diff out of the window.

A 20,000-line untracked fixture showed the preview limit and rendered only the
visible rows (22 diff-row accessibility groups in the inspected viewport), not
10,000 UI elements at once.

### Captures

- [Conversation and local repository](screenshots/m1-conversation.png)
- [Staged diff in a disposable real Git repository](screenshots/m1-diff.png)

The Git data in the diff capture comes from actual CLI reads of a test repository.
The conversation and thread statuses in both captures are labeled fixtures.

## Performance observations

One release session reported:

| Instrument | Observation |
|---|---:|
| Main-entry to first root render | 89 ms |
| Root-view construction in the captured diff | 44 µs |
| Peak root-view construction after narrow resize | 121 µs |
| Selection handler to next root render | 8.1 ms |
| Local status, including safe-config reads | 65 ms |
| Uncached tracked diff | 37 ms |

These are sample observations, not a benchmark distribution. The footer's UI
numbers exclude deferred list-row rendering, layout, GPU work, and presentation.
Startup does not include OS process creation. Git timing includes background
work; it is not UI blocking time. Cached diffs require no Git subprocess.

**The 60 FPS and complete warm-launch targets are not yet validated.** Measure
frame presentation and repeated launches before treating them as release gates.
There is no idle polling or continuous animation.

## Conversation refinement

The BB-inspired M1 pass keeps user messages in right-aligned bubbles and agent
replies as open text. Tool rows expand in place. The sidebar now holds the
wordmark; the extra app-wide header is removed.

Rechecked formatting, warning-free Clippy, all 16 Rust tests, and the release
build. The native smoke suite also checks that tool details start collapsed,
open on click, close again, and reset after a thread switch. Repository read-only
checks still pass.

Visually inspected 1380 × 860 and 900 × 600 windows, including expanded tool
text at the narrow size. The Git inspector hides at the existing breakpoint;
conversation content scrolls above the unavailable composer. Updated both M1
screenshots. Full VoiceOver and sustained frame-rate checks remain outstanding.

## macOS package and vector branding

The development package builds with `bash scripts/build-macos.sh`. Checks pass
for the property list, arm64 executable, system-only dynamic links, dependency
notices, compiled icon, ad-hoc signature, and the extracted ZIP. The archive's
SHA-256 checksum also passes.

Imported the owner's vector kit unchanged, with its reconstruction notes. GPUI
embeds the ribbon mark, outlined lettering, and category glyphs. The build uses
the kit's supplied 1024-pixel app-icon export to generate all native icon sizes.
No screenshot crops or replacement hand-drawn glyphs remain.

Formatting, warning-free Clippy, 16 library tests plus one GPUI SVG rendering
test, the release build, and the native accessibility smoke suite pass. The
scripted Pi Durable probe and both SIGKILL recovery tests pass without model
credentials. `actionlint` passes for the new macOS workflow; local checks do not
establish that the hosted build has passed.

Launched `dist/Plait.app` through Launch Services. Visually checked its full-color
brand and navigation at 1380 × 860 and the 900 × 600 content-size floor. The
narrow view keeps the brand and state labels and hides the Git inspector.
AeroSpace was temporarily set to float only the test window for these checks;
its tiled layout was restored afterward.

- [Packaged app with vector branding](screenshots/macos-branding.png)

This is still the M1 desktop. The package does not contain an agent host or
updater. See [macOS packaging](MACOS.md) for distribution limits.

## Known limitations

- One selected local checkout. Path, selection, and collapse state are not saved.
- All agent events and thread statuses are static fixtures. Marvin and Arthur
  do not connect. There is no daemon, Pi integration, or agent execution.
- Git refresh is manual. Refresh clears the diff cache and selects the first
  changed file. External edits can make a cached snapshot stale.
- No staging, discard, commit, history, editor, or terminal. Diff text is not an
  editable or selectable code editor.
- Each subprocess has a 10-second deadline. A job can contain several commands;
  new selections wait behind the current job. Slow filesystem mounts can also
  delay background file reads. The UI remains independent of that work.
- Bare repositories are not supported. Dirty submodule contents are not scanned;
  gitlink changes can be shown. Combined conflict patches remain readable text
  but do not get full multi-parent line-number gutters.
- Binary/non-UTF-8 content has no text preview. Large patches and long lines are
  clipped with a notice. Untracked symlinks and special files are not followed.
- Clean/process filters are disabled. Repositories that depend on filters, such
  as Git LFS, can show raw-content differences rather than filter-normalized data.
- The development app bundle is ad-hoc signed only. No Developer ID signing,
  notarization, installer, or automatic updater. Downloaded builds may be blocked
  by Gatekeeper. Cargo and `target/release/plait` remain supported.
- Intel macOS, other operating systems, full VoiceOver behavior, and sustained
  frame-rate/load testing have not been verified.

## Next milestone

M2 validation has started in [the isolated probe](../experiments/pi-durable/README.md).
Published Pi 1.5.2 passed a live `openai-codex/gpt-6-astra` run using existing Pi
authentication, real temporary-file read/edit tools, incremental state, observer
detach/reattach, SQLite reopen, stable identity, and request deduplication.

Separate scripted-provider SIGKILL tests now verify safe versus unsafe tool
replay after reopening the Durable store. This is not desktop integration or
Plait-host recovery verification. Next implement the independent host, native
composer, and one local create/resume/stream/persist path with host-crash and
client-reconnect tests. Keep the Git read-only
boundary until the separate Git-write milestone. Do not add remote execution or
orchestration as part of that first integration.
