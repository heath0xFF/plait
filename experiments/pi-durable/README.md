# Pi Durable integration probe

This isolated probe checks the pinned Pi Durable APIs. It is not Plait's host
service or command sandbox. Production host behavior is tested separately in
[`runtime/test`](../../runtime/test); see [Verification](../../docs/VERIFICATION.md).

## Run

Use Node 22.19+ and the lockfile. Pi packages are pinned to **1.5.2**.

```sh
cd experiments/pi-durable
npm ci --ignore-scripts
npm test
```

The default suite uses a scripted model, real tools, child processes, and SQLite.
It needs no account credentials or network model request.

An optional live probe uses your existing Pi configuration through the public
`ModelRuntime` SDK and makes requests on that account:

```sh
PI_PROVIDER=YOUR_PROVIDER PI_MODEL=YOUR_MODEL npm run probe:live
```

Choose an authorized provider/model configured in Pi. The SDK can refresh OAuth
credentials through its normal storage locks. Credentials are not printed or
sent to the UI. Live checks are not required to contribute.

## What it checks

Each run creates a private temporary directory containing `notes.txt` and a
SQLite session. Only `read` and `edit` are installed; a hook limits both tools
to that file. Shell execution and access to the Plait checkout are not enabled.

Assertions cover incremental scripted-model updates, a real file edit, observer
detach/reattach without cancellation, saved thread/transcript identity after
SQLite reopen, and deduplication of a repeated request ID.

The result names a temporary directory kept for inspection. The probe closes
the Harness and provider session resources before exit.

## Process-crash checks

`recovery.test.mjs` kills a child runtime after a tool writes its effect but
before it returns. A new process then opens the same SQLite store:

- An `unsafe` tool is not run again; its result records the interruption.
- A `safe` tool runs again and completes its idempotent overwrite.

Both cases check stable thread/submission IDs, no duplicate user message, and
correct file contents. An attempt log detects repeated execution. Workers and
fixtures are removed after each test. No credentials are used.

## Integration boundaries

- Use the published version's APIs. Upstream `main` can differ from a release.
- `ModelRuntime` supplies the model/authentication implementation; do not build a
  second credential store around it.
- Durable's registry does not automatically provide regular Pi extensions, TUI
  controls, skill discovery, or project-instruction loading.
- The independent host owns sidecar lifetime and a store writer lock. SQLite
  locking alone is not a multi-writer Harness protocol.
- Stable request IDs support safe retries. Reconnection observes saved state;
  it must not create a new submission automatically.
- Shutdown must close the Harness, settle credential writes, and call
  `cleanupSessionResources()` so provider resources do not keep the process alive.

These probes do not establish native desktop recovery, live-provider request
recovery, power-loss safety, or exactly-once effects for arbitrary tools. Keep
them separate from the production host's approval and containment tests.

References: [Pi Durable](https://github.com/earendil-works/pi/tree/main/packages/durable),
[Pi SDK](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sdk.md),
and the installed 1.5.2 declarations and implementation.
