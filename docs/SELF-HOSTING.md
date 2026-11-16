# Build Plait with Plait

Use Plait for a complete development cycle: investigate a feature, change the
Rust desktop/core or TypeScript runtime, run checks, review the diff, revise,
and continue in the same thread. The host owns execution and durable history;
closing the desktop does not stop accepted work.

The steps below describe current source builds. The released preview may lack
these controls; see [Mac installation](MACOS.md) for the installed version.

## Prepare and install

Building Plait requires an Apple Silicon Mac, Git, rustup, Node/npm, and full
Xcode with Metal. Installing and using Plait does not require those build tools.
If needed, install the Metal component with
`xcodebuild -downloadComponent MetalToolchain`.

From this source checkout, inspect existing work and prepare dependencies:

```sh
git status --short
bash scripts/prepare-self-host.sh
scripts/update-installed-macos.sh
```

Preparation fetches locked Cargo sources, installs locked runtime packages
without npm lifecycle scripts, builds, and runs the Rust tests. It does not
reset files or use model credentials. Installation preserves credentials,
threads, and drafts. **Settings → About** shows the desktop revision and host
protocol; a new host may be staged until existing runs finish.

Open this checkout with **Open repository…**, choose an authenticated model in
**Settings → Providers / Models**, and start a thread. Existing dirty files
remain protected by the file tools; commit or finish that work before asking a
new thread to edit those files. Worktrees are optional.

## Choose the development command policy

**Sandboxed** is the default. File tools can make approved edits, and commands
can run offline locked Cargo builds using a persistent per-thread build cache.
Commands cannot format source files, install dependencies from the network,
write the checkout, or run every test that probes network/process boundaries.
Run those steps externally when keeping this policy.

For the full development loop, explicitly select **Trusted** for this project
in **Settings → Approvals**. Trusted commands run as your user, with the login
shell environment, network access, and checkout/home writes. This enables
formatting, package installation, generation, and the full runtime test suite.
It also permits commands to alter pre-existing work; file-tool dirty-path
checks do not constrain a trusted shell. The app shows a confirmation before
changing this policy. Other projects keep their own policy.

Approval mode is a separate choice. **Ask each time** reviews every effect;
automatic approval allows tools to proceed within the selected command policy.
Both modes retain Stop, the command deadline, and one active writer per checkout.

## Work on a substantial change

Give the agent the desired behavior, constraints, and acceptance checks. A
cross-file feature can include Rust state transitions, native presentation,
runtime behavior, and focused tests. Ask it to read project instructions and
the current milestone first. Run the relevant focused checks during iteration;
the complete gate is:

```sh
bash scripts/check.sh --require-command-sandbox
python3 -m unittest discover -s scripts -p 'test_release_*.py'
```

The full gate requires Trusted commands or an external terminal. It runs the
real sandbox tests; choosing Trusted for development does not disable them.
Native window inspection remains a separate step after UI changes.

The agent's `read` tool supports byte ranges and gives an explicit continuation
for large files. Exact edits support UTF-8 files up to 8 MiB, with bounded edit
arguments and identity checks. Command results include a `read_output`
reference: the agent can retrieve earlier build diagnostics without rerunning
an effect. The host retains up to 16 MiB per command and the newest 32 command
logs per thread, with explicit truncation/expiration notices. These logs can
contain command secrets, so avoid printing credentials. See [tools](HOST.md#development-tools).

Use **Changes** to review the actual diff. Continue in the same thread for
revisions; its unchanged edited files remain editable after restart. Send a
follow-up or steering message while it works, or Stop before changing direction.
Change the model/thinking level between runs. Staging and commits use your own
Git tools; the app's Git interface remains read-only.

## Continue long-running work

The runtime automatically summarizes older model context as the conversation
fills. Summarization does not delete the stored conversation: the desktop pages
through the original messages and tool results. Internal context summaries are
not presented as messages you sent. A failed required summary reports a
recoverable error; send a new message or choose another model to retry.

Reopening restores the selected thread and drafts without sending anything.
After an interrupted run, inspect completed effects before continuing. **Retry
safely** reuses an uncertain submission's request ID. A host restart never
replays a tool effect or resumes a model request on its own, including a
summarization request. Stop clears queued follow-ups.

## Install the next version

Finish the agent run, review and commit the change externally, then run:

```sh
scripts/update-installed-macos.sh
```

The desktop and host run from installed artifacts, so edits in the source tree
do not replace the host executing the current task. The installer stages a new
host while runs or approvals are active. Finish or Stop them before
`scripts/update-installed-macos.sh --activate-host`. Reopen the app and continue
in the same project and thread. Do not ask a running agent to replace its own
host mid-task.

## Acceptance and limits

Automated tests cover large-file reads/edits, retained command diagnostics,
automatic compaction, restart during compaction, and continuation without
replay. Production-provider replay tests run real Plait source edits and Cargo
commands without account credentials. These are distinct from live-model
quality and installed native interaction.

The self-hosting acceptance bar is a real multi-file feature with diagnosed
failures, follow-up revisions, diff review, an installed resulting build, and
continued work afterward. It is not established by a single small edit. See
[Verification](VERIFICATION.md) for the recorded coverage and remaining limits.
Plait has no embedded terminal, native visual-testing agent, or agent-team
orchestration; external tools still handle native inspection and interactive
debugging.
