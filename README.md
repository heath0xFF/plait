# Plait

**A native macOS app for working with AI coding agents.**

Open a repository, start a conversation, approve edits and commands, and review
what changed. An independent host runs the agent, so closing the desktop does
not stop accepted work. Reopen it to return to the same thread and saved draft.

[Download preview](https://github.com/heath0xFF/plait/releases/tag/v0.1.0-preview.1) · [Documentation](docs/README.md) · [Contribute](CONTRIBUTING.md)

![Plait showing a completed coding task, native code block, tool results, and saved draft](https://github.com/heath0xFF/plait/releases/download/v0.1.0-preview.1/installed-client-coding.png)

*Installed Mac app. Scripted provider; real approved source edits and Cargo tests.*

## Install

The preview targets **Apple Silicon and macOS 15 or later**. You need Git and
your own model-provider account. The installer includes the host and Node runtime.

**This preview is unsigned and not notarized.** Review the
[Mac setup guide](docs/MACOS.md) before installing. Do not disable Gatekeeper.

[Download the DMG](https://github.com/heath0xFF/plait/releases/download/v0.1.0-preview.1/Plait-0.1.0-de4f1fe115d5-macos-arm64.dmg),
open it, and run **Install Plait.pkg**. Or install from Terminal:

```sh
curl --proto '=https' -fsSL \
  https://github.com/heath0xFF/plait/releases/download/v0.1.0-preview.1/install.sh |
  bash -s -- --version v0.1.0-preview.1 --allow-unsigned
```

Run as your normal user. The script requests administrator access for package
installation and verifies SHA-256 checksums, not publisher signatures.
See [independent updates](docs/MACOS.md#independent-updates) before replacing an
existing installation; client updates do not restart the host.

## Start working

1. Open **Plait** and choose **Open repository…** (Cmd-O).
2. Open **Settings** (Cmd-comma) and select an authenticated model.
   [Set up Pi authentication](docs/MACOS.md#authentication-and-status) if needed.
3. Start a thread. Review each edit and command before **Allow once**.
4. Read the results, inspect **Changes**, and continue in the same conversation.
   Use **Stop** to cancel work; closing the desktop is not Stop.

Code blocks, tool details, and message text have copy controls. Threads and drafts
restore on reopening; no task is sent automatically. See
[keyboard controls](docs/MACOS.md#keyboard-controls) and
[Build Plait with Plait](docs/SELF-HOSTING.md) for the edit → test → review loop.

## Current limits

This is the **M2 local preview**, not a complete IDE.

- Local macOS execution only. Remote hosts and Linux execution are not available.
- Git views are read-only. Use your own tools for staging and commits. There is
  no file editor, embedded terminal, mandatory worktree, or automatic updater.
- Commands cannot use external networking or write the checkout. Approved file
  tools make edits; build output goes to private caches. Model requests need
  access to your configured provider.
- Conversation and tool views show bounded previews, not the full stored history.

Mac CI verifies the installed app, sandbox, and recovery with scripted providers.
Installed live-model and accessibility acceptance remain open. CI runs on macOS 26;
older-system runtime coverage and other open checks are in
[Verification](docs/VERIFICATION.md). Planned features are in the [roadmap](docs/ROADMAP.md).

## Development

The desktop is **Rust + GPUI**, the host is **Rust**, and the agent runtime is a
**Pi Durable TypeScript sidecar**. Application code and design are original;
declared dependencies stay pinned and retain their licenses.

Start with [Contributing](CONTRIBUTING.md) for build requirements and checks.
[Architecture](docs/ARCHITECTURE.md), [host and sandbox rules](docs/HOST.md), and
[design principles](DESIGN.md) describe the implementation boundaries.

## License

Plait's original code and documentation are [MIT licensed](LICENSE).
Dependencies retain their own licenses, including GPUI's Apache-2.0 license.
