# Verification

Passing development tests, installed-package checks, and live-model acceptance
are different kinds of evidence. This page records their scope. For commands
and prerequisites, see [Contributing](../CONTRIBUTING.md#required-checks).

## Release baseline

- **Release:** [v0.1.0-preview.2](https://github.com/heath0xFF/plait/releases/tag/v0.1.0-preview.2)
- **Source:** [`5316950`](https://github.com/heath0xFF/plait/tree/5316950cf69035ed99e4c4d35369eb0b4282d589)
- **Platform:** Apple Silicon, macOS 26 CI

The [release workflow](https://github.com/heath0xFF/plait/actions/runs/43888424840)
passed before publication. [Linux CI](https://github.com/heath0xFF/plait/actions/runs/43888417201)
also passed for that source commit. These results do not certify later changes
or runtime behavior on older macOS versions.

| Check | Evidence |
| --- | --- |
| Rust workspace | Formatting, strict Clippy, 90 Mac tests, release builds |
| Production runtime | 105 tests from source and the same 105 with the installed host, Node, and sidecar |
| Durable runtime recovery | Two separate credential-free process-crash probes |
| Packaging | 21 tests, native `plutil`, package contents, and read-only DMG mount/content verification |
| Installed host | Standard service scheduling; three isolated first-start checks within 15 seconds; no developer Node path |
| Independent updates | Client replacement and staged host update preserve active work; activation follows cleanup |
| Native recovery | Actual installed client is terminated during an accepted command, then reopens the same thread and unsent draft |
| Public downloads | All 19 assets downloaded; all eight checksum files, release discovery, and component metadata checked |

The staged host upgrade uses a labeled fixture with unchanged executable code.
It does not establish compatibility with unknown future protocols or storage
migrations. Checksums detect corruption, not publisher identity. The preview is
**unsigned and not notarized**; see [Mac installation](MACOS.md).

## Protocol version 3

[Protocol version 3](HOST.md#protocol-version-3) is implemented in the host,
runtime, and desktop core, after the release baseline above. Its evidence is
development tests only:

- Runtime tests drive the real host binary and sidecar with a scripted provider
  in disposable stores and repositories. They cover history paging and
  oversized text, thread titles, listing and archive, the thread-list
  subscription, model and thinking changes, queued and steering inputs across
  Stop and restart, usage and reasoning, the turn limit, and retry without a
  repeated tool effect.
- Recorded version 1 and 2 replies are compared byte for byte with the current
  host, including for a thread that used version 3 features and a store saved
  before version 3.
- Rust tests cover negotiation from 3 to 2 to 1, request shapes, and the
  thread-list reader, and decode replies recorded from the real host with the
  client's strict types.

Not verified: a live model, a real provider's reasoning or usage figures, the
installed package with a live provider. Desktop integration is covered by the
conversation, composer, and navigation checks below. Thread-list backpressure relies on the code path shared with `watch` and
has no test of its own.

## Recovery and startup regressions

The desktop journal releases its writer lock when its final owner is dropped.
Regression tests cover a duplicated descriptor and a child held before exec.
They check immediate reopen, saved data, and exclusive ownership. Host and command
leases retain their separate lifetimes. See [the recovery fix](https://github.com/heath0xFF/plait/pull/20).

The installed host uses standard service scheduling. Its startup gate creates
three isolated LaunchAgents with separate stores, empty credential homes, and
no auto-restart. Each must answer within the existing 15-second bound. The
[startup correction](https://github.com/heath0xFF/plait/pull/21) also includes a controlled comparison with the retired background policy. These
are service-readiness checks, not native UI latency or live-account acceptance.

## Installed coding check

`runtime/test/dogfood.macos.test.ts` connects the production runtime to a scripted
loopback provider through Pi's real decoder. In a disposable copy of Plait source,
it approves a refactor and regression test, runs offline locked Cargo tests,
disconnects, retries the accepted request, restarts the host, and completes a
follow-up. Assertions cover Git diffs, the unchanged index and pre-existing dirty
work, stable tool/message identities, and absence of repeated execution.

The installed-client gate launches the actual native app against that host. It
terminates the client process during the command, then reopens it without a checkout argument.
The restored draft is supplied by the test, not typed by a person. Reopening
neither submits the draft nor calls the provider again.

Inspected native captures from the release:

- [Accepted command running](https://github.com/heath0xFF/plait/releases/download/v0.1.0-preview.2/installed-client-active.png)
- [Reopened thread, code block, and saved draft](https://github.com/heath0xFF/plait/releases/download/v0.1.0-preview.2/installed-client-coding.png)

These checks establish real tool effects and native process recovery, not
live-model quality, personal-account authentication, or keyboard/IME behavior.
The response labels the scripted provider. No sample-view code ships in the app.

## Coverage boundaries

- **Rust/core:** Git parsing and repository edge cases, unchanged index/worktree
  after reads, stale async results, protocol framing, peer identity, Unicode,
  transcript updates, and private recovery journals.
- **Runtime:** approvals and revocation, dirty-file protection, exact postimages,
  request deduplication, host/sidecar failure, cancellation, and bounded output.
- **macOS sandbox:** denied outside writes, network, sockets, signals, and cache
  access; permitted private build output and scratch IPC. This is not a general
  process container; see the [host contract](HOST.md#safety-boundary).
- **Linux:** headless development and explicit command rejection. Scripted
  lifecycle fixtures are not a working Linux containment backend.
- **Headless UI:** wide/narrow layouts, scroll-state retention, and recovery
  guards with injected test state, on macOS and Linux. Exported macOS images use
  the app's Metal renderer and Core Text offscreen; Linux images use Linux fonts
  and software rendering. Neither validates the native window, input, focus
  that follows input, the accessibility tree, or latency. Conversation text
  selection, copy, link hover, and the jump to the newest row are exercised by
  pointer and key events sent through GPUI's dispatch in these scenes, not by
  a native pointer.

Project navigation has headless scenes for wide and narrow grouping, unread and
approval states, archived threads, inline rename, search, empty results, thread
actions, paused updates, and the older-host fallback. A dispatched UI test switches
checkouts while preserving each draft and rejecting the previous watch. Core
tests cover listing identity guards, activity tracking, preference migration, and
serialized saves. Delayed management replies cannot dismiss a newer rename or
overwrite a newer live summary; reconnect and checkout changes release pending
controls whose old replies are discarded. System notification delivery and
notification-click activation are not exercised by these tests.

The optional headless PNG export is ignored in the normal Rust suite and runs
explicitly when requested; CI does not run it on either platform. See
[UI checks](../CONTRIBUTING.md#ui-checks).

## Provider sign-in

The [provider sign-in operations](HOST.md#provider-sign-in) landed after the
release baseline and have development evidence only:

- Scripted providers cover the sign-in states, cancellation, expiry, bounds,
  key storage, and the absence of secrets in every reply.
- Pi's real registry and credential store, in a disposable agent directory with
  outbound requests blocked, cover key storage and file modes, literal keys, and
  `models.json` edits. Connection checks use a local server.
- The host binary with a scripted sidecar covers a reload during an active run,
  operations over the socket, and sign-in cleanup at shutdown. The production
  sidecar entry is started once against an empty disposable Pi directory.

No test contacts a provider. Pi's real sign-in flows for Anthropic, OpenAI, and
the others, including the browser redirect to the loopback listener and token
refresh afterward, have not been run through the host. They need acceptance
with real accounts through Settings → Providers.

## Settings interaction coverage

Source builds have wide and narrow scenes for provider lists, browser and device
sign-in, optional code entry, failed sign-in, masked API keys, endpoint model
selection, command policy, and Trusted confirmation. These inject host replies;
they do not authenticate to a real provider or save a real credential.

Shortcut scenes send keys through GPUI dispatch to prove that recording intercepts
an existing command, conflicts name it, and rebinding removes the old shortcut.
Secret-field checks keep clipboard contents unchanged and prove that a sent value
cannot return through Undo. Dispatched Tab and Shift-Tab checks traverse endpoint
fields and reach the composer from adjacent controls. Endpoint results only
describe the address that was checked. Pure client tests cover request encoding,
tolerant replies, sign-in states, endpoint validation, and policy copy.

An isolated native source build was inspected against a disposable version 3 host
and checkout. All five Settings tabs, provider entry with masked accessibility
values, endpoint field traversal through Save, shortcut conflict cancellation and
rebinding, command-palette search and dispatch, and cancelling Trusted confirmation
with Return were checked. This exposed and resolved missing field tab stops and
duplicate Settings heading accessibility IDs. The pinned headless test platform
does not activate accessibility trees, so PNG exports cannot cover those IDs.

No real sign-in, credential save, endpoint request, or model run was performed in
that native pass. Exact minimum-size native scrolling, IME, token refresh,
notification delivery, and full VoiceOver acceptance remain separate checks.

### Settings by scope

Source builds split settings by scope: a thread's under its message box, a
project's on its own page, and the app's in Settings. Desktop tests check that
Project settings… for a project that is not active makes it the active one and
then opens its page, that it opens nothing while a thread is being created,
that the same project opens without changing checkout, that the menu bar's
**Project Settings…** opens the active project's page, and that Cmd-comma over
that page opens Settings rather than closing it. A Pi thread's run-settings
menu lists only its three approval choices, even when the project's commands
are trusted. A unit test keeps the Settings tabs' order and their element IDs
in step. A core test keeps the explanation of the default for edits and
commands from promising a sandbox: Settings describes no one project, and the
project's own page says what its commands can reach. Wide and narrow scenes render Settings → New threads (including the
default harness), Settings → Agents, the project's page with the command
policy and its Pi setup (`project-settings`, `agent-settings`), the Sandboxed,
Trusted, and confirmation states there (`policy-*`), and the project's sidebar
menu (`navigation-project-actions`). All of it uses injected host replies.

None of this has been checked in the installed app: not the menu bar item, the
sidebar menu with a real pointer, the header while a settings page is open, a
real policy change saved from the new page, or VoiceOver on either page.

## Paged conversation history

Source builds with a version 3 host have core tests for history/live seams,
duplicate IDs, full fields replacing clipped tails, late pages against newer
snapshots, reasoning-only rows, UTF-16 continuations, explicit oversized text
parts, memory eviction, and retained list identity when prepending. A scripted
owner-only socket test delivers history after the selected thread changes and
checks that the reply is discarded.

The scene walk includes full stored text, loaded older history, loading, text
parts, collapsed and expanded reasoning, selectable reasoning, live thinking,
retry, and ended run states at both window sizes. It dispatches wheel and key
events, selects and copies reasoning, and checks that prepending preserves the
list's item and pixel offset. These are windowless GPUI checks. Native trackpad
feel during page delivery, VoiceOver disclosure behavior, and live-provider
history timing still need native acceptance. No provider calls are needed by
these tests.

Turn records have core tests: a record attaches to the answer that closes a
turn and counts only the paths its finished edits changed, a turn with no tool
calls has a record with nothing to count, a running turn and a turn whose
request is outside the loaded rows have none, and a record moves into its
answer's row when a later message ends the rows. Scenes draw the turn line,
the closing nodes, and the state line in both themes and at both window sizes,
and the scrolled-back scenes show the line broken above the state. Three
things are not exercised by a test or a scene: the row being measured again
when its record moves, the broken line while older history pages are being
read, and a failure message long enough to wrap in the state line.

## Known gaps

The [native acceptance check](#native-acceptance-check) below is implemented,
but **has not yet been run against a window**. Its windowless checks do not
close any of these native gaps.

- Gatekeeper approval, installed authentication, and a live-model source change
  on a user-managed Mac need separate acceptance. CI does not replace them.
- The bundle targets macOS 15+, but CI currently runs on macOS 26. Older-system
  runtime behavior, Intel macOS, and a native Linux client are not validated.
- Full VoiceOver, keyboard focus, IME, and sustained typing/streaming checks
  remain open for the installed release.
- Text selection, link clicks, and hover-revealed copy controls have not been
  checked with a native pointer in the installed app. Links inside conversation
  text are not reachable from the keyboard.
- The released preview has bounded conversation/tool previews. Source builds have
  a paged history view, with native and live-provider history acceptance still
  open. There is no published ADEMark result.
- v0.1.0-preview.7's desktop changes (wrapping tool rows, resizable side
  panels, Cmd-L, the run-settings menu and per-thread approval) are covered by
  development tests and headless scene snapshots. Dragging a panel edge, hover
  text, and the menu have not been checked with a native pointer in the
  installed app.
- Git staging, unstaging, committing, and History (after preview.7) are covered
  by core tests against real temporary repositories (renames, deletions, an
  unborn branch, hooks, filters, a held `index.lock`, paging) and by scene
  snapshots of Changes and History. They have not been exercised with a native
  pointer in the installed app, with Git LFS, or with signed commits.
- The changed-files list is covered by a host test of `changed_files` (a
  written file is listed, a read one is not, unknown threads and version 2
  requests are refused), by Git tests of the line counts (staged and unstaged
  together, deleted, new, binary, ignored, and clean files, and a repository
  without commits), and by a desktop test that counts real files, hides the
  card, and opens a file in Changes. It has not been watched live in the
  installed app.
- The line of controls under the composer is covered by unit tests of its
  texts (the Permissions hover leads with what the mode allows; a harness
  using another mode, and a Pi run under another approval mode than its
  control names, are each one amber sentence) and by scenes of the idle,
  running, reported-mode, and new-thread composers. Scenes do not draw hover
  text, so the sentences moved into hover are checked as text only, and none
  of this has been checked in the installed app.
- The Deny once and Allow once shortcuts are covered by action-table tests
  (both defaults need Command and Option, take no composer or other default
  key, and match the table in [macOS](MACOS.md#application-shortcuts)) and by an
  isolated desktop test that dispatches keys through GPUI. They answer when the
  card's buttons would, and do nothing with no approval, while disconnected,
  with a control request in flight, once answered, with arguments that cannot
  be approved, behind Settings, Changes, or an open command palette, or for
  another thread. A repeated keystroke with no release between answers
  nothing, and a rebound key answers. No host runs in that test, so it checks
  that an answer was sent, not the host's reply. The keys have not been
  pressed in the installed app: real key repeat, key release while Command is
  held, the menu items, and choosing either command in the palette are
  unchecked there.
- Asking about diff lines is covered by unit tests of the quote (line ranges,
  removed-only picks, header lines left out, fences longer than any backtick
  run, long picks summarized) and a desktop test that picks lines in a real
  diff and quotes them into a composer. Native pointer clicks on line numbers
  have not been exercised in the installed app.
- Diffs that start at the change are covered by unit tests of the row mapping
  in `plait-core::patch` (a modified, new, deleted, renamed, mode-changed, and
  binary file, a quoted path, a commit with several files, a file with no
  hunks, and a removed line that reads like a header), by a test against a
  real repository whose commit holds each of those changes, and by scene
  snapshots of Changes, a picked range, a hovered hunk, and a commit with two
  files. The hunk header's context text in secondary color is covered only by
  a unit test of where its ranges end: the fixture's hunks start at line 1,
  so no scene shows a function name after `@@`. Paths written with
  `diff.noprefix` or mnemonic prefixes are named from the `diff` line only in
  part, and none of this has been checked in the installed app.
- Hunk staging is covered against real repositories (staging and unstaging
  one of two hunks, refusing a hunk that moved, refusing filtered files, and
  byte-exact patches with tabs, CRLF, and missing final newlines).
- Discard is covered against real repositories (a file keeps its staged side,
  one hunk of two, an untracked executable deleted and restored with its mode,
  and a restore that refuses to write through a symlink). The native
  confirmation alert has not been exercised in the installed app.
- Push, publish, and fetch are covered against a local bare repository
  (publish, push, fetch revealing a remote commit, a fast-forward pull, a pull
  refused because it would overwrite an uncommitted edit, and refused diverged
  push and pull). Pushing to GitHub over SSH or HTTPS with `gh` sign-in has been checked
  only by asking Git for credentials, not by a push from the installed app.
- Creating and switching branches are covered by core tests against real
  temporary repositories (create and switch back; a switch that would
  overwrite an uncommitted edit refused with Git's message and nothing
  changed; a non-conflicting edit and an untracked file carried; names with
  spaces, `..`, a leading dash, `@{`, and an existing name refused; names with
  slashes; a detached HEAD; a repository with no commits; a held
  `index.lock`; the list's order, 500-branch cap, and current mark; a branch
  checked out in another worktree, which Git refuses), by state tests that
  only the newest list lands and that a switch reloads history, by desktop
  tests that drive the palette with real keys (listing, the Create row only
  for a new name, switching, creating, an invalid name) and refuse both writes
  while another Git write is queued and while a thread runs in the checkout,
  and by scene snapshots of the control and palette at both window sizes.
  The control has not been used with a native pointer in the installed app,
  its hover text has not been seen there, and no switch has run hooks or LFS
  checkout filters from it.

- Per-thread worktrees are covered against real repositories (refused before
  a first commit, created beside the repository on a new branch, and removal
  refused while untracked work remains, keeping the branch), by the Mac sandbox
  suite (`git status` and `git log` in a linked worktree succeed, creating a
  branch is denied), and by a desktop test that creates one and requests its
  thread there. A worktree thread has not been run from the installed app.

## v0.1.0-preview.8 acceptance

Preview.8 completes M2.5 in source and scripted tests. These checks need the
installed app, a native pointer, and a live provider, and are open until
recorded here:

- [ ] Streaming text flows in smoothly on a ProMotion display, and the window
      stops redrawing once a reply has caught up.
- [ ] Appearance follows a live macOS light/dark switch; the titlebar matches
      the theme in System, Light, and Dark; Stillwater is the default dark theme.
- [ ] A Base16 file placed in the themes folder appears after Reload, and a broken
      one is named with its reason.
- [ ] The changed-files card lists a running thread's writes with counts, and a
      row opens the file in Changes.
- [ ] Clicking diff line numbers and **Ask about this** quotes them into the
      thread's message.
- [ ] **New Thread in Worktree** runs a thread on its own branch, and removing it
      refuses while work is uncommitted.
- [ ] Pull fast-forwards from Changes and refuses a diverged branch.
- [ ] The sidebar's filter chips, bell, and refresh controls work with the pointer
      and show their names on hover.
- [ ] Cmd-P opens a file in the Files tab; Cmd-F, Cmd-G, and Cmd-Shift-G find
      and step; a file an agent edits rereads when the window is focused again.

## Native acceptance check

`scripts/acceptance-macos.sh` is a development check of a separate native
process. **It has not yet been run against a window.** Its checks below are
intended coverage, not native acceptance evidence. The first real run will
most likely stop at Accessibility permission or expose a difference in GPUI's
live accessibility tree, field focus, or window geometry. Palette roles were
checked against the pinned `accesskit_macos` source, but their live names and
press actions still need that run. The text-selection coordinates, a free
stretch of the header, and macOS full-screen transitions are especially likely
to need adjustment. A failure keeps the observation instead of converting it
to a pass or silently using a pointer click.

Build the normal development executables and runtime dependencies with
`bash scripts/check.sh`, then run the check when the computer is free:

```sh
bash scripts/acceptance-macos.sh
# Skip bar and panel drags, double-click, and conversation text selection:
bash scripts/acceptance-macos.sh --no-pointer
# Keep machine-readable results and diagnostics in a new directory:
bash scripts/acceptance-macos.sh --artifacts /tmp/plait-acceptance-results
```

It requires the repository's normal macOS development tools (Python 3, Git,
Xcode's Swift compiler, and Node for the development host). It installs no
additional packages. `--app /path/to/plait` and `--host /path/to/plait-host`
select already-built executables, including extracted CI packages. The
terminal or runner needs **Accessibility** permission; macOS may also list
the compiled `axdrive` helper. **Screen Recording** permits failure captures.
The check never grants permissions or changes the system double-click setting.
It uses no Apple events, so it needs no Automation permission. A missing
Accessibility grant skips the native items with a reason. An inaccessible
window after permission was granted fails setup. Skips are not passes.

The normal run announces that it takes the pointer for a few minutes.
`--no-pointer` still brings its own window forward and sends native key events;
it is not a background check to run while typing elsewhere. Controls are
pressed with the accessibility API, keys go to the launched PID, and only the
six pointer items below post pointer events. Before a pointer gesture the
helper checks that its starting point belongs to that PID, then restores the
pointer's original position. The text-selection check temporarily uses the
clipboard and restores every representation it could read; clipboard contents
are never written to diagnostics.

Isolation is independent of the executable name. A private temporary folder
contains a copy of the desktop executable, `PLAIT_HOST_STORE`,
`PLAIT_DESKTOP_STATE_DIR`, `PLAIT_DESKTOP_PREFERENCES_FILE`, a disposable home
and Pi configuration, and a Git repository with two branches and two changed
hunks. The app and host receive a small environment without inherited provider
credentials or Git overrides; Git ignores system/global configuration, uses a
test identity, and has signing and hooks disabled. The current system
double-click choice is read once and mirrored into the disposable home's
global preferences, so isolating CF preferences does not change that choice.
Nothing is installed, and
no LaunchAgent is registered. Only the owned PIDs/process group are stopped.
The person's installed Plait may remain running.

Conversation data comes from the **production host and Pi runtime**, talking
to a scripted Anthropic-shaped SSE endpoint on loopback. Its event format
follows `runtime/test/support/replay.ts`; journal seeding follows
`runtime/test/support/installed-ui.ts`. A fixture model and a non-secret key
exist only in the private Pi directory. A real host thread holds the scripted
reply, pending edit approvals, and a deliberately slow stream. The desktop
reads it through the normal socket protocol. No sample view, hidden app mode,
real model, provider credential, or external service supplies the transcript.

Each row below corresponds to a reported item (the four workspace tabs and
seven Settings tabs each have separate results):

| Item | What must be observed | Pointer |
| --- | --- | --- |
| Fixture isolation | Private host and desktop state, test Git identity, scripted conversation | No |
| Native setup / permissions | PID-owned window and seeded assistant message; permission state recorded | No |
| Traffic lights, sidebar hidden | All three light frames centered within 3 points of the 44-point bar, first about 14 points in, header toggle at least 4 points clear | No |
| Drag sidebar bar | Empty rail strip moves the window; header control still activates | Yes |
| Drag header bar | Empty header moves the window with sidebar hidden; header control still activates | Yes |
| Double-click bar | Current system setting produces minimize, no movement, or resize; setting never changed | Yes |
| Full screen and back | Native full-screen state changes both ways; header control responds afterwards | No |
| Sidebar toggle and shortcut | AXPress and Cmd-B hide and restore it | No |
| Inspector toggle and shortcut | AXPress and Cmd-L hide and restore it; skipped on a display too narrow for it | No |
| Resize sidebar | Edge moves and new width is saved in private preferences | Yes |
| Resize inspector | Edge moves and new width is saved; skipped on a narrow display | Yes |
| Conversation tab | Selected state and message field | No |
| Changes tab | Selected state and commit field | No |
| History tab | Selected state and fixture baseline commit | No |
| Files tab | Selected state and Open File control | No |
| Settings | Cmd-comma opens Settings and Done is available | No |
| New threads settings | Selected tab and Models section | No |
| Providers settings | Selected tab and Find provider field | No |
| Agents settings | Selected tab and Agent harnesses section | No |
| Keyboard settings | Selected tab and Find shortcut field | No |
| Editor settings | Selected tab and custom-command fields | No |
| Appearance settings | Selected tab and Code font field | No |
| About settings | Selected tab and About and updates section | No |
| Project settings | Project's sidebar menu opens its Commands settings and returns | No |
| Command palette | Cmd-K, typing and Return dispatch Changes | No |
| Find in file | Cmd-F; three fixture matches; next/previous shortcuts and buttons; Escape | No |
| Find in conversation | Same sequence against the host's three text matches | No |
| Composer focus after tab switches | Clicked tabs and Cmd-Shift-1/2/3 preserve draft, return focus, and route later shortcuts and Find to the right view | No |
| Switch branch | Picker selection and actual Git HEAD agree | No |
| Create branch | New picker entry creates and switches the disposable branch | No |
| Stage one hunk | First hunk enters Git's index; second remains unstaged | No |
| Commit staged hunk | New commit and expected subject; empty index; second hunk retained | No |
| Custom handoff settings | Typed commands saved to private preferences | No |
| Open in Editor | Recorder receives exact file, current find-match line, and project arguments | No |
| Open Terminal Here | Cmd-Option-T recorder receives exact project argument | No |
| Show in Finder | Control is found; activation explicitly skipped (see below) | No |
| Long message | Native Unicode key events produce an exact long draft, grow the field, and save it without sending | No |
| Conversation text selection | Drag and Cmd-C copy a proper substring of the fixture reply | Yes |
| Allow approval shortcut | Cmd-Option-Y resolves host approval and changes the exact fixture file | No |
| Deny approval shortcut | Cmd-Option-period resolves host approval and leaves the file unchanged | No |
| Streaming reply | At least two lengths of the writing label, followed by final assistant text | No |
| Follow conversation link | Explicitly skipped because it would open the default browser | No |

The script cannot substitute a custom command for **Show in Finder**:
`plait_core::handoff::show_in_finder` always starts `/usr/bin/open -R`.
It finds that control but does not activate it. Conversation links similarly
open the system browser without a configurable override. Those two actions
remain manual acceptance; this check never opens Finder, an editor, a terminal,
or a browser. The editor and terminal custom commands only write their received
arguments to files. Testing actual editor line navigation, Finder selection,
Terminal/Rex startup and URL following still requires those applications.

Other limits: AX geometry cannot certify drawn pixels or macOS 15 appearance;
AXPress does not prove a mouse click's hit testing; the full-screen check does
not visually inspect the hidden lights; double-click tests only the current
system choice and checks resizing, not exact Fill margins. One long draft and
one short stream do not establish sustained performance, caret/IME correctness,
smoothness, scrolling, hover behavior, or VoiceOver usability. Settings checks
verify navigation and named contents, not provider sign-in or every control.
Live models, authentication, recovery, Stop, updates, signed commits, hooks,
Git LFS and older systems remain separate checks.

Output has one `PASS`, `FAIL` or `SKIPPED` line per item and a summary. Any
failure exits nonzero; an all-skipped native run exits zero with explicit skips.
Each failure attempts an AX dump and **only that PID's window screenshot**,
using its window ID with `screencapture -l`; a capture failure gets its own
reason file. Failures and skips retain the disposable folder and print its
path. A completely passing run removes it. `--artifacts` copies generated
results, logs, argument recordings and screenshots to the requested new
folder. It does not copy host databases or executables. CI uploads these plus
the console output. The advisory `native-acceptance` job runs only on pushes
to `main`, reuses the package job's artifact without installing it, and has
`continue-on-error`. There was no existing non-gating macOS job to append it
to; the additional job does no Rust build. Tagged releases do not run or wait
for it.

Verified without a window: Swift compilation and its pure selector/key/Unicode/
gap tests; Python tests of environment isolation, failure/skip reporting,
traffic-light bounds and replay frames; the production host/replay transcript
and allow/deny edit effects, growing stream, and argument recorder; shell syntax; workflow YAML and its non-gating,
main-only condition; and `bash scripts/check.sh` (format, Clippy, workspace and
runtime tests, release builds). The existing smoke AppleScript was compiled
with `osacompile`, not executed; this check adds no AppleScript or JXA.
ShellCheck was not installed during this verification. The check's own
windowless entry points are:

```sh
bash scripts/acceptance-macos.sh --self-test
bash scripts/acceptance-macos.sh --fixture-test --host target/debug/plait-host
```

Neither entry point launches the desktop, reads an accessibility tree, or
sends keyboard/pointer events. They do not replace the first real native run.

## Manual release acceptance

Once the native acceptance check above has passed its first real-window run,
run it before tagging and inspect every failure and skip, then complete the
remaining manual checks below. Until then it is an unproven advisory check.

Use a disposable checkout and private host/session directories. Do not reuse
another person's conversation, change OS security settings, or inject global
keystrokes into their desktop.

1. Install the downloaded package under normal macOS policy and authenticate
   with an account authorized for the test.
2. Check repository/model selection, native typing and IME, keyboard navigation,
   VoiceOver, and the 900 × 600 minimum window.
   - [ ] The window bar: the traffic lights sit centered on the 44-point bar
         about 14 points in, clear of the header with the sidebar hidden; an
         empty part of the rail strip or header drags the window while its
         controls still click; double-clicking there follows the system
         setting; full screen hides the lights and the header returns to the
         edge. Only where the lights sit with the rail shown has been checked
         in a real window.
3. Ask for a small real source change and test. Review exact approvals, actual
   output, the Git diff, and preservation of unrelated work.
4. Close/reopen during work, then test Stop, provider failure, and recovery.
   Confirm stable identities and no automatic replay.
5. Check independent client/host updates, including cleanup before activation.

Record the build, OS, model/provider, steps, and result. Do not publish credentials,
private prompts, or authentication seeds. Performance claims need separate,
repeatable measurements; see the [ADEMark plan](ADEMARK.md).

## Self-hosting tool and context coverage

Runtime integration tests exercise ranged reads beyond the former whole-file
bound, exact edits guarded by a full fingerprint, Unicode page boundaries,
unsafe paths, and stale edit rejection. Command-output tests cover scope,
retention, explicit size limits, incomplete capture, database reopen, and a
failed real sandboxed command whose earlier diagnostics remain readable after
a host restart without rerunning it.

Real-host tests trigger the pinned runtime's automatic compaction policy with
a scripted provider. They verify that the model keeps the task goal, original
history remains available, internal summaries do not become user messages,
restart during summarization makes no new requests until submission, and a
failed required summary can recover. They establish request sequencing and
storage behavior, not the quality of summaries produced by a live model.

An installed source build was also exercised with a live provider on Plait's
actual Rust and TypeScript code. The run made a cross-language change, added
regression tests and documentation, accepted review corrections in follow-up
messages, and completed a queued correction. Closing the desktop during work
left the host running; reopening restored the selected thread and an unsent
draft. The source installer’s active-run check refused to proceed while that run was active.

With Trusted commands explicitly enabled for that checkout, the agent ran the
ordinary full development gate: 332 Rust tests passed (one image-export test
ignored), 187 runtime tests passed including the macOS sandbox tests, and the
release-script suite passed 23 tests. Two permission fixtures were made explicit
so the gate also passes under the host's private `0077` umask. No production
permission checks or test assertions were weakened. This establishes a live
project-development loop; it does not certify every provider, native input
method, or long-running task. Automatic compaction remains covered by the
scripted tests above rather than this live run.

## Skills and user instructions

Runtime tests discover a user skill, add a project skill only for a trusted
project, follow Pi's precedence and size bound for the user-level context file,
map reads into a skill folder (refusing relative paths, prefix siblings, `..`,
and the agent directory), read a skill file through the host helper, and send
`/skill:name` in Pi's block. Regression tests refuse a skill file swapped for a
link to a dummy credential after discovery, grant a standalone skill in a
scratch `~/.ssh` only its own file, and skip skill folders inside a credential
folder or at the host store. A protocol test shows an expanded message in
history as typed. A real-sandbox test reads a skill folder only when it is
passed, and cannot write it. Test runtimes ignore the machine's own skills.
Template tests check Plait's argument parser and substitution against Pi's own
functions over a set of quoting and placeholder cases, load project templates
only for a trusted project, refuse a template swapped for a link after
discovery and links to non-Markdown or credential files, and keep the
`agent_resources` reply free of file contents and under the 1 MiB frame with
oversized lists. A host test sends `agent_resources` and sees trust follow the
command policy. Desktop tests cover agent command ranking and clashes, and
scenes render the project's settings page and the `/` list from sample state; choosing a
command inserts it. A live model receiving an expanded template in the
installed app has not been checked. Discovery on
a real Pi setup with 66 skills (29 turned off in settings) took 7–20 ms per run.
A live model choosing and following a skill in the installed app has not been
checked.

## MCP configuration listing

`runtime/test/mcp.test.ts` compares accepted servers and rejected entries with
Pi 1.5.2's actual configuration loader: trusted project replacement, restricted
`enabled`/`exposure`/`toolExposure` overrides, namespace clashes, provider-auth
restrictions, and invalid transport and OAuth settings. Tests cover BOM input,
malformed and oversized files, bounded replies, Pi's built-in MCP disable
setting, and saved project choices under both command policies. Store reopen
and directory-identity tests cover persistence and scope. A host protocol test
saves a switch, reads it back, restarts the host, and verifies Pi's configuration
was not edited.

A guarded listing test supplies commands, environment placeholders, secret
arguments, headers, URL credentials and paths, and OAuth tokens while replacing
process and network entry points with failures. Listing calls none of them;
serialized reports contain none of those values. OAuth tests cover normalized
namespace/URL keys, legacy keys without migration, wrong-URL rejection, saved
tokens, and unknown requirements for an uncontacted HTTP server.

Core tests decode older hosts and unknown fields/transports and serialize the
version 3 switch request. Desktop tests cover state wording, source details,
and stale connection/checkout replies. `project-settings-mcp-wide` and
`project-settings-mcp-narrow` draw the section from injected metadata;
`project-settings-no-mcp-*` draws its absence. The headless scenes do not prove
native pointer interaction or accessibility. No MCP server, browser sign-in,
network connection, model tool call, or installed-app acceptance is exercised:
this stage only lists configuration and saves project switches.

## Agent harnesses and Codex

Protocol tests accept the version 4 operations and fields only with version 4,
keep version 3 snapshots and summaries free of the harness, and read threads
saved earlier as Pi. Runtime tests list harnesses against a scripted check
(ready, not installed, too old, signed out), reuse a recent check, check again
after a new path, and close the server started only to ask. They refuse a
model from the other harness at creation and in `configure_thread`, and map
thinking levels to Codex's efforts. The Codex adapter's own tests check
version parsing and grading, the JSON line transport (handshake, replies,
refused server requests, exit, oversized frames, bounded diagnostics),
translation of scripted turns through the same `update()` Pi threads use, and
approval wording and replies.

End-to-end runtime tests drive Codex threads through a scripted fake
app-server that echoes the settings it is sent: an Ask turn's command
approval, a declined edit that lists no changed file, a second run that
resumes the same Codex thread, the Full access default sending `never` and no
sandbox, Approve for me sending Codex's reviewer, a mode change during a run
that the next turn uses, the default mode setting, a Codex held to another
mode reporting it, a steering message joining the running turn, Stop sending
`turn/interrupt`, Codex exiting mid-turn ending the run with a fixed error,
history read live and again after a restart without starting Codex, and every
server closing once idle. The adapter's message types were checked against
`codex app-server` 0.169.0 with a handshake that starts no thread or turn
(initialize, model list, account state, and refusals).

Opt-in live tests (`PLAIT_LIVE=codex npm run test:live` in `runtime/`) run real
Codex turns through the runtime with the installed Codex and its own sign-in,
on Codex's smallest listed model. They passed against Codex 0.169.0 on 2026-11-17:
Full access running a command without asking and reporting `full_access`, a
second run recalling the first, Read only asking before a write and the
approved file landing, a steering message joining the turn and shaping its
answer, Stop ending a running command within 15 seconds, and history reading
after a restart without starting Codex. They assert structure, not wording,
and are not part of `npm test` or `scripts/check.sh`. Ask and Approve for me
have not been run live, nor has what Codex's reviewer decides.

Measured on a replayed stream of 7,081 Codex events (20 answers and 20
commands, after 100 earlier entries): translation cost 0.6 µs per event, and
rebuilding the snapshot cost 0.16 ms on average and 0.71 ms at worst, under the
5 ms per event target even if every event rebuilt it. Streaming updates are
also coalesced to one per 16 ms and are not written to disk. Frame cost in the
desktop while a Codex thread streams has not been measured.

## Claude Code

Protocol tests accept `set_up_harness`, the `claude` harness, and its modes
only with version 4, and refuse setting up any other harness. Setup tests
build archives in the test and serve them through a stand-in for `fetch`, so
nothing is downloaded: a hash mismatch, a registry error, a truncated archive,
a path that climbs out with `..`, a link, an archive missing the license, and
a compression bomb each install nothing; a good archive leaves exactly the
four kept files in place with no staging folder; a second setup downloads
nothing; and a changed `sdk.mjs` is refused at load and replaced by setting
up again. A test checks that the pinned URL and `integrity` are the ones
`package-lock.json` records, that the SDK is a development dependency only,
and that the dev copy's `sdk.mjs` has the pinned SHA-256. The tar reader was
also run on the real 0.3.326 tarball, whose SHA-512 matched the pin, and setup
with that tarball finished in about 20 ms.

Runtime tests drive Claude Code threads through the SDK's dev copy and a
scripted stand-in for `claude` that follows the SDK's control protocol and,
like Claude Code, asks nothing in Auto or Bypass and nothing for edits in
Accept edits. They cover: the entry listed as `setup_required` before setup
with nothing looked for or started, threads refused then, and a run refused
once the copy is gone; setup through the operation, then the normal checks;
an Ask turn's command approval, its transcript, usage, and reported mode, and
history read again after a restart without starting Claude Code; Auto, the
default, never asking; a second run resuming the same session; a mode change
during a run reaching Claude Code and `reported` following it; a steering
message placed once in the same process; Stop interrupting, cancelling what
was queued, and running nothing after; an edit listed as changed; Claude Code
exiting mid-turn and a lost session each failing with a fixed message; and a
signed-out Claude Code, a model it does not offer, and another harness's model
or mode refused. The adapter's own tests check version grading, sign-in
parsing, the model list, approvals and their replies, the transcript
translation through the same `update()` Pi threads use, and the session
against the stand-in.

Checked against a real Claude Code 2.1.326 without sending any message: the
version and sign-in checks, the harness entry (ready, 14 models, about 0.26
seconds including the SDK load), every flag a run starts it with, mode changes
in every mode, and interrupting with no turn running. Opt-in live tests
(`PLAIT_LIVE=claude npm run test:live` in `runtime/`) run real turns through
the runtime on the smallest model Claude Code lists, with its own sign-in:
Ask asking before a command and the approved command running, a second run
recalling the first, a steering message placed and answered, a mode change
during a run that Claude Code reports, Stop ending a turn within 15 seconds,
and history reading after a restart without starting Claude Code. They passed
against Claude Code 2.1.326, signed in with a Claude subscription, on
2026-11-17. On one earlier run the model answered the Stop case without
starting its command; the test now says so instead of timing out. Plan, the auto mode's own safety decisions, and MCP tool
approvals are not covered by any live test.

Measured on a replayed stream of 4,202 Claude Code events: translation cost
about 0.15 µs per event, and translating and rebuilding the snapshot on every
event about 52 µs per event, under the 5 ms target. Streaming updates are
coalesced to one per 16 ms and are not written to disk.

## Files

Core tests read text, binary, non-UTF-8, and oversized files; refuse symlinks,
paths outside the checkout, directories, missing files, and a FIFO; split CRLF
and final lines; notice a changed stamp; and find with letter case ignored
inside and outside ASCII, no overlaps, and the 1,000-match cap. Desktop tests
expand tabs and clip long lines on character boundaries and draw matches in the
conversation's neutral fill, the current one underlined, and a harness test
opens a real file, finds in it, finds the kept query again on reopening,
rereads it after an edit on disk, keeps the last version after deletion, finds
non-ASCII text in either case, describes a binary file, and lists files in
quick open. Scenes draw the tab, find with the current match apart from the
others, and quick open. In the frame bench a 20,000-line highlighted
file drew in 0.6 ms at rest and 1.2 ms per frame while wheel-scrolling. Native
pointer use, find shortcuts, and reloading after an agent's edit are open in the
[preview.8 acceptance](#v010-preview8-acceptance) list.

## Ways out

Core tests split custom command lines with single quotes, double quotes, and
backslashes, refuse unclosed quotes, a trailing backslash, line breaks, a
missing program, unknown placeholders, an editor command without `{file}`, and
a terminal command with anything but `{project}`, and fill placeholders inside
each word in one pass. The two escaping layers are tested on paths with spaces,
single and double quotes, backslashes, `$`, backticks, line breaks, tabs,
non-ASCII text, shell punctuation, a leading dash, and an empty path. On a Mac,
`/usr/bin/osascript` reads each AppleScript literal back unchanged (`return`
only, which reaches no application), and the full Rex command line, read back
the same way and run by `/bin/sh` and `/bin/zsh` with `printf` in place of
Neovim, gives every word back whole. An ignored test compiles both Rex scripts
for every such path with `osacompile` against Rex's dictionary; it compiles and
does not run them, and needs Rex installed. A preferences test round-trips each
choice, reads an older file as the system default and Terminal, refuses an
unusable custom command, and reads a hand-edited one as the default.

Desktop harness tests record the command each action would start; test builds
never start one. They check the file and line picked in Files (the first line
on screen, after scrolling, a line asked for but not yet drawn, and the current
find match), in Changes (the first change, a picked line, a staged diff that
begins with a removed line, and a deleted file that has nothing to open), the
Option-Command-E shortcut, Show in Finder and Open Terminal Here arguments,
the Rex scripts, the hover text, and a saved command that no longer reads. A
Settings test refuses an unusable command, saves a usable one to the test's
own preferences file, and shows it again on reopening. Scenes draw Settings →
Editor with ready choices and with custom commands, and the Files and Changes
headers with their Editor and Finder controls.

None of this has been run against a real editor, Finder, Terminal, or Rex:
whether Rex opens the tab and Neovim lands on the line, what each app does
when it is not running, and the two-second failure window are unchecked.

## Find in the conversation

Core tests match with letter case ignored, inside and outside ASCII, with
ranges on character boundaries; find several matches in one block, without
overlaps; keep matches in conversation order across blocks and messages with
each one's row; skip list markers, the streaming reply, reasoning, and tool
output; return nothing for an empty query; stop at 1,000 matches, keeping the
newest; and step next and previous with wrap-around. Desktop harness tests
check that Cmd-F opens the conversation's bar and not the Files bar, and the
reverse in Files; that Escape closes it and returns focus to the composer; that
a tab that hides the composer, chosen by shortcut or by click, takes the
keyboard from it so later shortcuts and Cmd-F in Files work, and that coming
back by shortcut gives the composer the keyboard again with its draft and
caret; that
a result for an older query or an older projection is dropped and the search
runs again; that next and previous scroll each match into the list, across
rows far apart and wrapping at both ends; and that a new reply is searched
while the current match and scroll position stay. Scenes draw the bar with
matches highlighted, with no matches, and with earlier messages not loaded, at
both window sizes. In the frame bench (release profile, November 2026), the
3,750-row conversation drew in 1.13 ms per frame at the median with 750
matches highlighted, against 1.02 ms without find, and a new query searched
every row in the background and drew its result in 4.6 ms at the median.
The scene harness gives "loaded messages only" its state
directly; reaching it through a version 3 host's paged history, and the hover
text, have not been checked in the installed app, nor have Cmd-G from the menu
bar and VoiceOver's reading of the bar.

## Themes

Core tests parse both published Base16 layouts, name what is wrong with a broken
file, read a themes folder, keep older preference files loading, and check every
shipped theme against the contrast floors (text 7:1, secondary text 4.5:1, hints
and accents 3:1). Snapshot scenes draw the conversation, Changes, and theme
settings in Plait Light, and the settings in the default Stillwater; every
other scene draws in Stillwater, and one draws Plait Dark. Following a
live macOS appearance switch, the native titlebar in each appearance, and
reading a user's themes folder have not been checked in the installed app.

The code font is covered by a core test (an older preferences file keeps the
system font, a named family round-trips, a blank name is no choice), by
desktop tests of the name matching (a family is found whatever its case;
offered names lead with those that start with what was typed, and leave out
system families), and by two desktop tests on a Mac that read the family code
is set to draw in and the preferences in memory. One types into the setting: a
family that is not installed changes neither, `menlo` selects Menlo, a
half-typed name keeps the last choice, Helvetica is refused because its
characters differ in width, reopening Settings shows the choice again, and an
empty field returns to the system's font. The other opens Settings before a
preferences file that names Menlo is read, as can happen at launch: Menlo is
then selected, shown in the field, and still in the file. Every scene draws
code in the system's monospaced font, and the Appearance scene shows the empty
field. The tests do not look at drawn pixels. The offered names as drawn, a
font installed or removed while Plait runs, and the setting in the installed
app have not been checked.

## Window bar

Desktop unit tests check that the traffic lights' offset centers them on the
44-point bar, that the header clears them only while the rail is hidden and the
window is not in full screen, and which presses may move the window: a press a
control took never does, a second press is a double-click. A desktop test sends
pointer presses through GPUI's dispatch: the rail's empty strip, the space the
header leaves under the lights, and the header's empty middle arm a move; the
rail's resize handle and the sidebar toggle do not, and the toggle's click
still works; in GPUI's test full screen the toggle returns to the edge. Every
release disarms. The test platform cannot move a window or perform the
system's double-click action, so the test stops at arming. Scenes show the
rail's top strip empty, where macOS draws the lights.

One capture of a development build's window, on macOS 26 with the rail shown,
shows the traffic lights where they are meant to be: about 14 points in, their
centers level with the header's title, and ending 10 points short of where
the header starts when the rail is hidden. Nothing else has been checked in
the native window: the lights on macOS 15, the header with the rail hidden,
dragging the window from the strip and the header, the double-click action,
and full screen are open in the
[manual acceptance](#manual-release-acceptance) list.

## Measuring frame cost

```sh
bash scripts/bench-frames.sh          # release profile
bash scripts/bench-frames.sh --debug  # quick, unoptimized
```

The bench draws generated heavy scenes (2,000 changed files, 20,000-line
patches, 5,000 commits, a 1,500-message thread) and times each frame and each
action-to-next-frame switch, reporting min, median, p95, and max. A frame is
`Window::draw` on GPUI's test platform with Core Text shaping: render, layout,
prepaint, and paint into a scene. These are headless CPU measurements. They
exclude GPU submission, presentation, real input latency, and the installed
app's process, so they do not establish the 60 FPS or 50 ms targets in the
[roadmap](ROADMAP.md) for the shipped app.

On an Apple Silicon development Mac (release profile, November 2026), every
heavy scene drew in under 3 ms per frame, tab and cached file or thread
switches took under 5 ms at the median (12 ms p95 for a 1,500-message thread),
and selecting a file whose diff is not cached took about 34 ms, almost all of
it the background Git read. Warm launch was not measured.

The streaming row draws frames 8 ms apart, as a 120 Hz display asks for them,
while a burst of about 10 KB in short paragraphs is revealed. Each frame took
1.6 ms on average and 3.4 ms at p95, inside the 8.3 ms a 120 Hz frame allows.
A scripted test checks that streamed text trails, catches up within a bounded
number of frames, and then stops requesting frames. How smooth it looks on a
real display, against a live provider, has not been checked in the installed
app.
