# Local host contract (M2.5)

This is the version 1 contract plus the opt-in additions described in
[Protocol version 2](#protocol-version-2) and
[Protocol version 3](#protocol-version-3). Changes require compatibility review.
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
by default; versions 2 and 3 are opt-in per request.
Clients and the daemon verify connected-peer user identity with `getpeereid` on
macOS or `SO_PEERCRED` on Linux. Clients also check socket and parent ownership
and private modes before connecting. This does not enable Linux commands.
One request and one reply per connection, then EOF, except `watch` and the
version 3 `watch_threads`. No greeting.
Request limit: 64 KiB including newline. Reply/event limit: 1 MiB including
newline. Absolute input deadline: 5 seconds; sidecar exchange deadline: 20 seconds. Slow watchers are disconnected rather
than accumulating an unbounded queue. Reconnect starts with the latest snapshot;
clients replace state, not append it. Unknown operations and malformed requests
return an error and close the connection. Unknown object fields are rejected.

Success: `{"ok":true,"result":VALUE}`.
Failure: `{"ok":false,"error":{"code":"invalid_request","message":"Safe human-readable detail"}}`.
Reply codes: `invalid_request`, `not_found`, `busy`, `conflict`, `model_unavailable`,
`permission_denied`, `internal`. Snapshot errors can also use `runtime_error`,
`interrupted`, and, in version 3 snapshots, `turn_limit`. Never send credentials, auth paths, or raw
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

Version 3 adds operations for history, thread management, a live thread list,
model changes, and queued messages; see [Protocol version 3](#protocol-version-3).

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
A submission to a thread with an active run returns `busy`, unless a version 3
client asks to [queue it](#messages-during-a-run).

`watch` first returns `{"ok":true,"result":{"thread":SNAPSHOT}}`, then zero or
more `{"event":"snapshot","thread":SNAPSHOT}` lines. The connection stays open
until disconnect, failure, or host shutdown. A disconnect does not stop work.

Delivery is event-driven. The runtime signals changed thread IDs, and one
reserved name when any thread summary changed, through a
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
Every operation accepts an optional `"protocol_version":1`, `2`, or `3`; omitting it
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

## Protocol version 3

Version 3 is negotiated per request, exactly as version 2 is. Every operation
accepts `"protocol_version":3`. Version 1 and 2 replies are byte-for-byte
unchanged, and the operations and fields below do not exist for a version 1 or
2 request: they are rejected with `invalid_request`, `Unknown operation or field.`

- **Negotiation.** A client sends `{"op":"status","protocol_version":3}`. A
  version 3 host replies with `"protocol_version":3`. A version 2 host rejects the
  number and a version 1 host rejects the field, both with `invalid_request`; the
  client then asks for version 2, and finally sends plain `status`. Clients send
  version 3 operations and fields only to a host that negotiated them.
- **Versioned replies.** Every operation that returns `{"thread":SNAPSHOT}`
  (`create_thread`, `stop`, `approve`, `watch`, and the new ones) returns a
  version 3 snapshot when the request carries `"protocol_version":3`.

```json
{"op":"status","protocol_version":3}
{"op":"list_threads","protocol_version":3,"checkout":"/absolute/checkout","limit":100,"cursor":"CURSOR","archived":true}
{"op":"watch_threads","protocol_version":3,"archived":true}
{"op":"history","protocol_version":3,"thread_id":"HOST_UUID","before":"CURSOR","limit":100}
{"op":"history_text","protocol_version":3,"thread_id":"HOST_UUID","ref":"REF","offset":65536}
{"op":"rename_thread","protocol_version":3,"thread_id":"HOST_UUID","title":"New title"}
{"op":"archive_thread","protocol_version":3,"thread_id":"HOST_UUID","archived":true}
{"op":"configure_thread","protocol_version":3,"thread_id":"HOST_UUID","provider":"anthropic","model_id":"MODEL","thinking":"high"}
{"op":"submit","protocol_version":3,"thread_id":"HOST_UUID","request_id":"CLIENT_UUID","text":"User message","approval":"ask","when_busy":"follow_up"}
{"op":"cancel_queued","protocol_version":3,"thread_id":"HOST_UUID","request_id":"CLIENT_UUID"}
```

In `list_threads`, `checkout`, `limit`, `cursor`, and `archived` are optional. In
`watch_threads`, `archived` is optional. In `history`, `before` and `limit` are
optional. In `configure_thread`, give `provider` with `model_id`, `thinking`, or
all three. In `submit`, `approval` and `when_busy` are optional. Every other
field shown is required. Timestamps are Unix milliseconds.

### Snapshot and summary

A version 3 snapshot has every version 2 key, with these changes and additions:

```json
{"id":"HOST_UUID","checkout":"/canonical/checkout","model":{"provider":"anthropic","model_id":"MODEL"},"status":"running","messages":[{"id":"STABLE_ID","role":"assistant","text":"Answer","reasoning":"Reasoning text"}],"live_text":"","tools":[{"id":"STABLE_ID","name":"read","status":"done","input":"JSON argument preview","output":"Text output preview","after":"STABLE_ID"}],"pending_approval":null,"error":null,"truncated":false,"approval_mode":"ask","title":"Fix the parser","created_at":1763300000000,"updated_at":1763300060000,"archived":false,"thinking_level":"off","queued":[{"id":"request-CLIENT_UUID","request_id":"CLIENT_UUID","text":"Queued message","mode":"follow_up","approval":"ask"}],"usage":{"input":1200,"output":300,"cache_read":8000,"cache_write":0,"reasoning":120,"total":9500,"context_used":9500,"context_window":200000},"live_reasoning":"","retry":null}
```

- **Messages.** Each message has `reasoning`: the model's readable reasoning for
  that message, or `""`. An assistant message that reasoned and then only called
  tools has `"text":""` and nonempty `reasoning`; a tool's `after` can name it.
  Version 1 and 2 snapshots omit such messages and order those tools after the
  last message with text, as before. Redacted reasoning is never sent.
- **`title`.** `""` until the first accepted message, then that message with
  whitespace runs collapsed to one space, control characters removed, and at
  most 72 characters (Unicode scalar values). A renamed thread keeps its name.
  Threads saved before version 3 get a title from their first message once.
- **`created_at`, `updated_at`.** `updated_at` advances when the thread's status
  changes, which includes every accepted run. Streaming text, rename, and archive
  do not advance it. Threads saved before version 3 report the time the store
  was first opened by a version 3 host for both.
- **`thinking_level`.** One of `off`, `minimal`, `low`, `medium`, `high`, `xhigh`,
  `max`. `off` until changed with `configure_thread`.
- **`queued`.** Inputs waiting behind the active run, oldest first. `id` is the
  message ID the input takes when it joins the conversation. `mode` is
  `follow_up` or `steer`. See [Messages during a run](#messages-during-a-run).
- **`usage`.** `null` before the first model response, then the token counts of
  the latest response that did not fail. `reasoning` is `null` when the provider
  does not report it. `context_used` is that response's total, which is what the
  next request carries; `context_window` is the model's window from the catalog,
  or `null` for a model no longer listed. There are no cost figures.
- **`live_reasoning`.** Reasoning text of the response being generated, as
  `live_text` is for its answer. Empty when idle.
- **`retry`.** `null`, or `{"attempt":1,"at":1763300065000}` while the host waits
  to repeat a failed model request. `at` is when the next attempt starts. The
  provider's error text is not sent.
- **`error`.** A run that reaches its turn limit ends with code `turn_limit`.
  Version 1 and 2 snapshots report the same event as `runtime_error`.

Bounds: latest 100 messages with text, as before, plus the latest 50
reasoning-only messages; `reasoning` keeps its last 4096 characters per message;
`live_reasoning` its last 16384; each queued `text` its last 4096. These count
UTF-16 code units like the other budgets. Clipping reasoning does not set
`truncated`. A whole version 3 snapshot is at most 768 KiB: the content shared
with version 2 is bounded to 512 KiB exactly as before, then the oldest
reasoning is dropped first. The snapshot stays the live tail; use `history` for
complete text.

A version 3 thread summary, in `list_threads` and `watch_threads`:

```json
{"id":"HOST_UUID","checkout":"/canonical/checkout","model":{"provider":"anthropic","model_id":"MODEL"},"status":"idle","title":"Fix the parser","created_at":1763300000000,"updated_at":1763300060000,"archived":false,"pending_approval":false}
```

`pending_approval` is true while the thread waits for an approval answer.

### Catalog

`status` result, version 3:

```json
{"protocol_version":3,"default_model":null,"models":[{"provider":"anthropic","model_id":"MODEL","name":"Display name","authenticated":true,"reasoning":true,"thinking_levels":["off","minimal","low","medium","high"],"context_window":200000,"input":["text","image"]}]}
```

`reasoning` says whether the model can reason. `thinking_levels` lists the levels
`configure_thread` accepts for it; a model without reasoning lists only `off`.
`context_window` is in tokens. `input` lists `text` and, where supported,
`image`; this host does not accept image input yet. All four come from Pi's
catalog.

### Full history

`history` returns a thread's complete conversation, newest page first:

```json
{"items":[{"type":"message","id":"STABLE_ID","role":"user","text":"Full text","reasoning":""},{"type":"tool","id":"STABLE_ID","name":"read","status":"done","input":"Full JSON arguments","output":"Full output"}],"before":"CURSOR"}
```

- Items are in conversation order, oldest first within a page. A tool item sits
  where its call was made; there is no `after`. IDs are the snapshot's IDs, so a
  client can replace a clipped snapshot message with its history item.
- Without `before`, the page ends at the newest item. Pass the reply's `before`
  to get the next older page; `"before":null` means the page starts the
  conversation. Cursors are opaque and stay valid as the thread grows.
- `limit` is 1 to 500, default 100. A page ends on a stored-entry boundary, so it
  can hold a few more items than `limit`, and fewer when it reaches about 600 KiB.
- Text is complete and unclipped, including inputs that were accepted but never
  ran. A tool call with no result is `running` while the thread's run is active
  and `interrupted` otherwise. Live output of a running command is only in the
  snapshot.
- **Oversized fields.** A `text`, `reasoning`, `input`, or `output` longer than
  65536 UTF-16 code units is delivered in parts. The item carries the first part
  and a `more` array, present only then:
  `"more":[{"field":"text","ref":"REF","offset":65536,"length":200010}]`.
  `offset` is where the item's part ends and `length` is the full length. When a
  page would still exceed the frame, the host shortens the largest inline parts
  further; `offset` always says how much the item holds.
- `history_text` returns the following part of one field:
  `{"text":"Next part","offset":65536,"next_offset":131072,"length":200010}`.
  Each part holds at most 65536 code units and never splits a character. Repeat
  with `next_offset` until it is `null`. `ref` is opaque. An unknown `ref` returns
  `not_found`; an `offset` beyond the text or inside a character returns
  `invalid_request`.
- Every reply fits the 1 MiB frame.

Reading history never starts or resumes work. For a thread with no active run
the host opens the saved conversation read-only: it recovers no task, runs no
tool, and sends no model request, including for a thread whose last run was
interrupted.

### Thread list and management

`list_threads` result, version 3: `{"threads":[SUMMARY],"next_cursor":null}`.

- Without `checkout` it lists threads of every checkout. With it, the checkout is
  resolved as in version 1.
- Order is `updated_at`, newest first. `limit` is 1 to 200, default 100.
  `next_cursor` is `null` on the last page; otherwise pass it as `cursor`. A
  thread that becomes active during paging can appear on two pages; merge by `id`.
- Archived threads are omitted unless `"archived":true`, which lists both.
  Version 1 and 2 listings have no archive and still include them.

`rename_thread` and `archive_thread` return `{"thread":SNAPSHOT}` and are allowed
during a run. A title is normalized like a derived one, with a limit of 200
characters; a title with no visible text is `invalid_request`. `"archived":false`
restores a thread. Archiving changes only listing: the thread, its history, and
its files are kept, and it can still run.

`watch_threads` is a subscription shaped like `watch`. Its first frame is
`{"ok":true,"result":{"threads":[SUMMARY]}}` with the 200 most recently updated
threads, archived ones only with `"archived":true`. Each later frame is

```json
{"event":"threads","threads":[SUMMARY],"removed":["HOST_UUID"]}
```

- `threads` holds only summaries that are new or changed since the previous
  frame; `removed` holds IDs that left the list, by archive or by falling out of
  the newest 200. Replace by `id`, delete the removed, and order by `updated_at`.
- A summary changes with its status, title, model, archive flag, pending
  approval, or `updated_at`. Streaming text does not produce a frame.
- Delivery follows the `watch` rules: the subscription registers before the
  first read, repeated changes coalesce into one pending wakeup per client, a
  client that does not read is disconnected rather than queued for, and a
  disconnect stops no work. After a reconnect the first frame is the full list.

### Model and thinking level

`configure_thread` changes a thread's model, its thinking level, or both, and
returns `{"thread":SNAPSHOT}`. The change applies from the next run.

- A thread with an active run, including one being stopped, returns `busy`.
- The model must be in the catalog with configured authentication, or the reply
  is `model_unavailable`. This is also checked for the current model when only
  the thinking level changes.
- `thinking` must be in the chosen model's `thinking_levels`, or the reply is
  `invalid_request`. When only the model changes, a level the new model lacks
  moves to its nearest supported level.
- The choice is saved with the thread and survives restart. The default model
  still affects only new threads.

### Messages during a run

Without `when_busy`, `submit` to a thread with an active run returns `busy`, as
before. With it, the input is accepted and queued:

- **`"when_busy":"follow_up"`** waits for the active run to end, then starts a
  new run.
- **`"when_busy":"steer"`** joins the active run at its next step: after the tool
  calls in progress finish, or when the model finishes its answer. It runs under
  that run's authorization and approval mode. A steering input the run did not
  take before it ended stays queued and then behaves as a follow-up.
- If the thread has no active run and nothing queued, `when_busy` has no effect
  and the submission starts a run at once.
- `approval` is the mode of the run this submission starts, now or later. A
  steering input that joins a run does not change that run's mode.

The reply is the usual `{"thread_id":"HOST_UUID","request_id":"CLIENT_UUID","accepted":true}`.
A queued input is saved with FULL durability before the reply. Request IDs
deduplicate as for any submission: the same ID and text returns the original
acceptance without queueing again, and different text returns `conflict`. A
thread holds at most 16 queued inputs; one more returns `busy`. `when_busy` does
not wait for another thread's run on the same checkout: that is still `busy`.

A queued input is listed in the snapshot's `queued` and is not a message yet.
When it starts or is taken, it leaves `queued` and appears in `messages` and
`history` with the ID `request-CLIENT_UUID`, at the position where it ran.

- **Starting.** When a run ends with status `idle`, the oldest queued input
  starts as a new run through the same admission as a submission: checkout
  lease, checkout identity check, and a new authorization object in that input's
  own `approval` mode. Nothing carries over from the run before it.
- **Held queue.** A run that ends in `error` or `interrupted`, and a host
  restart, leave the queue in place and start nothing. Queued inputs survive
  restart. The next submission with `when_busy` to that thread joins the queue
  and starts it from its oldest input; a plain submission runs first, and the
  queue follows when that run ends `idle`. A queued input that cannot start, for
  example because its model is no longer available, sets the thread to `error`
  with that reason and stays queued.
- **Stop** cancels every queued input of the thread, as well as the run. An
  input sent with `when_busy` after Stop starts when cleanup finishes.
- **`cancel_queued`** removes one queued input and returns `{"thread":SNAPSHOT}`.
  An input that already started, was taken by the run, or was cancelled returns
  `not_found`.
- A cancelled input never runs. Its request ID stays used: repeating it returns
  the original acceptance and queues nothing.

### Run limits

- **Turn limit.** A run may take 300 tool rounds. The round that reaches the
  limit completes its tools, then the run ends with status `error` and
  `{"code":"turn_limit","message":"..."}`. Nothing is lost: the next submission
  continues from those results with a new limit.
- **Retry.** A model request that fails with a transient provider error, such as
  overload, rate limiting, or a dropped connection, is repeated up to three
  times, after about 2, 4, and 8 seconds. The snapshot shows `retry` while
  waiting, and Stop cancels the wait. Other provider errors, and a fourth
  failure, end the run with `runtime_error` as before. Only the model request is
  repeated. A tool call that ran is recorded before the next request and is
  never run again, and no retry follows a host restart.

### Compatibility

- Version 1 and 2 clients keep working against a version 3 host and see none of
  the above, with one exception: a thread's model can now change between runs.
  Desktop builds from before version 3 treat a model change on the thread they
  are watching as a protocol error and must reselect the thread.
- A version 3 client must not send version 3 operations to an older host, and
  must treat every version 3 field as absent there.
- Storage changes are additive. A store written by an older host loads without
  an export step: on first start the host adds summary columns to `host.sqlite`
  and fills them from the saved snapshots. Repeating the migration changes
  nothing. Downgrading a host after version 3 state was saved is not supported.
- The host and its runtime sidecar are one update unit, as before. The internal
  notification channel gained one reserved name for thread-list changes.

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

A version 3 [queued input](#messages-during-a-run) is the only deferred start. The
host saves its text and the approval mode its user submitted it with; that is a
recorded choice, not a grant. When the input starts, it passes the same
admission as a new submission and gets its own authorization object, so nothing
from the run before it, including automatic approval, carries over. A queue
never starts by itself after a failed run or a host restart. A steering input
joins a run that is already authorized and adds no permission to it. There is no
other scheduler and no persisted grant in this contract. Future automation
grants require separate protocol, persistence, and permission review.

## Provider sign-in

These operations change which providers the host can use while it runs. The
desktop Settings UI in source builds calls them. They are additions, not
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

## Command policy

Each canonical checkout has a command policy: `sandboxed`, the default, or
`trusted`. It decides how the `command` tool runs for runs in that checkout.
Trusted mode is opt-in per project. Source builds expose it under Settings →
Approvals → Command policy, with explicit confirmation of its consequences.
The host keeps sandboxed mode until a client sends the operation.

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
`invalid_request`; the desktop reports that policy could not be loaded. No existing request
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

Load checkout `AGENTS.md` explicitly before execution. A thread's checkout is
immutable. Its model and thinking level change only through the version 3
`configure_thread`, never during a run; changing the default affects only new threads.
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
