# macOS installation and independent updates

Plait is an early local preview for **Apple Silicon and macOS 15 or later**.
The client and host are separate programs. Closing or updating the client does
not stop accepted host work. Model requests require access to the configured
provider; installing Plait does not include a model or offline inference engine.
The bundle declares macOS 15, but packaging CI uses macOS 26. Older-system
runtime acceptance is not yet complete.

## Download and install

Published previews live on the [GitHub releases page](https://github.com/heath0xFF/plait/releases).
Download the **DMG**, open it, then run **Install Plait.pkg**. The disk image also
has **Update Client.pkg** for an already-installed host build. This is an
installer-based app and host, not a drag-to-Applications client that silently
omits the execution runtime.

For a command-line installation or update, download and inspect the installer:

```sh
curl --proto '=https' -fL \
  https://raw.githubusercontent.com/heath0xFF/plait/main/scripts/install.sh \
  -o install-plait.sh
less install-plait.sh
bash install-plait.sh --allow-unsigned
```

The short form is:

```sh
curl --proto '=https' -fsSL https://raw.githubusercontent.com/heath0xFF/plait/main/scripts/install.sh | bash -s -- --allow-unsigned
```

Run it as your normal user, not under `sudo`. It requests
administrator access only for the package installation. The flag explicitly
accepts the unsigned preview; the script does not change Gatekeeper or remove
quarantine attributes.

The script uses the newest published release, including previews.
`--version v0.1.0-preview.2` pins a release; `--component client` updates only the app.
`--download ~/Downloads` checks and saves the package without installing it.
It verifies the package against the release's SHA-256 metadata over HTTPS.
This detects corruption, not publisher identity; it is not a signed updater.
An already-installed identical host is left alone and the default installation
selects only the client. No active process is stopped.

## Packages

`bash scripts/build-macos.sh` produces:

| Artifact | Purpose |
| --- | --- |
| `Plait-…-macos-arm64.dmg` | Finder download containing the combined installer, client update, and instructions |
| `Plait-Installer-…-macos-arm64.pkg` | Install both components; Customize can select either one |
| `Plait-Client-…-macos-arm64.pkg` | Install or update only `/Applications/Plait.app` |
| `Plait-Host-…-macos-arm64.pkg` | Install or stage only the host, Node, and pinned runtime |
| `Plait-…-macos-arm64.zip` | Portable client bundle; no host installation |
| `Plait-Release-….json` and `.sha256` files | Component metadata and artifact checksums |

**These are unsigned preview installers, not notarized releases.** The client
and Rust host have ad-hoc signatures. The bundled Node binary retains its
upstream signature. A checksum detects corruption; it does not prove publisher
identity. Check the source and artifact origin before using macOS's explicit
per-app or per-installer approval. Do not disable Gatekeeper. CI installation
does not test the downloaded-file approval flow on your Mac. If macOS blocks the
installer, review its origin, then use the explicit **Open Anyway** option in
System Settings → Privacy & Security if offered. If machine policy forbids it,
stop; this preview provides no security-policy bypass.

## First installation

1. Finish work in any manually started development host. Do not start two hosts
   against the same store. The installer never kills an existing host.
2. Quit the Plait client, then open the combined `.pkg`. Installer needs an
   administrator account to install shared program files.
3. Open **Plait** from Applications. Use **Open repository…** (Cmd-O), then
   **Settings** (Cmd-comma) to check the local host and choose an authenticated
   model. Reconnect if the client opened before the host was ready.
4. Start a new real thread. Review each exact edit or command before approval.
   The app shows only real threads; there is no sample conversation.

The host is a **per-user LaunchAgent**, not a root daemon. It starts for the
current graphical user when possible and at later graphical logins. If no
user is logged in, startup waits until login. Service startup resolves the home
from the OS account and rejects root execution; it does not use the installer's
environment. It uses the same default store as
the development host: `~/.local/share/plait`. A development store override is
not copied into the installed agent.

The package includes Node **26.12.0**, the Rust host, runtime source, and locked
production npm dependencies. It needs no Homebrew or global Node installation.
It does **not** include model credentials, user data, project dependencies,
Rust, Git, Xcode, or a general terminal. Keep the development tools needed by
your project installed. Sandboxed Cargo builds remain offline and need cached
sources and an existing lockfile. Full Xcode and its Metal component are needed
for native Plait builds.

### Authentication and status

The host owns provider accounts. Sign-ins and API keys are kept in Pi's
credential file, `~/.pi/agent/auth.json`, readable only by your account. Custom
OpenAI-compatible servers are entries in `~/.pi/agent/models.json`. A login or
API key set only in an interactive shell is not automatically available to a
LaunchAgent. Do not put credentials into the system LaunchAgent property list or
the package.

Locate the installed host with native macOS tools:

```sh
build_id=$(/usr/bin/plutil -extract build_id raw -o - \
  '/Library/Application Support/Plait/Host/installed.json')
host="/Library/Application Support/Plait/Host/releases/$build_id"
"$host/bin/plait-host" version
"$host/bin/plait-host" status
```

#### Signing in from Plait

Source builds after v0.1.0-preview.2 provide **Settings → Providers**. The host
must support the [provider operations](HOST.md#provider-sign-in); an older host
shows an update requirement. These controls have scripted and scene coverage;
live provider sign-in acceptance remains open.

- **Subscription** (for example Claude Pro or Max, or ChatGPT Plus or Pro): choose
  the provider and sign in. Use **Open in browser** after checking the displayed destination. The
  browser normally returns to the host by itself; if it cannot, paste the code
  or the final address into Plait. Some providers show a short code to type on
  their page instead.
- **API key** (for example OpenRouter): paste the key once. Plait sends it to the
  host and does not keep or show it again. Replace it by entering a new one.
- **Your own server:** give an OpenAI-compatible server a name, base URL,
  optional key, and model IDs. **Check connection** lists the models it reports.
- **Sign out** removes the stored sign-in or key for one provider.

Closing the provider form cancels a pending sign-in. API keys and sign-in answers
are masked, have a reveal control, and are cleared when sent or closed. They are
not copied into desktop preferences, the recovery journal, accessibility values,
or clipboard copy. A connection check consumes an entered key without saving it;
paste it again before saving a new endpoint. Endpoint removal requires confirmation.

**Settings → Approvals → Command policy** changes the current checkout between
Sandboxed and Trusted. Trusted requires confirmation of full user access, network,
Git writes, and the ability to change already-modified files. It affects the next
run; an active run retains its original policy. The approval default is independent.

The desktop never receives a stored credential. Changes apply to new threads and
new messages at once. A run already in progress continues with the providers it
started with. Existing threads retain their exact saved model; changing the
default affects only new threads.

#### Pi CLI fallback

The host payload includes the pinned Pi CLI:

```sh
"$host/bin/node" \
  "$host/runtime/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js"
```

Use its `/login` command, then exit. Do not enter coding tasks into that CLI if
you intend them to use Plait's approval and sandbox rules.

The v0.1.0-preview.2 host reads credentials only when it starts. Finish any host
work, then restart the host session; the simplest restart is to log out and
back in. A host built from later source can load them without a restart:

```sh
printf '{"op":"reload_providers"}\n' | /usr/bin/nc -U "$HOME/.local/share/plait/host.sock"
```

### If the host does not connect

Use **Reconnect** once. With the installed `$host` path from
[Authentication and status](#authentication-and-status), check the service and its
last startup record. These checks do not stop a run or change saved work:

```sh
"$host/bin/plait-host" status
/usr/bin/plutil -p "$HOME/.local/share/plait/startup.json"
```

The startup record contains phases, elapsed times, process IDs, and OS error
kinds—not credentials or prompts. It describes the last startup attempt, not
current health; a past `ready` phase does not prove that the host is still running.
The `version` command identifies the binary you invoke. After a staged update,
that can be newer than the active host.

Do not delete the host store or force a restart to clear an error. Follow the
[independent update and activation rules](#updates). When reporting a
problem, include the macOS version, build, final startup phase, and error kind.
Do not include credentials, private conversations, or a complete environment dump.

## Workspace recovery

Plait restores the last checkout and selected thread. It also saves draft text
and uncertain submission IDs. Reopening never sends or replays a task. **Retry
safely** uses the original request ID. An acknowledgement cannot clear a draft
that you edited after sending. Settings shows authenticated models first; **Show
all models** includes unavailable choices without making them selectable.

The Mac recovery journal is
`~/Library/Application Support/Plait/session/desktop.json`. It is private local
JSON, not encrypted by Plait, and can contain sensitive prompt text. Normal
window close and Cmd-Q wait for a save. Force quit and power loss can lose the
last unsaved keystrokes. Accepted work remains in the independent host store.

If recovery reports an error, do not delete or reset the journal. Close duplicate
Plait clients first. Keep a copy of the file before repair; malformed or unknown
versions are left intact. A conflicting saved checkout disables sending
for that thread rather than moving an old draft or request to a new identity.
Changing a thread’s model does not discard its draft or saved selection.
For development with a different `PLAIT_HOST_STORE`, use a separate absolute
`PLAIT_DESKTOP_STATE_DIR`. Reconnect does not erase local state or restart work.

For a prepared development checkout and a first source-change task, see
[Build Plait with Plait](SELF-HOSTING.md).

## Projects and threads

Source builds after v0.1.0-preview.2 list projects across the local host's checkouts.
Opening a folder adds it to the sidebar. Selecting a thread in another project
switches its conversation, draft, and Git workspace together. The local machine
appears in workspace context without a redundant row beneath every project.
Missing folders remain visible as **Missing**; their saved conversations can still
be read.

Use the sidebar search to filter titles and project names. **Needs me** includes
unread activity, pending approvals, failures, and interrupted runs. **Archived**
includes archived conversations. Thread actions and the **Thread** menu offer
rename and archive/unarchive. Archiving keeps the conversation and does not stop
an active run. Removing a project from the sidebar keeps its host threads;
**Show hidden projects** or a project entry in the command palette brings it back.

The host keeps its newest 200 threads live. When more are loaded, the sidebar
states the limit; use **Refresh all threads and folders** for older threads and
folder availability. A disconnected thread-list watch reports that updates are
paused. Hosts using protocol 1 or 2 retain their one-checkout thread listing and
journal titles; rename and archive require version 3.

**Alerts on/off** controls system notifications and window attention requests.
Unread markers remain when alerts are off. Alerts occur for a thread you are not
viewing, or while Plait is inactive, when it finishes, fails, stops, is interrupted,
or needs approval. Opening a notification selects its thread. A bundled Mac app
requests notification authorization on first delivery; an unbundled build cannot
post macOS notifications, and delivery depends on system permission and Focus
settings. The initial listing never posts alerts. Native delivery and activation
remain manual acceptance checks.

## Keyboard controls

When focus is in the conversation, Home or Cmd-Up goes to the beginning of the
loaded history and requests an earlier page when one is available. End or Cmd-Down
returns to the latest messages. Text fields keep their usual editing shortcuts.
Version 3 history also loads as you scroll toward the top; use **Load earlier**
for an explicit page request and **Retry** after a failed read. Older hosts retain
bounded previews. Completed reasoning expands with **Show reasoning**.


### Application shortcuts

These are the default shortcuts for application actions. The table is generated
from the action table in `crates/plait/src/ui/actions.rs`, and a test fails when
the two differ. The ID is the action's stable name. **Settings → Keyboard**
records shortcuts, searches actions, clears a binding, and restores defaults.
Conflicts name the affected actions and require **Reassign**. Text-editing and
common system shortcuts are refused. Escape cancels recording; the key being
recorded never runs its normal action. The nine sidebar jumps change together.
Menus keep Settings and Quit reachable even when their shortcuts are cleared.

Overrides live in `~/Library/Application Support/Plait/desktop.json` under
`keymap`, for example `"keymap": {"thread.new": "cmd-t", "app.quit": ""}`.
An empty string unbinds; an invalid keystroke keeps the default. Unknown action
IDs survive saves. Menus and palette hints update immediately. For an isolated
development instance, set `PLAIT_DESKTOP_PREFERENCES_FILE` to an absolute file
path alongside separate `PLAIT_HOST_STORE` and `PLAIT_DESKTOP_STATE_DIR` paths.

| Shortcut | Action | ID |
| --- | --- | --- |
| Cmd-comma | Settings… | `app.settings` |
| Cmd-Q | Quit Plait | `app.quit` |
| Cmd-N | New Thread | `thread.new` |
| Cmd-Shift-N | New Thread with Model… | `thread.new_with_model` |
| Cmd-Shift-M | Change Thread Model… | `thread.switch_model` |
| Cmd-O | Open Repository… | `repository.open` |
| Cmd-R | Refresh Git Status | `git.refresh` |
| Cmd-W | Close Window | `window.close` |
| Cmd-K | Command Palette… | `palette.toggle` |
| Cmd-B | Toggle Sidebar | `view.toggle_sidebar` |
| Cmd-Option-B | Toggle Git Inspector | `view.toggle_inspector` |
| Cmd-Shift-D | Toggle Diagnostics | `view.toggle_diagnostics` |
| Cmd-Shift-1 | Conversation | `view.conversation` |
| Cmd-Shift-2 | Changes | `view.changes` |
| Cmd-Option-Down | Next Thread | `thread.next` |
| Cmd-Option-Up | Previous Thread | `thread.previous` |
| Option-Down | Next Changed File | `git.next_file` |
| Option-Up | Previous Changed File | `git.previous_file` |
| Cmd-M | Minimize | `window.minimize` |
| Cmd-1 to Cmd-9 | Jump to Sidebar Item 1–9 | `jump.1` to `jump.9` |
| Tab | Focus Next Control | `focus.next` |
| Shift-Tab | Focus Previous Control | `focus.previous` |

Cmd-N creates a thread with the saved default model and puts the cursor in its
composer. When no default can be used, it opens the model picker and creates
the thread on selection. Cmd-Shift-N always asks for the model. Cmd-1 to Cmd-9
count threads in sidebar order; Settings can change this to machines or
projects. Cmd-Option-B hides or shows the Git inspector beside the conversation
and the choice is remembered. On a window too narrow for the inspector it
explains where Git is instead.

### Command palette and model picker

Cmd-K opens the command palette: threads in the open checkout, every menu
command with its shortcut, and "New thread with" each signed-in model. The model
picker is the same list over the host's model catalog.

| Key | Action |
| --- | --- |
| Type | Filter. Words match in any order, in the title or the detail |
| Up / Down | Move the selection; it wraps at the ends |
| Page Up / Page Down | Move a page |
| Return | Run the selected item |
| Cmd-Return | Model picker: save the selected model as the default |
| Cmd-D | Model picker: add or remove a favorite |
| Escape | Close and return focus to where it was |

### Working during a run

With a version 3 host, Enter queues a follow-up and Option-Enter sends steering.
Steering joins the current run at its next step and uses that run's approval
mode; if it is not taken in time, it becomes a follow-up. Stop stays beside the
send control and clears the host queue as well as stopping the run.

The stack above the composer lists waiting messages in order. Cancel removes
one; Edit cancels it back into an empty draft. Edit is offered for messages sent by this desktop, whose complete text is
saved locally; the host only sends a bounded preview. If the
draft changes while cancellation is in flight, the cancelled text is copied to
the clipboard and the draft stays intact. Unconfirmed messages each offer Retry
safely, using their original request ID and approval mode. Reopening sends
nothing. A held queue resumes from its oldest input on the next send.

Choose the model beneath the composer, or Cmd-Shift-M, to change this thread's
next run. The adjacent thinking control lists exactly the chosen model's
supported levels. Both are locked during a run. The next-message approval
control does not change an active run or previously queued messages.

Context usage shows the latest response, not cumulative cost. Hover or activate
it for exact token counts. At 75% it says high; at 90% it says nearly full. It
stays hidden when context usage or the window size is unknown. Older hosts keep
their existing send-when-idle behavior and have no per-thread model controls.

### Text and controls

| Key | Action |
| --- | --- |
| Enter / Shift-Enter | Send, or queue a follow-up during a run / insert a line break |
| Cmd-Enter | Send or queue a follow-up, even with a suggestion list open |
| Option-Enter | Steer the running agent at its next step (version 3 host) |
| Up / Down in the draft | On the first line, Up recalls earlier messages sent in this thread; Down returns toward the draft |
| @ | List checkout files at the start of a word; type to filter |
| / | List local commands when it is the first character of an empty draft |
| Up / Down, Enter or Tab, Escape | With a suggestion list open: choose, accept, dismiss |
| Escape | Close the suggestion list, then leave message recall |
| Escape twice | Stop the running agent, when pressed twice within 1.5 seconds |
| Option-Delete / Option-Fn-Delete | Delete the previous / next word |
| Cmd-Delete / Cmd-Fn-Delete | Delete to the start / end of the line |
| Option-Left / Right | Move by word; add Shift to select |
| Cmd-Left / Right | Move to the start / end of the line; add Shift to select |
| Cmd-A / Cmd-C / Cmd-X / Cmd-V | Select all / copy / cut / paste |
| Cmd-Z / Cmd-Shift-Z | Undo / redo |
| Return / Space | Activate the focused control |

Search fields take one line: pasted line breaks become spaces.

Scroll the conversation, navigation, file list, and diff with the trackpad or
mouse. Long diff lines scroll horizontally. On narrow windows, use Changes to
open Git; the optional conversation inspector is hidden.

## Updates

### In the app

**Plait → Check for Updates…** (also **Settings → About**) looks for the newest
signed release on GitHub Releases, including previews. Plait also checks quietly
at launch; when a release is available, an **Update** button appears beside
Settings in the sidebar.

**Install and Restart** downloads the release, verifies it, replaces
`/Applications/Plait.app`, installs that release's host, restarts the host, and
reopens Plait. It refuses while any thread is running or awaiting approval,
because a host restart would interrupt that run. Finish or stop the run first.
Saved drafts and the workspace layout are kept, as on a normal quit.

Each release publishes `update.json` and a detached Ed25519 signature. Plait
checks the signature against the key built into the app before it trusts the
version or any artifact, then checks each download's signed size and SHA-256.
An edited manifest cannot make an older signed build look newer. The app's code
signature remains ad hoc; downloads are not notarized.

In-app updates need a per-user install: an app bundle you own and a host under
`~/Library/Application Support/Plait/Host`. An install made by the published
Installer packages is root-owned. Move it once with
`scripts/update-installed-macos.sh` from a checkout (it asks for administrator
access once). Release builds compare versions; a source build is offered the
newest release.

### Headless hosts

On a Mac that runs only a host, `plait-host update --check` reports whether a
newer signed release exists, and `plait-host update` installs its host archive
with the same checks and the same refusal while runs are active. Linux hosts do
not have published builds yet.

### From a source checkout

Contributors can install their working tree as this Mac's Plait:

```sh
scripts/update-installed-macos.sh
```

It builds the app (uncommitted changes included), quits it, replaces
`/Applications/Plait.app`, and reopens it. The app carries its host; the script
unpacks it into `~/Library/Application Support/Plait/Host/releases/` only when
its content changed and runs it from a per-user LaunchAgent. No administrator
access is needed, except once to remove an earlier system-wide package install;
threads, drafts, and credentials stay where they are.

A new host replaces the running one only while no run is active or awaiting
approval; otherwise it is staged. Activate it later with `--activate-host`, or
keep the current host with `--stage-host`. After activation, releases other than
the running one are removed. **Settings → About** shows the running desktop
revision, build time, and host protocol. Builds are ad-hoc signed and not
notarized.

This per-user install trades one protection for convenience: the system-wide
package keeps host code owned by root, so processes running as you cannot change
it. Sandboxed agent commands cannot write there; commands in a project set to
the opt-in [trusted command mode](HOST.md#command-policy) run as you and can.

### Installer packages

The published Installer packages predate in-app updates and install system-wide:

- **Client:** quit only the client and run a client-only package. The host and
  accepted work remain running. Reopen the client to reconnect.
- **Host:** run a newer host-only package. It installs a content-addressed release
  directory and updates the LaunchAgent file. Each host release has a distinct
  package receipt, so Installer does not remove the previous release's files.
  The LaunchAgent label stays `dev.plait.host`. Installation does not stop the active host.
  Finish active threads, or Stop them and let command cleanup finish. Then log
  out and back in to activate the update. Shutdown during cleanup leaves an
  **interrupted** thread for inspection; it does not replay the accepted request.
- Installing the **same exact host build again is rejected before payload writes**.
  This prevents Installer from rewriting code used by a running process. Use the
  client-only package if only the client needs installation.
- A manually started host is not adopted or killed. Finish its work and stop it
  before activating the installed agent. A store lock rejects duplicate writers.

The v0.1.0-preview.2 client and host use protocol version **1**. Later source
builds negotiate opt-in [version 3](HOST.md#protocol-version-3), then
[version 2](HOST.md#protocol-version-2), and fall back to
version 1, so either side can be updated first. The client rejects other versions. Future protocol or storage changes need an explicit compatibility and
migration review; arbitrary future versions are not promised to interoperate.
Package installs have no automatic rollback or release-directory eviction. Do
not delete an old release while its host or sidecar can still be running.

Package program files:

```text
/Applications/Plait.app
/Library/LaunchAgents/dev.plait.host.plist
/Library/Application Support/Plait/Host/installed.json
/Library/Application Support/Plait/Host/releases/<content-build-id>/
  bin/plait-host
  bin/node
  runtime/src/
  runtime/node_modules/
  component.json
  LICENSE, NODE_LICENSE.txt, THIRD_PARTY_NOTICES.txt
```

`installed.json` describes the latest installed host, not necessarily the active
host. The active job keeps its original immutable program and runtime paths.
Threads, private build caches, and credentials are outside these program files
and are not installer payloads.

## Build and verification

Use a Git checkout on an Apple Silicon Mac with rustup, Python 3, Node/npm, and
full Xcode with Metal. Run:

```sh
bash scripts/check.sh --require-command-sandbox
bash scripts/build-macos.sh
```

To inspect the built client without installing it, run
`open dist/Plait.app --args /absolute/path/to/repository`. Agent work still needs
a running [local host](HOST.md).

The build compiles the locked workspace, generates client and host Rust notices
with pinned `cargo-about` 0.9.3, verifies the pinned Node download checksum, and
installs locked production npm dependencies without lifecycle scripts. It checks
Mach-O architecture, system-only dynamic links, signatures, property lists,
package contents, and the extracted client ZIP. Building never installs into
`/Applications` or starts a user agent.

The Mac workflow also installs the combined package on a disposable runner. It
checks startup without a developer Node path, reruns the real Mac sandbox suite
with the packaged host and runtime, checks independent component installation
during a scripted command, and checks saved state after host activation. Its
upgrade payload is explicitly labeled a disposable fixture and is not published.
The workflow launches the installed client and captures its window when the
runner has a display. These are scripted checks, not live-model acceptance or
validation on your Mac. See [verification](VERIFICATION.md) for results.

Branch builds upload a `plait-macos-arm64-…` workflow artifact, which expires
after 14 days. Version tags run the complete Mac checks again, then a separate
publish job creates an immutable GitHub prerelease with the DMG, packages,
installer script, and checksums. A failed build or installer test publishes
nothing. The job does not use signing secrets. [Issue #6](https://github.com/heath0xFF/plait/issues/6)
tracks Developer ID signing, notarization, and verified automatic update delivery.
