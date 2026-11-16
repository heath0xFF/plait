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
  guards with injected test state. Linux fonts and software rendering do not
  validate the Mac renderer or native input latency.

The optional headless PNG export is ignored in the normal Rust suite and runs
explicitly when requested. See [UI checks](../CONTRIBUTING.md#ui-checks).

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
with real accounts once the Settings UI exists.

## Known gaps

- Gatekeeper approval, installed authentication, and a live-model source change
  on a user-managed Mac need separate acceptance. CI does not replace them.
- The bundle targets macOS 15+, but CI currently runs on macOS 26. Older-system
  runtime behavior, Intel macOS, and a native Linux client are not validated.
- Full VoiceOver, keyboard focus, IME, and sustained typing/streaming checks
  remain open for the installed release.
- Conversation and tool previews are bounded. There is no paginated full-history
  view, automatic updater, or published ADEMark result.

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
