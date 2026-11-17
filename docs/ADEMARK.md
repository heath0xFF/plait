# ADEMark readiness and performance

**Plait has no published ADEMark result, adapter, or verified ranking.**
[Issue #15](https://github.com/heath0xFF/plait/issues/15) tracks baseline work.
Performance goals are in the [roadmap](ROADMAP.md#performance-goals); current
functional evidence is in [Verification](VERIFICATION.md).

## Measurement basis

The reviewed [ADEMark source](https://github.com/zqkra/ademark/tree/119a5f7b99247cffbe7f96bb2c36c03496cf3018)
is pinned to `119a5f7b99247cffbe7f96bb2c36c03496cf3018`. Use its
[methodology](https://github.com/zqkra/ademark/blob/119a5f7b99247cffbe7f96bb2c36c03496cf3018/METHODOLOGY.md),
[test procedures](https://github.com/zqkra/ademark/blob/119a5f7b99247cffbe7f96bb2c36c03496cf3018/TESTS.md),
[adapter guide](https://github.com/zqkra/ademark/blob/119a5f7b99247cffbe7f96bb2c36c03496cf3018/GUIDE.md),
and [fairness rules](https://github.com/zqkra/ademark/blob/119a5f7b99247cffbe7f96bb2c36c03496cf3018/FAIRNESS.md).
Check for upstream changes before starting a measured run.

V1 ranks tests separately, not with a composite quality score. Track A normally
uses a local replay server; Tracks B and C use live model requests. Plait runs
its own Pi Durable integration, not Claude Code. Any adapter must disclose that
and attribute the desktop, Rust host, Node sidecar, LaunchAgent, and child processes.

## Current gaps

| Area | Requirement |
| --- | --- |
| Launch and first response | Measure installed launch/input-to-pixel latency and prove replay routing |
| Idle, memory, and post-quit state | Count every attributed process, including the intentionally persistent host |
| Five-stream workloads | One active run per checkout is currently enforced; five saved threads are not five concurrent streams |
| Long conversations | The UI receives a bounded transcript tail, not 2,000 accessible history items |
| Tokens and repository rules | Capture real requests, model/effort settings, side calls, and instruction handling |
| Permissions | Exact-call approvals and offline, read-only command access are not equivalent to full-access benchmark settings |
| Orchestration | Not part of Plait; do not add swarms or mandatory worktrees for a ranking |

The runtime permits at most eight active checkouts and 20 turns per run. Separate
checkouts can support an internal load probe, but do not claim equivalence to a
same-workspace workload without upstream agreement. Read-only concurrency,
history paging, and explicit effort settings need contract and persistence review.

## Development probes

Event-driven host watches coalesce notifications instead of polling the sidecar.
The desktop cancels old subscriptions explicitly and retains unchanged transcript
row measurements. These reduce known work; they do not establish total idle CPU,
native frame rate, or typing latency.

Run the credential-free transport probe:

```sh
cargo build --locked --release -p plait-host
node runtime/bench/watch.mjs
```

It uses a scripted sidecar, not Pi, a live model, or the desktop. It reports
change-to-socket latency and sidecar request counts for one and 20 watchers, with
40 samples per watcher count. Twenty socket watchers are not ADEMark's 20-thread
workload. [Recorded development samples](performance/watch-2026-11-16.json) retain
the build hashes and environment; personal workstation/path metadata is removed.
They are not a controlled Mac benchmark.

Root-render diagnostics exclude layout, GPU work, and frame presentation. The
2,000-row state regression checks invalidation counts, not accessible host history
or end-to-end latency. Headless Linux renders are layout evidence only.

## Baseline procedure

1. Pin a [published build](https://github.com/heath0xFF/plait/releases), its client
   and active host IDs, checksums, dependencies, OS, and display settings.
2. Prove replay requests reach only the intended endpoint. Do not assume Pi
   honors another client's environment variables. Restarting the desktop does
   not replace the running host or its environment.
3. Calibrate reset, project opening, model selection, send, completion, and
   quit/relaunch on a dedicated Mac account. Use Pi-specific completion evidence;
   another harness's session-log reader is not sufficient.
4. Validate the adapter and run upstream conformance/smoke checks. Record invalid
   and unsupported tests. A partial baseline is not a complete Track A pass.
5. Run timed measurements under the pinned methodology, with its window size,
   display refresh, quiet-machine gates, repetitions, and matching CLI control.
6. Obtain explicit consent for live usage before Track B. Keep repository rules
   and safety context intact; do not remove them to reduce token counts.

The reviewed methodology requires a person to prepare the Mac and start timed
measurements from an ordinary terminal, outside an agent session. Agent-driven
smoke runs are not a valid measured baseline. Do not use Accessibility inspection
during timed measurements or label a development machine as a reference machine.

## Reporting and safety

- Keep the run manifest, raw repetitions, invalid reasons, versions, process
  lists, and summary. Report distributions, not the best repetition.
- Report cold start and relaunch with a retained host separately. Do not stop
  the host just to improve a post-quit memory number.
- Compare the same builds and conditions. Do not combine Linux transport
  observations with native Mac results or publish unsupported comparisons.
- Keep private prompts, screenshots, credentials, and authentication seeds out of
  Git. Review shareable outputs for sensitive data before publication.
- Profile launch, host readiness, replay/model delay, transport, durable writes,
  view updates, and presentation separately. Source inspection is not a measured
  bottleneck; durable commits must not be weakened for a faster result.
- Keep normal production code paths, exact approvals, writer leases, dirty-file
  protection, cancellation, and recovery. No benchmark-only execution permission,
  hidden fixture response, or database seeding in a measured app workload.
