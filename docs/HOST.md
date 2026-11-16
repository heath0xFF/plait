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
- The Rust host owns and monitors the Node sidecar. No TCP port is opened,
  except the loopback callback of a pending [provider sign-in](#provider-sign-in).
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
Catalog and auth status come from Pi ModelRuntime. No reply carries a credential;
[Provider sign-in](#provider-sign-in) names the requests that carry one to the host.
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
There is no scheduler or persisted grant in this contract. Future automation
grants require separate protocol, persistence, and permission review.

## Provider sign-in

These operations change which providers the host can use while it runs. The
desktop Settings UI that calls them is not shipped yet. They are additions, not
a protocol version: each accepts the optional `protocol_version` and replies the
same for every version. A host without them rejects the operation name with
`invalid_request`.

```json
{"op":"reload_providers"}
{"op":"list_providers"}
{"op":"start_sign_in","provider":"anthropic","method":"browser"}
{"op":"sign_in_status","sign_in_id":"HOST_UUID"}
{"op":"answer_sign_in","sign_in_id":"HOST_UUID","input":"Pasted code or choice ID"}
{"op":"cancel_sign_in","sign_in_id":"HOST_UUID"}
{"op":"set_provider_key","provider":"openrouter","key":"SECRET"}
{"op":"sign_out_provider","provider":"openrouter"}
{"op":"add_endpoint","id":"home","name":"Home server","base_url":"http://127.0.0.1:8080/v1","model_ids":["MODEL"],"key":"SECRET"}
{"op":"update_endpoint","id":"home","name":"Home server","base_url":"http://127.0.0.1:8080/v1","model_ids":["MODEL"],"key":"SECRET"}
{"op":"remove_endpoint","id":"home"}
{"op":"check_endpoint","base_url":"http://127.0.0.1:8080/v1","key":"SECRET"}
{"op":"check_endpoint","id":"home"}
```

`method` and every `key` are optional. `update_endpoint` needs only `id`; a field
left out keeps its saved value. String values are trimmed and must not contain
control characters. Provider and endpoint IDs are at most 128 bytes; `key` and
`input` are at most 8192 bytes.

### What crosses the socket

Credentials stay in Pi's own files in Pi's agent directory (`~/.pi/agent`, or
`PI_CODING_AGENT_DIR`): `auth.json` for sign-ins and keys, `models.json` for
custom endpoints. The host store holds none of them.

- **Desktop to host, once each:** an API key in `key`, and the text a user pastes
  to finish a sign-in in `input`. Each travels in one request on the owner-only
  socket. The host passes it to Pi and keeps no other copy. The `key` of
  `check_endpoint` with `base_url` is used for that one check and not stored.
- **Host to desktop, never:** a key, token, or any other stored credential; a
  credential file path; a provider response body; the pasted `input`. No
  operation reads a stored credential back.
- **Host to desktop, for the user to act on:** the sign-in `url` and, in a device
  flow, the `user_code`. These are one-time values of a pending sign-in, not
  credentials, but the address can hold the flow's state. Open it; do not log or
  save it. The host drops both when the sign-in ends.
- Notifications still carry thread IDs only.

### Reload

`reload_providers` result: `{"providers":[PROVIDER]}`. The host reads `auth.json`,
`models.json`, and Pi's cached catalogs again, with no network request. It builds
a complete registry first, then replaces the model catalog and the authenticated
set of `status` in one step. If loading fails or exceeds 5 seconds, the reply is
`internal` and the previous registry stays in use.

A run in progress keeps the registry it started with, including a provider or
endpoint that the reload removed. Credentials are read from Pi's store for each
model request, so signing out of a provider fails a run that needs it at its
next model request.
Every operation below that changes a credential or an endpoint reloads before it
replies; the next `status` already shows the change.

### Provider list

`list_providers` result: `{"providers":[PROVIDER]}`, one entry per Pi provider.
All keys are always present.

```json
{"id":"anthropic","name":"Anthropic","kind":"builtin","authenticated":false,"credential":"none","model_count":18,"sign_in":{"name":"Anthropic (Claude Pro/Max)","subscription":true},"api_key":{"name":"Anthropic API key"},"endpoint":null}
{"id":"home","name":"Home server","kind":"custom","authenticated":true,"credential":"external","model_count":1,"sign_in":null,"api_key":{"name":"API key"},"endpoint":{"base_url":"http://127.0.0.1:8080/v1","api":"openai-completions","model_ids":["MODEL"]}}
```

- `kind`: `builtin` ships with Pi; `custom` is declared in `models.json`.
- `authenticated` matches the `authenticated` flag of that provider's models in `status`.
- `credential`: `sign_in` or `api_key` is stored in `auth.json` and can be removed
  with `sign_out_provider`. `external` means the provider works through an
  environment variable, a key in `models.json`, or ambient cloud credentials,
  which the host does not manage. `none` means not authenticated.
- `sign_in` is null or names the sign-in `start_sign_in` runs; `subscription` tells
  a plan such as Claude Pro or ChatGPT Plus from an ordinary account sign-in.
- `api_key` is null when `set_provider_key` cannot apply. A provider can offer both
  methods, one, or neither.
- `endpoint` is null for built-in providers. `base_url` is shown without any user
  name, password, query, or fragment, and is null if it cannot be parsed. `api`
  is the API of the first model.

### Subscription sign-in

`start_sign_in`, `sign_in_status`, `answer_sign_in`, and `cancel_sign_in` all
return `{"sign_in":SIGN_IN}`. All keys are always present.

```json
{"id":"HOST_UUID","provider":"anthropic","state":"waiting","url":"https://provider.example/authorize?...","user_code":null,"input":{"kind":"code","message":"Complete login in your browser, or paste the authorization code / redirect URL here:","placeholder":"http://localhost:53692/callback","required":false,"choices":[]},"message":"Complete login in your browser. If the browser is on another machine, paste the final redirect URL here.","expires_in_seconds":598,"error":null}
```

The host runs Pi's sign-in flow for the provider and stores the result where Pi
stores it. The desktop shows what the flow asks for and relays answers:

- `state`: `starting` (nothing to show yet), `waiting` (the user must act),
  `finishing` (the host is exchanging the result), then one of `signed_in`,
  `failed`, `cancelled`, or `expired`. Those four are final.
- `url`: open it in the user's browser. It is always `http` or `https`.
  `user_code`: show it; the user types it on that page (device flows).
- `input` is null or one question. `kind` is `code` (an authorization code or the
  final redirect URL), `choice` (answer with one `choices[].id`), `text`, or
  `secret`. `required` is false only for a `code` when the address returns the
  browser to this machine: the sign-in then normally completes by itself, and
  the field is a fallback if the redirect cannot arrive. Keep showing it.
- `message` is Pi's current instruction or progress text, or null.
- `error` is null unless `failed`; it is `{code,message}` with fixed host text.
- `signed_in` is reported after the reload, so `status` already agrees.

Pi's flows for Anthropic and OpenAI Codex first ask for a method. If `method`
names one of the offered choices, the host answers that first question; otherwise
it is passed on as a `choice`. A browser method listens on a loopback TCP port
(Pi's callback server on `127.0.0.1`, on a port Pi chooses) until the sign-in
ends. Pi's flow contacts the provider to exchange the result; the desktop does not.

Limits: four sign-ins in progress; one per provider, where a new start cancels the
earlier one; expiry after 10 minutes, or sooner when a device code expires
sooner. Final states stay readable for 5 minutes, at most 16 records. Nothing is
saved: a host restart forgets every sign-in, and host shutdown cancels those in
progress and closes their listeners. `start_sign_in` and `answer_sign_in` wait up
to 3 seconds for the flow to show something or finish, then reply with the state
so far. Read `sign_in_status` about once a second while `starting`, `waiting`, or
`finishing`. Other requests queue behind a waiting reply.

### API keys and sign-out

`set_provider_key` and `sign_out_provider` return `{"provider":PROVIDER}`.
`set_provider_key` stores or replaces the key through Pi's own key setup. Pi keeps
one credential per provider, so a key replaces a subscription sign-in and a
sign-in replaces a key. `auth.json` is created with mode 0600, and the host
restores 0600 after a write if group or other access was present. The key is
stored as literal text: Pi's `!command` and `$NAME` forms are escaped, so a
request cannot make the host run a command or read its environment. A provider
whose key setup asks for more than one secret value (for example Amazon Bedrock)
returns `invalid_request`; configure it in Pi.

`sign_out_provider` removes the stored credential and is not an error when none
exists. It cannot remove an `external` credential. Both operations first cancel a
pending sign-in for the same provider.

### Custom endpoints

`add_endpoint` and `update_endpoint` return `{"provider":PROVIDER}`;
`remove_endpoint` returns `{"removed":"home"}`. They edit one provider entry in
Pi's `models.json`:

```json
{"providers":{"home":{"name":"Home server","baseUrl":"http://127.0.0.1:8080/v1","api":"openai-completions","apiKey":"none","models":[{"id":"MODEL"}]}}}
```

- New entries use the `openai-completions` API. `apiKey` `"none"` is Pi's
  documented placeholder for servers without a key; it makes the endpoint usable
  and is not a secret. A real `key` goes to `auth.json` like any API key and takes
  precedence. `sign_out_provider` removes it again.
- `id`: 1 to 64 lowercase letters, digits, dots, hyphens, or underscores, starting
  with a letter or digit, and not a built-in provider ID. `name`: at most 128 bytes.
  `base_url`: an `http` or `https` URL of at most 2048 bytes with no user name,
  password, query, or fragment. `model_ids`: 1 to 200 distinct IDs of at most 256
  characters.
- Every other entry and every field the host does not set is written back as
  read, including a hand-written `apiKey`, `headers`, `compat`, and per-model
  fields of models that stay. Indentation and file mode are kept; a new file is
  0600. The write is a synced temporary file and one rename beside the real
  file, so a symlinked `models.json` stays a symlink.
- The host refuses with `conflict`, and writes nothing, when `models.json` is
  not plain JSON (Pi also accepts comments, which a rewrite would delete), has
  an unexpected layout, or changed on disk during the edit. After writing, the
  host reloads and checks that Pi reports the endpoint. Pi ignores the whole
  file when any entry breaks its schema; in that case the host restores the
  previous file and returns `conflict`.
- Built-in providers cannot be edited or removed here. `remove_endpoint` deletes
  the stored key first, then the entry.
- With `key`, `add_endpoint` and `update_endpoint` save the entry first. If
  storing the key then fails, the entry remains and the reply is an error.

`check_endpoint` sends one `GET BASE_URL/models` and returns:

```json
{"reachable":true,"reason":null,"status":200,"model_ids":["MODEL"],"truncated":false}
{"reachable":false,"reason":"unauthorized","status":401,"model_ids":[],"truncated":false}
```

With `base_url` it uses that address and the optional `key`. With `id` it checks
a saved custom endpoint with its saved key and headers. `reason` is `timeout`,
`unreachable`, `unauthorized` (401 or 403), `not_found` (404), `http_error`, or
`invalid_response`; `status` is null without an HTTP response. Limits: 5 seconds,
a 1 MiB response, the first 500 model IDs (`truncated` flags more). Redirects are
reported as `http_error`, not followed. The request goes only to the address
named, and only when this operation is sent. Other requests queue until it
replies.

### Errors

The reply codes are the existing ones. `invalid_request`: malformed input, an
unknown field, a provider that lacks the requested method, or a built-in provider
given as an endpoint. `not_found`: unknown provider, endpoint, or sign-in ID.
`busy`: four sign-ins already in progress. `conflict`: an existing endpoint or
provider ID, an `answer_sign_in` when no input is pending, or a `models.json` the
host will not rewrite. `internal`: Pi could not store, remove, or reload. Messages
name the field at fault and never repeat its value.

## Safety boundary

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
