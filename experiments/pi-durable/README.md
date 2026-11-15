# Pi Durable integration probe

This is an M2 validation tool, not Plait's host service. The desktop still uses
example conversations. Nothing here gives the desktop an active composer.

## Run

Use Node 22.19 or newer; checked with Node 26.12.0 on Zaphod. All Pi packages are
pinned to published version 1.5.2. Keep the lockfile.

```sh
cd experiments/pi-durable
npm ci --ignore-scripts
npm test
```

The default suite uses Pi's scripted model. It needs no credentials or network
model request. It still uses real Pi Durable tools, child processes, and SQLite
storage.

The live probe uses the existing Pi credential/model configuration through the
public `ModelRuntime` SDK. It can refresh stored OAuth credentials through Pi's
normal storage locks. It makes model requests on that account:

```sh
PI_PROVIDER=openai-codex PI_MODEL=gpt-6-astra npm run probe:live
```

Use a provider/model configured in your Pi installation. Credentials are not
printed, copied into Plait, or sent to the UI.

## Checks

Each run creates a private temporary directory with `notes.txt` and a SQLite
session. The only tools installed are `read` and `edit`. A hook limits both to
`notes.txt`; neither shell execution nor access to the Plait checkout is enabled.

Assertions check:

- An incremental model update arrives with the scripted provider.
- Tools change `hello world` to `hello durable` in the temporary file.
- Detaching an observer does not cancel the run.
- A new observer receives completed tool and assistant entries.
- Closing and reopening SQLite preserves the thread identity and transcript.
- Reusing the same request ID returns the original completed submission.

The result names the temporary directory. It is kept for inspection. The probe
closes the Harness and Pi's provider session resources before it exits.

### Process crash checks

`recovery.test.mjs` starts a runtime in a child process. After a tool writes its
file effect, but before it returns a result, the parent sends `SIGKILL`. A new
process then opens the same SQLite store.

Two tests check the replay policy:

- An `unsafe` tool is not run again. Its result records the interruption.
- A `safe` tool is run again and completes its idempotent file overwrite.

Both check that the thread and submission IDs survive, retrying the same request
does not add a user message, and the file contents remain correct. An attempt
log detects duplicate execution. Workers and temporary directories are removed
after each test, including a failure. The tests use no account credentials.

The live run on Zaphod also received an incremental update: 9 watch callbacks,
1 partial update before detach, and 7 final transcript entries. This is a small
integration check, not a performance benchmark.

## Integration findings

Pi Durable is viable for the next local slice, with these boundaries:

- Use the published APIs and exact versions. Upstream `main` can differ from a
  published package even when its manifest still shows the same version.
- `ModelRuntime` implements Pi AI's `Models` interface. It can supply the same
  configured provider, model, and authentication to Durable without a second
  credential implementation.
- Durable has its own extension registry. Regular Pi extensions, TUI controls,
  skill discovery, and `AGENTS.md` loading do not appear merely by using its
  Harness. This probe does not claim compatibility with those resources.
- The Rust host must own the sidecar's lifetime. A GPUI window or an event
  subscriber must not own it. Disconnect means stop watching, not abort work.
- Only one process may write a Durable store. The host needs an ownership lock.
  SQLite file locking alone is not a multi-writer Harness protocol.
- Use stable conversation IDs and request IDs. Reconnect from a current view,
  then page missing history. Do not replay an unbounded event log into the UI.
- On host shutdown, close the Harness, settle credential writes, and call
  `cleanupSessionResources()`. Closing the Harness alone left a provider socket
  open in the first live probe.

## Still required for M2

A separate Rust host, authenticated local transport, a TypeScript sidecar,
creation/resume controls, a native editable composer, and streamed typed UI
state. Add Plait host-process crash/restart, actual client reconnect, abort, and
failed-storage tests. Extend replay checks to the tools enabled in that host.
Load project instructions explicitly
before enabling coding against a real checkout.

The original probe stops an observer in one process and reopens storage cleanly.
The crash tests kill an isolated Pi runtime, not a Plait host. They do **not**
prove desktop reconnection, real-provider request recovery, power-loss safety,
or exactly-once effects for arbitrary tools. No regular Pi extensions, remote
execution, or orchestration are included.

Sources: [Pi Durable](https://github.com/earendil-works/pi/tree/main/packages/durable),
[Pi SDK](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sdk.md),
and the installed 1.5.2 package declarations and implementations.
