# macOS installation and independent updates

Plait is an early local preview for **Apple Silicon and macOS 15 or later**.
The client and host are separate programs. Closing or updating the client does
not stop accepted host work. Model requests still need provider connectivity;
a flight without working internet is not an offline agent environment.
The bundle declares macOS 15, but packaging CI uses macOS 26. Older-system
runtime acceptance is not yet complete.

## Packages

`bash scripts/build-macos.sh` produces:

| Artifact | Purpose |
| --- | --- |
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
does not test the downloaded-file approval flow on your Mac.

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
personal-machine validation. See [verification](VERIFICATION.md) for results.

Download the packages from a successful `plait-macos-arm64-…` workflow artifact.
Artifacts expire after 14 days. The workflow does not publish a GitHub release
or use signing secrets. [Issue #6](https://github.com/heath0xFF/plait/issues/6)
tracks Developer ID signing, notarization, and verified automatic update delivery.
