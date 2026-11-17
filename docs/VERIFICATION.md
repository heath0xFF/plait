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
the closing nodes, and the state line in both themes and at both window sizes.
Three things are not exercised by a test or a scene: the row being measured
again when its record moves, the broken line while older history is being
read, and a failure message long enough to wrap in the state line.

## Known gaps

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
- Asking about diff lines is covered by unit tests of the quote (line ranges,
  removed-only picks, header lines left out, fences longer than any backtick
  run, long picks summarized) and a desktop test that picks lines in a real
  diff and quotes them into a composer. Native pointer clicks on line numbers
  have not been exercised in the installed app.
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

## Manual release acceptance

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
scenes render Settings → Agent and the `/` list from sample state; choosing a
command inserts it. A live model receiving an expanded template in the
installed app has not been checked. Discovery on
a real Pi setup with 66 skills (29 turned off in settings) took 7–20 ms per run.
A live model choosing and following a skill in the installed app has not been
checked.

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
and final lines; notice a changed stamp; and find with ASCII case folding, exact
non-ASCII matches, no overlaps, and the 1,000-match cap. Desktop tests expand
tabs and clip long lines on character boundaries, and a harness test opens a
real file, finds in it, rereads it after an edit on disk, keeps the last version
after deletion, describes a binary file, and lists files in quick open. Scenes
draw the tab, find, and quick open. In the frame bench a 20,000-line highlighted
file drew in 0.6 ms at rest and 1.2 ms per frame while wheel-scrolling. Native
pointer use, find shortcuts, and reloading after an agent's edit are open in the
[preview.8 acceptance](#v010-preview8-acceptance) list.

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
