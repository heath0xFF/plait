# ADEMark readiness and performance plan

**Status: a target, not a result.** Plait has no ADEMark measurements, adapter,
or verified ranking. [Issue #15](https://github.com/heath0xFF/plait/issues/15)
tracks this work. It stays within M2 local execution; it does not add remote
execution or an orchestration framework.

The goal is strong native launch, input, memory, idle CPU, and conversation
performance, with low token overhead. Measure the installed client and host,
not just the GPUI process. Keep the safety and independent-host design intact.

## Research baseline

Reviewed on 2026-11-16:

- Plait source: `25bec03b594825c45d509808bb2c8207210e644f`.
- [ADEMark](https://ademark.dev), pinned to
  [`119a5f7b99247cffbe7f96bb2c36c03496cf3018`](https://github.com/zqkra/ademark/tree/119a5f7b99247cffbe7f96bb2c36c03496cf3018).
- Upstream [method](https://github.com/zqkra/ademark/blob/119a5f7b99247cffbe7f96bb2c36c03496cf3018/METHODOLOGY.md),
  [test procedures](https://github.com/zqkra/ademark/blob/119a5f7b99247cffbe7f96bb2c36c03496cf3018/TESTS.md),
  [adapter guide](https://github.com/zqkra/ademark/blob/119a5f7b99247cffbe7f96bb2c36c03496cf3018/GUIDE.md),
  and [fairness notes](https://github.com/zqkra/ademark/blob/119a5f7b99247cffbe7f96bb2c36c03496cf3018/FAIRNESS.md).

ADEMark v1 ranks each test separately. It has no composite score. Its site
orders apps by tests led, then top-third finishes; this is not an overall
quality measure. V1 publishes speed/weight (Track A), tokens (Track B), and
orchestration (C1). Reliability tests C2–C6 are not part of the published run.
Track F records product facts without a score.

The reported reference setup is a Mac mini M4 with 16 GB, macOS 27.0.1,
Claude Opus 5.5 at high effort, and a 60 Hz display. This is upstream's setup,
not a Plait test result. Track A normally uses a local replay server, not a
live model. Track B and C spend real model usage.

## What fits Plait

| Tests | What to measure | Current position and required work |
| --- | --- | --- |
| A1–A3: cold start, relaunch, new thread | Time until the composer accepts typing | Native client and lazy thread execution exist. Need installed Mac calibration and input-to-pixel evidence; root-render timings are not these metrics. |
| A4: first word | Enter to the first visible answer, against replay | Validate replay routing through Pi and the installed host. Profile transport, projection, and presentation separately. |
| A5/A6/A9: idle memory, CPU, post-quit memory | All attributed processes, including the persistent host | Include Rust host, Node sidecar, LaunchAgent, and children. Record both normal quit and retained sessions; do not stop the host to improve the number. |
| A7/A8: memory and typing under load | Five concurrent streams | Blocked by the current one-active-run-per-checkout policy. Typing during a run is supported; sending another turn while busy is not. |
| A10: 20 threads | Idle memory growth and typing with five streams | Test retained UI and host state. The load part has the same concurrency gap as A7/A8. |
| A11: long thread | Open, scroll, and switch about 2,000 visible items | Lists are virtualized, but the client has only a bounded transcript tail. History access and run limits need review. |
| A12: leak | Memory growth during extended real work | No measured baseline. Upstream samples during Track C; a standalone Plait soak test must have its own label. |
| B1–B4: tokens and repository rules | Request anatomy, side calls, usage, and AGENTS.md canary | Root AGENTS.md is loaded explicitly. Need real captures, exact model/effort settings, and token evidence; source inspection is not a B4 pass. |
| C1: orchestration | Lead/worker fan-out and integration | Not supported. Do not add swarms or required worktrees for a rank. |
| C2–C6: reliability | Follow-ups, input availability, agent/network/app failure | Relevant future checks. Existing Stop, reconnect, and no-replay tests are not ADEMark results. |

Plait is native, but it runs its **own Pi Durable harness**, not Claude Code.
The adapter must disclose this. Agree the upstream family and process groups;
do not label the Node sidecar as a Claude CLI or omit it from totals.

## First implemented slice: event-driven host watches

The host no longer polls the sidecar every 150 ms. A private change channel
wakes only matching subscriptions; each client reads the latest bounded
snapshot. Notifications coalesce under backpressure. Disconnects release idle
subscriptions immediately, and notification-channel EOF detects sidecar death
without a periodic process check. The public protocol and durable schema stay
unchanged. See [the host contract](HOST.md#transport).

Run the credential-free transport probe with:

```sh
cargo build --locked --release -p plait-host
node runtime/bench/watch.mjs
```

This uses a labeled scripted sidecar, not Pi, a live model, or the desktop.
It measures change-to-socket delivery and sidecar request counts with one and
20 connected watchers. It does not measure idle CPU, frame presentation,
five running agents, or ADEMark A10. The output includes all 40 latency samples
per watcher count. Run builds and probes in a named Rex pane during development.

## Incremental conversation updates

The desktop now preserves measured rows across snapshots. Stable row IDs produce
bounded structural edits and targeted height invalidations. A state test with
2,000 saved messages plus one streaming row invalidates only the streaming row;
a status-only change invalidates none. This is a work-count regression test,
not an A11 result or a claim that the host exposes 2,000 messages yet.

The real GPUI headless test checks cached history measurements and scroll anchors
during 20 streaming updates, history clipping, and tool movement. Tool disclosure
follows its stable ID, not its current row number. See the
[verification and labeled Linux captures](VERIFICATION.md).

## Next delivery: a valid Mac baseline

1. **Pin the installed build.** Record client and active host build IDs,
   dependency versions, checksums, and the source commit. Provide a stable,
   public download before requesting inclusion. Current CI artifacts expire;
   they are not a stable release channel. See [Mac installation](MACOS.md),
   [release issue #6](https://github.com/heath0xFF/plait/issues/6), and
   [startup issue #14](https://github.com/heath0xFF/plait/issues/14).
2. **Prove replay and settings.** Confirm requests reach ADEMark's replay server
   and no live provider receives them. Do not assume Pi honors
   `ANTHROPIC_BASE_URL`. The installed host can predate an environment change;
   relaunching only the desktop is not enough. Pin provider, model, effort,
   and effective permissions. The current thread contract saves provider/model,
   not an explicit effort setting. Extend the contract with compatibility and
   persistence tests when adding that setting.
3. **Calibrate on a dedicated Mac account.** Follow the pinned upstream guide.
   Establish clean reset and private sign-in seeds, project opening, new thread,
   send, completion, quit/relaunch, and accurate process attribution. The stock
   Claude/Codex session-log completion readers are not proof of Pi completion.
   Obtain real coordinates and build hashes; do not invent a working profile.
4. **Validate the adapter, then measure.** Run upstream conformance checks and
   smoke runs before a real Track A run. Record unsupported tests and invalid
   rows with reasons. A partial baseline is not a complete Track A pass.
   Keep the normal production code path; no special benchmark permissions,
   database seeding, or hidden fixture responses in the app.
5. **Capture Track B separately.** Obtain approval for live usage. Inspect all
   model requests, including side calls, and check the repository-rule canary.
   Keep useful instructions and safety context; do not remove them for a lower
   token count. C1 is outside this first delivery.

Upstream requires a person to prepare the Mac and start real measurements from
an ordinary terminal, outside an agent session. Rex is suitable for development
builds and probes, but an agent-driven run is not a valid measured baseline.
Use upstream's 1600 × 1000 window, 60 Hz display, quiet-machine gates, and repeat
counts. Do not use Accessibility inspection during timed measurements.

## Engineering work for full coverage

### Concurrent load

[`runtime/src/runtime.ts`](../runtime/src/runtime.ts) rejects a second active
run in one checkout and permits at most eight active checkouts. Five saved
threads are not five simultaneous streams. Running five separate checkouts is
a useful internal probe, but not the specified same-workspace comparison unless
upstream accepts and records the difference.

Implement explicit read-only capabilities and admission rules before supporting
this load in one checkout. A prompt that says “read-only” is not a security
boundary. Keep exclusive writer leases and dirty-file protection.
Do not add a replay-only exception or silently enable concurrent writers.

### Long history

[`runtime/src/transcript.ts`](../runtime/src/transcript.ts) retains at most
100 messages and 50 tool records in a 512 KiB snapshot, with smaller text limits.
The runtime sets a 20-turn limit per run. ADEMark builds its long thread through
normal agent/tool calls; it does not inject a database fixture. These limits
need a history and run-budget design that represents the workload faithfully.
If a run cuts
history, report it, as upstream requires.

Prefer bounded history pages and incremental updates over one ever-growing
snapshot. Keep the client/host contract, storage behavior, run limits, and
recovery tests consistent through the change. Virtualization alone does not
make unavailable history accessible.

### Permissions and persistence

ADEMark asks for full access inside the worktree. Plait's current manual runs
require exact-call approvals; commands have a read-only checkout, private
scratch/cache writes, and no external network. These are not equivalent
settings. Disclose the difference and agree which tests are comparable.

[Automation grants (#13)](https://github.com/heath0xFF/plait/issues/13) remain
separate product work. They must not become blanket benchmark approval or
weaker command containment. Closing the client must still leave accepted host
work running. Count that intentional persistence in A9 rather than hiding it.

## Profile before optimizing

Source inspection identified these candidates, **not measured bottlenecks**:

- The original 150 ms sidecar poll has been replaced by event-driven delivery.
  Continue to measure the whole request path, including serialized sidecar reads
  under concurrent load. Removing a timer does not prove low total idle CPU.
- [`runtime/src/runtime.ts`](../runtime/src/runtime.ts) rebuilds the transcript
  projection and saves a snapshot on runtime watch updates.
  [`runtime/src/store.ts`](../runtime/src/store.ts) uses full SQLite durability.
  Measure allocation and write cost. Keep accepted input, approval, and file
  provenance durable; do not weaken commit guarantees for a speed result.
- The desktop no longer replaces the live list's full item range on every
  snapshot. Measure native layout/paint and typing while streaming to determine
  the end-to-end benefit of retained row measurements.

Separate OS launch, host readiness, replay/model delay, transport, view updates,
and final frame presentation. Use release builds and repeated before/after runs
on the same Mac. Internal timing probes are diagnostics, not ADEMark rows.
Keep transport and history changes modular, with bounded memory and explicit
recovery behavior. Do not add benchmark-only execution paths.

## Evidence and regression rules

- Keep the run manifest, raw repetitions, invalid reasons, versions, machine and
  display settings, process lists, and summary. Compare with a Bare CLI control
  measured on the same machine; do not compare Linux timings to the Mac chart.
- Report cold start and relaunch with a retained host separately. Count all
  attributed processes even when the desktop was not their parent.
- Keep private screenshots, request captures, and authentication seeds out of
  Git. Review even the upstream shareable rows, summary, and manifest for secrets.
- Do not merge measurements from different builds or runs. Publish medians and
  distributions, not the best repetition. Smoke and fixture runs do not count.
- Only reference-machine or audited runs may carry upstream's verified label.
  Do not use `--reference` on a development machine.
- Keep regression checks for stale async results, bounded output, input during
  streaming, Stop, reconnect, no replay, dirty paths, and checkout ownership.
  Add performance budgets after a stable baseline, not from guessed rankings.
- [Existing render observations](VERIFICATION.md#performance-observations) and
  Linux layout images remain development evidence, not native benchmark results.
