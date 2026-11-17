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

## Manual release acceptance

Use a disposable checkout and private host/session directories. Do not reuse
another person's conversation, change OS security settings, or inject global
keystrokes into their desktop.

1. Install the downloaded package under normal macOS policy and authenticate
   with an account authorized for the test.
2. Check repository/model selection, native typing and IME, keyboard navigation,
   VoiceOver, and the 900 × 600 minimum window.
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
failed required summary can recover. They establish orchestration and storage
behavior, not the quality of summaries produced by a live model.

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
