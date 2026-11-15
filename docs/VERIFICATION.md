# M1 verification

## Environment

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
- No application bundle, signing, notarization, installer, or custom Dock icon.
  Launch with Cargo or `target/release/plait`.
- Intel macOS, other operating systems, full VoiceOver behavior, and sustained
  frame-rate/load testing have not been verified.

## Next milestone

M2 validation has started in [the isolated probe](../experiments/pi-durable/README.md).
Published Pi 1.5.2 passed a live `openai-codex/gpt-6-astra` run using existing Pi
authentication, real temporary-file read/edit tools, incremental state, observer
detach/reattach, SQLite reopen, stable identity, and request deduplication.

This is not desktop integration or crash-recovery verification. Next implement
the independent host, native composer, and one local create/resume/stream/persist
path with process-crash and client-reconnect tests. Keep the Git read-only
boundary until the separate Git-write milestone. Do not add remote execution or
orchestration as part of that first integration.
