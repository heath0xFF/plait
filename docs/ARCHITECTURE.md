# M1 architecture

## Scope

One Cargo workspace, one package: `crates/plait`. The package has a library for
models, fixtures, state, and Git, plus a GPUI desktop binary. There is no daemon,
RPC layer, persistence service, agent runtime, terminal, or worktree manager.

```text
main.rs                 native app lifecycle, window, startup
ui/                     GPUI layout, actions, focus, lists, theme
  mod.rs                UI state and background-task dispatch
  navigation.rs         Project → Machine → Thread
  conversation.rs       labeled fixture events
  changes.rs            real local status and diff views
state.rs                selection, collapse state, request queue, cache
model.rs                plain Rust data, no GPUI
fixtures.rs             example projects, machines, threads, events
git.rs                  synchronous, read-only Git adapter; background only
```

The library does not import GPUI. Its tests do not need a window. M1 does not
need crate boundaries or a service trait for each module.

## Data flow

1. A UI action changes application state.
2. `RepositoryState::next_job` returns at most one pending job.
3. The UI dispatcher runs `Job::run` on GPUI's background executor.
4. The result returns through a weak entity handle.
5. State checks the repository epoch and selected diff key before displaying it.
6. `cx.notify()` schedules an update. There is no redraw loop or repository poll.

Only one Git job runs at a time. Rapid selections replace the pending job, not
the running job. A repository change or refresh increments the epoch. An old
result cannot overwrite the new repository. A stale file result can enter the
current snapshot's cache, but cannot replace another selected file.

Diffs are loaded on demand. Up to eight diffs are cached per status snapshot.
Refresh clears that cache. All threads on Zaphod share the same local checkout.
Remote fixture selection never starts Git, SSH, or a network connection.

## Git boundary

Public operations are `status(path)` and `diff(root, change, mode)`. Both return
data or a user-facing error. There are no write operations.

- Git CLI, not libgit2. Use porcelain v1 with NUL-delimited paths.
- Keep paths as `PathBuf`/OS bytes. Lossy text is for display only.
- Use literal pathspecs and an argument array, never shell interpolation.
- Separate index and worktree diffs. No combined “all changes” view that hides
  partially staged files.
- Disable optional index locks, fsmonitor, untracked cache, external diff, and
  textconv. Read configuration **key names only** and disable all configured
  clean/process filter drivers for each command. Git can otherwise execute a
  filter even during a read-only diff. No pager, prompt, hook invocation, or
  network command. Ignore dirty submodule contents; show gitlink changes only.
- Ignore inherited repository-location environment variables.
- Reap subprocesses. Bound each subprocess to 10 seconds, status/config keys to
  8 MiB, diff output to 512 KiB, and stderr to 8 KiB. Pipe readers drain
  concurrently. A job includes multiple subprocesses, so its total deadline can
  be longer than 10 seconds.
- Reject oversized status rather than showing an incomplete file list.
- Limit displayed patches to 10,000 lines and 2,000 characters per line.
  A clipped preview is labeled. Unified source line numbers are parsed in the
  background, as is the widest-line index used for horizontal scrolling.
- Untracked regular files get a bounded text preview. Binary/non-UTF-8 content,
  symlinks, directories, and special files are not followed as text.
- Untracked file opens walk directory descriptors with `openat`, `O_NOFOLLOW`,
  and `O_NONBLOCK`. This prevents a parent symlink swap from escaping the
  checkout or a special file from blocking a preview. The small `libc` dependency
  supplies platform constants and calls; it is already used by GPUI.

A status snapshot and its later diff are not a filesystem transaction. Another
program can change the checkout between reads. Refresh is explicit. No data is
staged, discarded, committed, or repaired in response to a read error.

## UI and performance

Use GPUI's native macOS platform, Metal renderer, system sans-serif font, and
Menlo for code. SVG assets are embedded strings, not runtime file reads. GPUI
renders the supplied ribbon mark and outlined wordmark as full-color images;
category glyphs use its tinted SVG path. Brand files live in `assets/branding`.
The development bundle and icon build are described in [MACOS.md](MACOS.md).

Conversation events use GPUI's variable-height `list`. User messages render as
right-aligned bubbles; agent replies remain open text. Tool details start
collapsed. Disclosure state belongs to the UI and clears on thread selection.
Expanding a tool invalidates only that list item's measured height with `splice`,
not the whole scroll state. Changed files and diff
lines use `uniform_list`; only visible rows are built. Cached data uses `Arc`
rather than cloning large strings on every render. Long diff lines scroll
horizontally.

The sidebar is 248 points on wide windows, 218 on narrow windows. Below 1150
points, the conversation's 280-point Git inspector is removed; Git remains available in
the Changes tab. The minimum window is 900 × 600. Cmd-B hides the sidebar.

Controls have focus rings, tab stops, accessible names, and semantic roles.
Thread state has both a color and a text label. No decorative animation is used.

The footer records root-view construction time, peak construction time,
selection-handler-to-render time, and startup-to-first-render time. Git panels
show background read durations. These are diagnostics, **not** measurements of
GPU frame time, input-to-display latency, or complete launch time. Instrumentation
does not write a log from the UI thread.

## Dependency decision

GPUI and `gpui_platform` are pinned together at:

`596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14`

This was upstream HEAD when researched. It was compiled and run on Zaphod.
The toolchain is pinned to upstream's Rust 1.98.1. Cargo.lock pins the full
resolved graph. Only macOS features are enabled; `font-kit` is required for text.

The live gpui.rs landing-page snippet still uses `Application::new()`. The pinned
source's working examples use `gpui_platform::application()`. Source and builds,
not the landing-page snippet, define this implementation.

Reviewed upstream:
- [GPUI site](https://gpui.rs/)
- [README](https://github.com/zed-industries/zed/blob/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/gpui/README.md)
- [Contexts](https://github.com/zed-industries/zed/blob/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/gpui/docs/contexts.md)
- [Key dispatch](https://github.com/zed-industries/zed/blob/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/gpui/docs/key_dispatch.md)
- [Examples](https://github.com/zed-industries/zed/tree/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/gpui/examples):
  hello_world, list_example, uniform_list, set_menus, a11y, tab_stop,
  focus_visible, and svg.
- [Zed UI crate](https://github.com/zed-industries/zed/tree/596dfdcf32d85d4d8d17a8887b4d0979ec7c5e14/crates/ui):
  inspected its manifest and Button implementation. It is GPL-3.0-or-later and
  depends on Zed's theme, menu, component, and macro layers. It is not imported
  or copied. Small original controls use GPUI's existing focus and action APIs.

Research scrape files are kept locally under ignored `.firecrawl/`.
