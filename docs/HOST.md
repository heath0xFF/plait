# Local host contract (M2)

This is the version 1 contract plus the opt-in version 2 additions described in
[Protocol version 2](#protocol-version-2). Changes require compatibility review.
All values below are JSON. The desktop has no dependency on the host crate and
does not own its process.

## Launch and recovery

Build `cargo build -p plait-host`; install runtime packages with
`cd runtime && npm ci --ignore-scripts`. Node 22.19 or newer is required for
source development. The [Mac host package](MACOS.md) includes pinned Node and
runtime dependencies; it does not need a development checkout or shell Node.

- `plait-host run [--store ABSOLUTE_PATH] [--runtime ABSOLUTE_PATH]`: foreground
  daemon. `--runtime` is the path to `runtime/src/main.ts`.
- `plait-host run-user [same options]`: installed per-user service entry. Resolve
  the home from the OS account, reject root, and set the sidecar home and working
  directory explicitly. Do not inherit an installer's home or store override.
- `plait-host start [same options]`: spawn an independent daemon and return when
  its socket answers, or report a startup error. Closing the client has no effect.
- `plait-host status [--store ABSOLUTE_PATH]`: print the status JSON reply.
- `plait-host version`: print local binary version/revision and protocol metadata.
  This is a CLI operation, not a new socket operation.
- Default store: `$PLAIT_HOST_STORE`, if set, otherwise `$HOME/.local/share/plait`.
  Socket: `STORE/host.sock`. Store overrides must be absolute.
- The store directory is mode 0700 and socket mode 0600. A lifetime OS writer
  lock on `STORE/host.lock` rejects a second host, including after a crash.
- The Rust host owns and monitors the Node sidecar. No TCP port is opened.
  Sidecar failure closes the host. Development restart is explicit through
  `start`; the installed Mac LaunchAgent restarts failed hosts. Neither route
  replays interrupted work. The packaged host uses adjacent immutable Node and
  runtime paths, not a build-machine source path. It removes inherited
  `NODE_OPTIONS` and `NODE_PATH` before starting its sidecar.
- Startup marks interrupted work as `interrupted`. It does not run the model
  or replay commands. A new user submission resumes the saved conversation.
  Thread IDs, submitted request IDs, and exact model choice survive restart.

## Transport

Owner-only Unix stream socket, UTF-8 newline-delimited JSON, protocol version 1
by default; version 2 is opt-in per request.
Clients and the daemon verify connected-peer user identity with `getpeereid` on
macOS or `SO_PEERCRED` on Linux. Clients also check socket and parent ownership
and private modes before connecting. This does not enable Linux commands.
One request and one reply per connection, then EOF, except `watch`. No greeting.
Request limit: 64 KiB including newline. Reply/event limit: 1 MiB including
newline. Absolute input deadline: 5 seconds; sidecar exchange deadline: 20 seconds. Slow watchers are disconnected rather
than accumulating an unbounded queue. Reconnect starts with the latest snapshot;
clients replace state, not append it. Unknown operations and malformed requests
return an error and close the connection. Unknown object fields are rejected.

Success: `{"ok":true,"result":VALUE}`.
Failure: `{"ok":false,"error":{"code":"invalid_request","message":"Safe human-readable detail"}}`.
Reply codes: `invalid_request`, `not_found`, `busy`, `conflict`, `model_unavailable`,
`permission_denied`, `internal`. Snapshot errors can also use `runtime_error` and
`interrupted`. Never send credentials, auth paths, or raw
provider error bodies. Errors are not accepted submissions.

## Operations

```json
{"op":"status"}
{"op":"set_default_model","provider":"anthropic","model_id":"MODEL"}
{"op":"list_threads","checkout":"/absolute/checkout"}
{"op":"create_thread","checkout":"/absolute/checkout","provider":"anthropic","model_id":"MODEL","request_id":"CLIENT_UUID"}
{"op":"submit","thread_id":"HOST_UUID","request_id":"CLIENT_UUID","text":"User message"}
{"op":"stop","thread_id":"HOST_UUID"}
{"op":"approve","thread_id":"HOST_UUID","approval_id":"HOST_UUID","allow":true}
{"op":"watch","thread_id":"HOST_UUID"}
```

`status` result:
```json
{"protocol_version":1,"default_model":null,"models":[{"provider":"anthropic","model_id":"MODEL","name":"Display name","authenticated":true}]}
```

`default_model` is null or `{"provider":"anthropic","model_id":"MODEL"}`.
Catalog and auth status come from Pi ModelRuntime; no credentials cross the socket.
`set_default_model` result: `{"default_model":{"provider":"anthropic","model_id":"MODEL"}}`.
`list_threads` result: `{"threads":[{"id":"HOST_UUID","checkout":"/canonical/checkout","model":{"provider":"anthropic","model_id":"MODEL"},"status":"idle"}]}` (latest 100 summaries).
`create_thread` result: `{"thread":SNAPSHOT}`. It never starts model execution.
`submit` result: `{"thread_id":"HOST_UUID","request_id":"CLIENT_UUID","accepted":true}`.
`stop` and `approve` result: `{"thread":SNAPSHOT}`. Stop revokes permission and
signals cancellation immediately; the checkout lease remains held until cleanup.
A new submission can return `busy` while cleanup finishes, including an already
approved atomic edit. `idle` is published only after the checkout lease is free. Repeating an approval
after it has been used returns `not_found`; reconnect to inspect the current state.

Request IDs are nonempty ASCII identifiers (letters, digits, hyphen, underscore),
maximum 128 bytes. They are permanent deduplication keys. Repeating an ID with
identical input returns the original acceptance; changed input returns `conflict`.
A client must retain an uncertain submission's ID, including across reconnect.
Never retry an uncertain submission with a fresh ID. At most one active writing
run is permitted for a canonical checkout, including across different host stores.
A host admits at most eight active checkouts. Empty submissions are rejected.

`watch` first returns `{"ok":true,"result":{"thread":SNAPSHOT}}`, then zero or
more `{"event":"snapshot","thread":SNAPSHOT}` lines. The connection stays open
until disconnect, failure, or host shutdown. A disconnect does not stop work.

Delivery is event-driven. The runtime signals changed thread IDs through a
private inherited socket (`PLAIT_NOTIFY_FD`); the host fetches state only after
an invalidation. This internal channel is separate from request/reply stdio and
is not a client API. The host and runtime must be updated together, as packaged.
No snapshot or credential travels in a notification. Repeated changes coalesce;
a full ID set becomes an all-watch invalidation instead of an unbounded queue.
Each client has one pending wakeup, registers before its initial snapshot read,
and releases its subscription on disconnect. EOF or malformed notification
frames stop the host rather than leave clients silently stale. The public watch
frames, request deadlines, durable writes, and recovery rules are unchanged.

The desktop client waits for snapshots without a periodic read timeout. Its
initial watch reply has a 15-second deadline; after that, a partial frame must
finish within 15 seconds of its first byte. An idle, complete connection can
wait indefinitely. Explicit cancellation closes only that subscription on
selection, reconnect, or window release. A dedicated reader and one-slot async
inbox keep blocked sockets and snapshot backlogs out of GPUI's worker pool and
UI loop. Cancellation never submits a Stop operation.

## Snapshot

All keys below are always present. Nullable fields use JSON null.

```json
{"id":"HOST_UUID","checkout":"/canonical/checkout","model":{"provider":"anthropic","model_id":"MODEL"},"status":"idle","messages":[{"id":"STABLE_ID","role":"user","text":"Hello"}],"live_text":"","tools":[{"id":"STABLE_ID","name":"read","status":"done","input":"JSON argument preview","output":"Text output preview"}],"pending_approval":null,"error":null,"truncated":false}
```

Statuses: `idle`, `running`, `awaiting_approval`, `stopped`, `interrupted`, `error`.
Message roles: `user`, `assistant`. Tool statuses: `running`, `done`, `error`,
`interrupted`. `error` is null or `{code,message}` with the same safe error shape.
`pending_approval` is null or
`{"id":"HOST_UUID","tool_id":"STABLE_ID","kind":"edit","summary":"Change src/main.rs","details":"Full exact JSON tool arguments","dirty_paths":["notes.txt"],"risk":"Pre-existing dirty paths are protected."}`.
`details` is never clipped: tool arguments over 16384 bytes are rejected.
Approval kind is `edit` or `command`. Approval is bound to one tool call; it is
not blanket permission. Stop and restart revoke pending approvals.
Messages: latest 100, 4096 characters each. Tools: latest 50, 4096 characters
per input/output. Live text: latest 16384 characters. These text budgets count
UTF-16 code units; clipping keeps whole Unicode code points and does not increase
the budget. Command stdout and stderr each decode UTF-8 across pipe chunks.
The original accepted input remains in the private journal. The complete serialized
snapshot is also limited to 512 KiB; older content is removed first. Approval
arguments are never clipped. `truncated` flags clipping.

## Protocol version 2

Version 2 is negotiated per request, so desktops and hosts can update separately.
Every operation accepts an optional `"protocol_version":1` or `2`; omitting it
means version 1, and version 1 replies are byte-for-byte unchanged.

- **Negotiation.** A client sends `{"op":"status","protocol_version":2}`. A
  version 2 host replies with `"protocol_version":2`. A version 1 host rejects the
  unknown field with `invalid_request`; the client then repeats plain `status` and
  uses version 1. Clients send version 2 fields only to a host that negotiated them.
- **Snapshots.** Requests with `"protocol_version":2` receive snapshots with two
  extra keys. Each tool has `after`: the ID of the message it followed, or null
  for a tool before any message; tools saved before version 2 omit it and are shown
  in the old order. Clients show tools after that message. A tool
  whose message was clipped from the preview is shown first. The snapshot has
  `approval_mode`: `"ask"` or `"auto"`, the mode of the latest accepted run.
- **Submit.** `submit` accepts `"approval":"ask"` or `"approval":"auto"`, only with
  `"protocol_version":2`. Omitted means `ask`. The mode applies to that run only;
  the next submission chooses again. Request-ID deduplication still compares only
  the text, so a retry returns the original acceptance and mode.

Version 1 clients that watch a thread started in `auto` mode still see version 1
snapshots, without the mode. Downgrading a host after version 2 snapshots were
saved is not supported.

## Internal authorization boundary

Each accepted run gets a new in-memory authorization object. Its policy is either
exact-call interactive approval (`ask`) or automatic approval (`auto`), chosen by
that run's submission. A thread ID, an earlier approval, or a recovered snapshot
does not grant permission to another run. Recovery keeps authorization inactive;
Stop and shutdown revoke it. Answers after cancellation or consumption return
`not_found`, as before.

Authorization and tool execution are separate. Argument limits, dirty-path and
ignored-path protection, checked file helpers, and the command sandbox still
apply in both modes. Automatic approval removes the prompt, not those checks.
The sandbox is the default [command policy](#command-policy); approval mode
never changes which policy a run uses.
There is no scheduler or persisted grant in this contract. Future automation
grants require separate protocol, persistence, and permission review.

## Command policy

Each canonical checkout has a command policy: `sandboxed`, the default, or
`trusted`. It decides how the `command` tool runs for runs in that checkout.
Trusted mode is opt-in per project. The host operations below are implemented;
**the desktop control that sets the policy is not shipped yet**, so the app
uses the sandbox unless a client sends the operation.

```json
{"op":"command_policy","checkout":"/absolute/checkout"}
{"op":"set_command_policy","checkout":"/absolute/checkout","policy":"trusted"}
```

Both return `{"checkout":"/canonical/checkout","policy":"sandboxed"}`, with the
policy now in force for that checkout. `checkout` is an absolute path inside a
Git working tree and is resolved to its canonical root, as in `list_threads`.
`policy` is `sandboxed` or `trusted`; anything else is `invalid_request`. Both
operations accept the optional `protocol_version` and reply the same way for
every version. A host without them rejects the unknown operation with
`invalid_request`; a client treats that host as sandboxed. No existing request
or reply changes: `create_thread`, `submit`, and snapshots carry no policy. A
snapshot does not yet report the policy its run used.

- **Storage.** `host.sqlite` keeps one `settings` row per trusted checkout, keyed
  by the canonical path and holding the checkout root's device and inode. There
  is no schema change. `sandboxed` removes the row. A missing or unreadable
  row, or a directory that is no longer the one that was trusted, is
  `sandboxed`. Another host store never shares the setting.
- **Who can set it.** Only this operation, sent by a local client as an explicit
  user action. No tool, model output, thread, run, or snapshot carries or
  changes the policy, and a thread cannot take it to another checkout.
- **When it applies.** A run reads the policy once at admission and keeps it
  until it ends. A change applies to the next run. Recovery starts no run.
- **Linux.** Commands are rejected in both modes.

| | `sandboxed` | `trusted` |
| --- | --- | --- |
| Process | Seatbelt profile, `/bin/sh -c` | `/bin/sh -c` as the user, no profile |
| Working directory | Checkout | Checkout |
| Checkout and Git writes | Denied | Allowed |
| Other files | Read system and toolchain paths; write private scratch and the thread build cache | Anything the user account can read or write |
| Network | Denied, except Unix sockets in scratch | Allowed |
| HOME and TMPDIR | A new private directory per command | The user's real home and temporary directory |
| Environment | Fixed PATH and Apple toolchain | The user's login-shell environment |
| Build output | Per-thread cache; Cargo offline | Wherever the project's tools put it |
| Standard input | None | None |
| Deadline, Stop, output bounds | As in [Safety boundary](#safety-boundary) | The same |

**Trusted environment.** The host runs as a per-user service with a minimal
PATH, so it asks the account's login shell, as a new terminal would. Once per
run, before the first trusted command, it starts the account's shell (from the
OS account record) as a login shell in the host's HOME and reads the resulting
environment. The shell is also interactive for zsh, bash, and fish, whose
interactive startup files commonly extend PATH. Startup files are the user's
own and run with the user's access. That shell starts from a fixed base: `HOME`, `USER`, `LOGNAME`,
`SHELL`, the system PATH, and, when the host has them, `TMPDIR`, `LANG`,
`LC_ALL`, `LC_CTYPE`, `SSH_AUTH_SOCK`, and `__CF_USER_TEXT_ENCODING`. Nothing
else from the host process is inherited. `PLAIT_HOST_EXE`, `PLAIT_LOCK_FD`, and
`PLAIT_NOTIFY_FD` are removed, and the guardian marks every inherited descriptor
close-on-exec, so the store lock, the checkout lease, and the notification
socket never reach the command. The read has a 10-second limit and runs under
the guardian. If it fails, the command runs with the base variables and
`HOME/.cargo/bin`, Homebrew, `/usr/local/bin`, and the system PATH, and its
output begins with a notice. Shell functions and aliases are not available:
commands run under `/bin/sh`.

**Unchanged in trusted mode.**

- Authorization. `ask` still prompts for each exact call and `auto` still
  removes only the prompt. Authorization is checked again before the command
  starts, so an approval after Stop does nothing.
- Lifecycle. The Rust guardian holds the store and checkout writer leases and
  kills the command's process group on Stop, host or sidecar death, the
  15-minute deadline, and shell exit. Output keeps its bounds and Unicode rules.
- File tools. `read`, `edit`, and `write` keep every check in
  [Safety boundary](#safety-boundary). Paths that were dirty or ignored before
  the run stay refused for the whole run, even if a command commits them.
- Approval text. A command prompt reads `Run command with full user access and
  network`, and the risk text of every prompt in a trusted run says that
  commands run as the user, outside the sandbox, with network access.
- Model instructions. The run's instructions and the `command` tool description
  describe the mode in force. The sandboxed wording is unchanged.

**Around each trusted command**, whether it succeeds, fails, or is stopped:

- Before it starts, the host compares each file this thread owns with its
  postimage record: identity, mode, size, and times. A file that already
  differs was changed by something else. Its record is dropped, in memory and
  in the store, so the command cannot bring that change into the thread's
  ownership.
- After it ends, a file the thread still owned at the start and that changed
  during the command gets a fresh record of its new state, read through the
  checked file helper and committed to the store. The thread keeps ownership,
  so edit, format or generate, then edit again works. The host cannot tell the
  command's change from another program's change made while the command ran;
  both are refreshed.
- A file that is gone, ignored, or no longer one the file helper accepts loses
  its record instead.
- Nothing else becomes owned. A file the command creates, or changes without
  the thread having owned it, is an ordinary dirty path: file tools refuse it,
  in this run and later ones, until it is clean again, for example after a
  commit. Paths that were dirty before the run stay refused whatever the
  command does.
- A later edit still compares the whole record, including the content hash. If
  the host stops between the command and the refresh, the old record remains
  and never matches, as before.
- If a path that was dirty before the run changed during the command, the
  command's output begins with a notice naming up to 20 such files.
  Directories are not examined. This reports the change; it does not prevent
  or undo it.

**What trusted mode gives up.** A trusted command is a program running as you.

- It can change, delete, or commit any file your account can, including files
  that were dirty before the run, ignored files, Git history, other checkouts,
  and your home directory. Dirty-path protection covers only the file tools.
- It can use the network, your credentials, your SSH agent, and your keychain
  as any program you run can, and can send what it reads anywhere.
- It can reach the host itself: the socket, the store, and the installed host
  files. The host does not offer the model a way to change the policy, approve
  a call, or reach another thread, but it cannot stop a trusted command from
  doing those things as you. Trust the project and the model accordingly.
- It can signal other processes, and a process that deliberately leaves the
  command's process group survives Stop with the same access.
- The checkout writer lease still excludes other Plait runs, not the command's
  own writes to other places.
- With automatic approval, trusted commands run without a prompt.
- Build output and dependency installs land in the checkout or your home, not
  in a private cache. An ignored directory such as `node_modules` or `target`
  is one entry of the 500-entry dirty baseline however many files it holds,
  whether it existed before the run or a command created it. Untracked files,
  and files ignored one at a time by a file pattern such as `*.o`, each count:
  a command that leaves more than 500 of them makes file tools and later runs
  fail until they are removed, committed, or ignored by directory.

## Safety boundary

Command rules in this section describe the default `sandboxed` policy. A
project set to `trusted` replaces them as described in
[Command policy](#command-policy). File-tool rules apply to both.

Load checkout `AGENTS.md` explicitly before execution. Model and thread settings
are immutable once a thread exists; changing the default affects only new threads.
Edits and commands need explicit approval unless the run was submitted with
automatic approval. Either way, protect pre-existing dirty paths,
including untracked and ignored paths. File descriptors are checked against
saved pre-run device/inode identities; case and Unicode aliases cannot bypass
these checks. New outside changes are checked again before each edit.

A private per-thread postimage record permits follow-up edits to this thread's
own unchanged files. The Rust helper returns the canonical descriptor path,
device/inode, SHA-256 of the exact bytes written, size, mode, mtime, and ctime
after the atomic replacement. It does not include atime. The host commits this
record with SQLite FULL durability before tool completion. A later run compares
the current descriptor fingerprint with that exact record. Only a full match
from the same thread can be removed from the protected dirty set. Ignored paths
and parents, external changes, another thread's files, root replacements, and
missing records remain protected. A crash after an effect but before its record
never grants ownership. No ownership is inferred from an unrelated later read. Never reset, stash, or overwrite these paths. Reject
symlinks, parent traversal, special files, Git metadata, and filesystem escapes.
Command execution uses the macOS filesystem sandbox; other hosts reject commands.
Commands READ the checkout and WRITE ONLY private scratch and the current
thread's private build cache. File edit/write tools are the sole checkout
writers. HOME and TMPDIR use a new mode-0700 directory under `/private/tmp` for each
command. The short path leaves room for native Unix socket names.
CARGO_HOME and CARGO_TARGET_DIR stay in `STORE/build-cache/THREAD_ID/{cargo,target}`
so later commands reuse compiled dependencies. Cache directories must be
canonical, owner-only directories with no symlink traversal; the sandbox cannot
rename or change the mode/owner of their roots. No other thread cache, host store,
credentials, or general home access is granted. The host resolves store aliases
such as `/tmp` once, after it rejects a symlink store leaf.

Installed Rust toolchains, shared Cargo registry/Git source caches, and Xcode
remain read-only. No user Cargo configuration or Pi authentication is copied.
Fixed native `xcrun --find` queries resolve installed Apple Metal tools before
execution; a scratch-local adapter handles only the pinned GPUI Metal calls.
Other forms run stock xcrun inside the sandbox. No license is changed and no
Mach service is granted. Git uses the installed Xcode binary, not its system
license-discovery stub. Cargo is offline. Builds must use an existing lockfile
(`cargo test --offline --locked`); use an approved file tool to create a missing
lockfile. Tests that require checkout writes fail. Dirty sources can be tested
safely, including this run's approved edits.

Tests may create Git repositories and Unix sockets only in disposable scratch.
Anonymous socket pairs also work. TCP, outside Unix sockets, real checkout/Git
writes, source-cache writes, and unlisted Mach services remain denied. Commands
can signal only processes in the same sandbox, not the host or outside processes.
Each command has a 15-minute deadline; Stop remains available during a build.
Caches have no automatic eviction or size quota. To discard one, stop the host
and remove only that thread's `STORE/build-cache/THREAD_ID` directory.

Commands must not daemonize or start background services. A Rust guardian kills
the normal shell process group on Stop, host/Node death, timeout, or shell exit.
An intentionally detached descendant can evade group cleanup, but still cannot
write the checkout or outside that command's scratch and thread cache. This is
not a general process container.
The guardian holds both store and canonical-checkout writer leases until exit.
The checkout lease is an OS flock on the canonical root directory inode itself,
not a pathname under TMPDIR; different stores and environments share the lease.
File edits use a synced temporary file and atomic rename, with original mode and
identity/content checks before replacement. The original is opened read-only:
a write-capable open can change macOS provenance metadata before any write. Root replacement is rejected by its
saved device/inode identity. Concurrent hostile filesystem mutation by another
same-user process is outside this local trust boundary. Stop kills
command process groups and aborts the model. No interrupted effect is replayed. Acceptance is a durable journal commit before
execution: a crash in the admission/start gap leaves `interrupted`, even if the
input never reached Pi Durable. The same acceptance transaction saves the full
original input in a private journal and its bounded user message in the snapshot.
Request-derived message IDs remain stable when Pi later places the input. Failed
preflight and interrupted admissions remain visible in conversation order; they
are not silently inserted into the model context. Retrying the same ID confirms
acceptance but does not run it; inspect the snapshot and submit a new explicit
request.

Read tools and approved commands can read ignored files inside the chosen
checkout. Their contents can be sent to the provider. Keep secrets outside the
checkout, or do not give the agent tasks that require those files. This host does
not provide a secret-file filter. New files use private mode 0600.

Tests must use disposable repositories and a scripted provider before any live
model integration. Scripted data is test-only and never appears as real catalog
or transcript data in the production host.

## Verification

Use `bash scripts/check.sh` for the shared macOS/Linux development checks.
`bash scripts/check.sh --require-command-sandbox` requires full command-sandbox
coverage and currently fails on Linux. Runtime `npm test` labels its platform
coverage; Linux runs common daemon/file tests and explicit execution-denial tests.
See [Verification](VERIFICATION.md) for versioned results and
[Linux containment](LINUX-SANDBOX.md) for the open backend decision. A Linux
development pass is not full host-execution acceptance.

Use [Contributing](../CONTRIBUTING.md#required-checks) for focused commands and
[manual release acceptance](VERIFICATION.md#manual-release-acceptance) for native
and live-model checks. Do not substitute scripted lifecycle fixtures for the
actual platform containment tests.
