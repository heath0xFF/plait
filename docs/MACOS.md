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

The host reads your existing Pi authentication. A login or API key set only in
an interactive shell is not automatically available to a LaunchAgent. Do not
put credentials into the system LaunchAgent property list or the package.

Locate the installed host with native macOS tools:

```sh
build_id=$(/usr/bin/plutil -extract build_id raw -o - \
  '/Library/Application Support/Plait/Host/installed.json')
host="/Library/Application Support/Plait/Host/releases/$build_id"
"$host/bin/plait-host" version
"$host/bin/plait-host" status
```

For first-time authentication, the host payload includes the pinned Pi CLI:

```sh
"$host/bin/node" \
  "$host/runtime/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js"
```

Use its `/login` command, then exit. Do not enter coding tasks into that CLI if
you intend them to use Plait's approval and sandbox rules. Finish any host work,
then restart the host session to load new credentials. The simplest restart is
to log out and back in. Existing threads retain their exact saved model;
changing the default affects only new threads.

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
[independent update and activation rules](#independent-updates). When reporting a
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
versions are left intact. A conflicting saved checkout/model disables sending
for that thread rather than moving an old draft or request to a new identity.
For development with a different `PLAIT_HOST_STORE`, use a separate absolute
`PLAIT_DESKTOP_STATE_DIR`. Reconnect does not erase local state or restart work.

For a prepared development checkout and a first source-change task, see
[Build Plait with Plait](SELF-HOSTING.md).

## Keyboard controls

| Shortcut | Action |
| --- | --- |
| Cmd-O | Open a local repository |
| Cmd-R | Refresh Git status and cached diffs |
| Cmd-1 … Cmd-9 | Jump to the Nth thread; Settings can change this to machines or projects |
| Cmd-Shift-1 / Cmd-Shift-2 | Conversation / Changes |
| Cmd-B | Show or hide the sidebar |
| Cmd-Shift-D | Show or hide render timing diagnostics |
| Cmd-comma | Open local host settings |
| Enter / Shift-Enter | Submit the draft / insert a line break |
| Cmd-Option-Up / Down | Previous / next thread |
| Option-Up / Down | Previous / next changed file |
| Tab / Shift-Tab | Move keyboard focus |
| Return / Space | Activate the focused control |
| Cmd-M / Cmd-W / Cmd-Q | Minimize / close / quit |

Scroll the conversation, navigation, file list, and diff with the trackpad or
mouse. Long diff lines scroll horizontally. On narrow windows, use Changes to
open Git; the optional conversation inspector is hidden.

## Independent updates

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

Client and host currently use protocol version **1**. The client rejects an
unsupported host protocol. There is no wire-schema change in this packaging
work. Future protocol or storage changes need an explicit compatibility and
migration review; arbitrary future versions are not promised to interoperate.
There is no automatic updater, automatic rollback, or release-directory eviction.
Do not delete an old release while its host or sidecar can still be running.

Program files:

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
