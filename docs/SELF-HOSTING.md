# Build Plait with Plait

The first useful loop is **edit → test → review → continue**. The native client
is independent of the host, so closing it does not stop accepted work. The app's
Git view remains read-only. Use Rex for commits, dependency changes, packaging,
and native app inspection.

## Prepare once on the Mac

Installing and using Plait does not require Rust or Xcode. Building Plait does.
Use an Apple Silicon Mac with Git, rustup, Node/npm, and full Xcode. Install the
Metal component if needed:

```sh
xcodebuild -downloadComponent MetalToolchain
```

In your Plait source checkout, inspect existing work, then prepare dependencies:

```sh
git status --short
bash scripts/prepare-self-host.sh
```

Run this trusted development command in a named Rex pane, not in an agent tool.
It fetches the locked Cargo sources, installs locked runtime dependencies without
npm lifecycle scripts, builds the workspace, and runs its Rust tests. It does not
change source files, reset Git, install the app, or use model credentials. A new
dependency or toolchain version needs preparation again.

Do not reset, stash, or discard existing work to make preparation pass. Plait
protects dirty files that the current thread does not own. If a planned change
needs one of those files, finish or commit that work yourself first, or choose a
different change. Worktrees are not required.

## Open the installed app

1. Install the [Mac preview](MACOS.md). Open Plait from Applications.
2. Use **Open repository…** to select this checkout.
3. Open **Settings** and select an authenticated provider/model. The host uses
   your Pi account; credentials stay on the host. See the Mac guide for login.
4. Start a new thread. Check the checkout, machine, and model before sending.

A good first task is:

> Make Plait's thread title omit trailing whitespace when its 72-character
> limit cuts just after a word. Add a regression test for that boundary. Do not
> add dependencies. Read the project instructions first and preserve existing
> dirty files. Run `cargo test --offline --locked -p plait-core --lib`.
> Show the real diff and test result. Do not stage or commit.

Review each exact edit and command before **Allow once**. The command reads the
checkout and writes build output only to a private thread cache. It cannot write
the checkout, contact external services, or start a background development server.
Only approved file tools write project files.

## Check the result

- Read the actual test output and final response. A proposed command is not a
  completed test.
- Open **Changes**. Git refreshes after observed work finishes; Cmd-R refreshes
  explicitly. Inspect both staged and unstaged work.
- Continue in the same thread. Its unchanged approved files remain editable by
  that thread; externally changed files remain protected.
- Close and reopen the client. The last checkout/thread and drafts restore.
  If acceptance was uncertain, **Retry safely** reuses the saved request ID.
  Reopening does not submit or replay a task automatically.
- Stage and commit reviewed work in Rex. Run `bash scripts/check.sh` there for
  the full runtime and platform checks, then build/install the next package.

The full runtime suite intentionally tests network and process boundaries. It is
not a promise that every development test can run inside an agent command's
restricted sandbox. Native window inspection and package installation also stay
outside that sandbox. Do not relax containment to make a test green.

## Updating while building

Use a client-only package to replace the UI after closing it. Accepted host work
continues. A host package stages an immutable release. Finish active work and
wait for cleanup before activating it through a session restart. See [Mac
updates](MACOS.md#independent-updates). No updater may kill a build to replace its
runtime.

## Verification boundary

Scripted production-provider replay tests establish decoding, approval, file
edits, sandboxed Cargo commands, durable identity, and reconnect behavior without
provider credentials. They do not measure live-model quality. Complete the first
live task on your own installed Mac before relying on Plait for valuable work.
The complete conversation remains in the host's durable store, but the current
UI exposes only a bounded transcript tail, with a visible clipping notice.
