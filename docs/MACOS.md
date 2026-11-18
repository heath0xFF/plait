# macOS installation and independent updates

Plait is an early local preview for **Apple Silicon and macOS 15 or later**.
The client and host are separate programs. Closing or updating the client does
not stop accepted host work. Model requests require access to the configured
provider; installing Plait does not include a model or offline inference engine.
The bundle declares macOS 15, but packaging CI uses macOS 26. Older-system
runtime acceptance is not yet complete.

## Download and install

Published previews live on the [GitHub releases page](https://github.com/heath0xFF/plait/releases).
Download the **DMG**, open it, then run **Install Plait.command**. It installs
the prebuilt app and bundled host in the per-user layout used by in-app updates.
No source checkout, Xcode, Rust, or separately installed Node is required.

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

Run it as your normal user, not under `sudo`. Only migrating an older
system-wide installation requires administrator access, once. The flag accepts
the unsigned preview; the script does not change Gatekeeper or remove quarantine
attributes. Finish or Stop active tasks before installing. Threads, drafts, and
credentials are preserved; the app reopens after installation.

The script uses the newest published release, including previews.
`--version v0.1.0-preview.7` pins a release. `--download ~/Downloads` verifies and
saves the app archive without installing it. Initial downloads are checked
against the release's SHA-256 metadata over HTTPS; this detects corruption, not
publisher identity. Later in-app updates verify signed release manifests.

`--component all`, `client`, and `host` retain the older system-wide package
installation paths for manual component management. Those packages do not
create an installation eligible for in-app updates.

## Packages

`bash scripts/build-macos.sh` produces:

| Artifact | Purpose |
| --- | --- |
| `Plait-…-macos-arm64.dmg` | Prebuilt app archive, Install Plait.command, and instructions |
| `Plait-Installer-…-macos-arm64.pkg` | Install both components; Customize can select either one |
| `Plait-Client-…-macos-arm64.pkg` | Install or update only `/Applications/Plait.app` |
| `Plait-Host-…-macos-arm64.pkg` | Install or stage only the host, Node, and pinned runtime |
| `Plait-…-macos-arm64.zip` | App bundle with its host archive and prebuilt-install script |
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
   against the same store. The installer refuses while a managed run is active or awaiting approval.
2. Open **Install Plait.command** from the DMG. It closes the client, installs
   the app and host, and reopens Plait. An older system-wide install needs an
   administrator password once; subsequent updates do not.
3. Open **Plait** from Applications. Use **Open repository…** (Cmd-O), then
   **Settings** (Cmd-comma) to check the local host and choose an authenticated
   model. Reconnect if the client opened before the host was ready.
4. Start a real thread. Settings → New threads selects automatic approval or
   asking for each action. Whether a project's commands are sandboxed or
   trusted is that project's own choice, in its settings (the project's **…**
   menu in the sidebar → **Project settings…**). A Codex or Claude Code thread
   uses that harness's own permissions instead, chosen with **Permissions**
   under a new or open thread's message box, with a default for each harness
   in Settings → Agents. The app shows only real threads.

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
LaunchAgent. Do not put credentials into the LaunchAgent property list or
the package.

Locate the installed host with native macOS tools:

```sh
build_id=$(/usr/bin/plutil -extract build_id raw -o - \
  "$HOME/Library/Application Support/Plait/Host/installed.json")
host="$HOME/Library/Application Support/Plait/Host/releases/$build_id"
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

**Commands** in a project's settings changes that checkout between Sandboxed
and Trusted. Open the page from the project's **…** menu in the sidebar
(**Project settings…**), or with **File → Project Settings…** for the active
project; a project that is not active becomes the active one first. The page
also lists the Pi skills, prompt templates, and instructions its threads are
offered. Trusted requires confirmation of full user access, network,
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
switches its conversation, draft, and Git workspace together. The **+** on a
project row, shown on hover or keyboard focus, opens a new thread there.
A thread that runs in a harness other than Pi shows that harness's logo beside
its title; under an open thread's message box, the logo names its harness, with
the version on hover. The local machine
appears in workspace context without a redundant row beneath every project.
Missing folders remain visible as **Missing**; their saved conversations can still
be read.

Use the sidebar search to filter titles and project names. **Needs me** includes
unread activity, pending approvals, failures, and interrupted runs. **Archived**
includes archived conversations. Thread actions and the **Thread** menu offer
rename and archive/unarchive. Archiving keeps the conversation and does not stop
an active run. Removing a project from the sidebar keeps its host threads;
the eye button (**Show hidden projects**) beside the filters, or a project entry
in the command palette, brings it back.

The host keeps its newest 200 threads live. When more are loaded, the sidebar
states the limit; use the refresh button (**Refresh threads and folders**) for older threads and
folder availability. A disconnected thread-list watch reports that updates are
paused. Hosts using protocol 1 or 2 retain their one-checkout thread listing and
journal titles; rename and archive require version 3.

The bell button beside the filters (struck through when off) controls system notifications and window attention requests.
Unread markers remain when alerts are off. Alerts occur for a thread you are not
viewing, or while Plait is inactive, when it finishes, fails, stops, is interrupted,
or needs approval. Opening a notification selects its thread. A bundled Mac app
requests notification authorization on first delivery; an unbundled build cannot
post macOS notifications, and delivery depends on system permission and Focus
settings. The initial listing never posts alerts. Native delivery and activation
remain manual acceptance checks.

### Codex and Claude Code threads

Source builds from `main` with a matching host can run a thread in Codex or
Claude Code instead of Pi. Neither is in a release yet. Each runs your own
install with its own sign-in, settings, instructions, and tools; Plait never
sees their credentials. **Settings → Agents** lists each harness with its state,
version, path, and what protects your checkout. **Choose…** runs a particular
executable, and **Use PATH** returns to the one your login shell finds.
**Refresh** checks again after you install, update, or sign in.

**Setting up Claude Code threads.** Install Claude Code and sign in with
`claude` in a terminal as usual. A Claude subscription, an API key, or a cloud
provider all work. Plait drives `claude` through Anthropic's Agent SDK, which
Plait's downloads do not include, so the Claude Code entry first reads **Needs
setup**. Choose **Set up**. The host downloads that one pinned SDK version from
the npm registry, checks it against a pinned checksum before writing anything,
and keeps it beside its own data. This is the only step that reaches the
network for a harness, and it reads none of Claude Code's files. While it runs
the button reads **Setting up…**. If it fails, the entry stays in **Needs
setup** and says why; choose **Set up** again. Afterwards the entry reads like
Codex's: ready, or what to fix (not installed, too old, or signed out).

**Permissions.** These threads use the harness's own permission modes in place
of Plait's Ask and Auto and the project's command policy. Claude Code offers
Ask, Accept edits, Plan, Auto (the default), and Bypass. Codex offers Read only,
Ask, Approve for me, and Full access (the default). Set each harness's default
for new threads in Settings → Agents, or choose **Permissions** under the
message box, in a new thread or an open one. The control names the mode; hold
the pointer over it to read what the mode allows and what protects the
checkout. In Claude Code a change applies at once, even during a run; in Codex
it applies from the next turn. If the harness's own settings run a different
mode, an amber line under the controls says so. Each request the harness asks you about is an approval card, allowed or
denied once. Plait's guard on files you had already changed does not apply
here: the card names those files, but nothing stops the harness writing them.

**Default harness for new threads** in Settings → New threads chooses where Cmd-N
starts. A harness that is not ready is shown dimmed and cannot be chosen; a
draft whose default is not ready starts in Pi. What has been checked against
real installs is in [Verification](VERIFICATION.md#claude-code).

## Appearance and themes

**Settings → Appearance → Light or dark** follows macOS between light and dark by
default, or keeps Light or Dark. Choose a theme for each. The dark list starts
with **Stillwater**, the default: a quiet indigo-slate theme mapped by hand.
**Plait Dark** follows it, a neutral graphite alternative. The light list starts
with **Plait Light**, the default. Both lists continue with shipped Base16 schemes such as Catppuccin, Gruvbox, Nord, Rosé Pine,
Solarized, and Tokyo Night. A choice applies at once and is saved in the
desktop preferences.

**Code font** sets the font for code, file paths, and commands. Left empty, it
is the system's monospaced font, SF Mono. Type the name of an installed
monospaced font family, such as `JetBrains Mono`; matching installed names are
offered as you type, and the font changes as soon as the name matches one. A
name that is not installed, or a font whose characters differ in width, changes
nothing, and the line beneath the field says so. Fonts installed while Plait is
running are found the next time Settings opens.

More themes come from Base16 scheme files: put `.yaml` files (for example from
[tinted-theming/schemes](https://github.com/tinted-theming/schemes)) in
`~/Library/Application Support/Plait/themes/`, then choose **Reload**. **Open
themes folder** creates and shows it. Each file appears in the light or dark list
by its own `variant`; a file that cannot be read is named beneath the lists with
the reason. Plait keeps every theme's text readable: if a scheme's text, grays,
or accents are too faint against its background, they are moved toward its text
color until they meet a minimum contrast, so a few schemes look slightly crisper
in Plait than elsewhere. A theme removed from the folder falls back to Plait's
own.

## Keyboard controls

When focus is in the conversation, Home or Cmd-Up goes to the beginning of the
loaded history and requests an earlier page when one is available. End or Cmd-Down
returns to the latest messages. Text fields keep their usual editing shortcuts.
Version 3 history also loads as you scroll toward the top; use **Load earlier**
for an explicit page request and **Retry** after a failed read. Older hosts retain
bounded previews. Completed reasoning expands from its **Reasoning** line.

**Find** (Cmd-F) over the conversation opens the same bar as in Files: it
highlights every match in the requests and replies that are loaded and counts
them ("3 of 17", or "No matches"). Enter or Cmd-G moves to the next match,
Shift-Enter or Cmd-Shift-G to the previous, wrapping at the ends, and the list
scrolls to each one. Escape closes the bar and puts focus back where it was. A
new search starts at the first match from the top of the view, or at the
newest when the conversation is at its end. While the bar is open, a reply
that arrives is searched too, without moving the list. Find ignores letter
case, including outside ASCII, and matches plain text, not patterns. It counts
up to 1,000 matches, keeping the newest. It searches the text as drawn, not
the Markdown behind it, and does not search the reply still being written,
reasoning, tool calls and their output, or a turn's closing line. Only loaded
messages are searched; when earlier messages, or the rest of a long one, are
not loaded, the bar says **Loaded messages only**, with the reason on hover.
Find does not load pages to search them.


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
| Cmd-Option-N | New Thread in Worktree | `thread.new_worktree` |
| Cmd-Shift-M | Change Thread Model… | `thread.switch_model` |
| Cmd-O | Open Repository… | `repository.open` |
| Cmd-P | Open File… | `file.open` |
| Cmd-R | Refresh Git Status | `git.refresh` |
| Cmd-Option-E | Open in Editor | `file.open_in_editor` |
| Cmd-Option-T | Open Terminal Here | `project.open_terminal` |
| Cmd-W | Close Window | `window.close` |
| Cmd-K | Command Palette… | `palette.toggle` |
| Cmd-B | Toggle Sidebar | `view.toggle_sidebar` |
| Cmd-L | Toggle Git Inspector | `view.toggle_inspector` |
| Cmd-Shift-D | Toggle Diagnostics | `view.toggle_diagnostics` |
| Cmd-Shift-1 | Conversation | `view.conversation` |
| Cmd-Shift-2 | Changes | `view.changes` |
| Cmd-Shift-3 | History | `view.history` |
| Cmd-F | Find | `file.find` |
| Cmd-G | Find Next | `file.find_next` |
| Cmd-Shift-G | Find Previous | `file.find_previous` |
| Cmd-Option-Down | Next Thread | `thread.next` |
| Cmd-Option-Up | Previous Thread | `thread.previous` |
| Option-Down | Next Changed File | `git.next_file` |
| Option-Up | Previous Changed File | `git.previous_file` |
| Cmd-Option-period | Deny Once | `approval.deny_once` |
| Cmd-Option-Y | Allow Once | `approval.allow_once` |
| Cmd-Shift-C | Commit Staged Changes… | `git.commit` |
| Cmd-Shift-B | Switch Branch… | `git.switch_branch` |
| Cmd-M | Minimize | `window.minimize` |
| Cmd-1 to Cmd-9 | Jump to Sidebar Item 1–9 | `jump.1` to `jump.9` |
| Tab | Focus Next Control | `focus.next` |
| Shift-Tab | Focus Previous Control | `focus.previous` |

Cmd-N opens a new thread in the active project with the cursor in its message
box. Nothing is created on the host until you send the first message; the
thread is then created and the message submitted to it. If creation fails, the
text stays with the reason, and an unconfirmed creation is retried with the
same request ID, so it cannot make a second thread. Above the message box,
choose the **project** (a filterable list of sidebar projects, most recently
active first, and **Open repository…**), the **model** (only the chosen
harness's models), the **harness** (Pi, and each harness the host lists; one
that is not ready is listed with its state), and **Permissions** (Pi: Ask or
Auto, defaulting to Settings → New threads; another harness: its own modes,
defaulting to its Settings → Agents default). Changing the harness returns the
model and permissions to that harness's defaults: Pi's saved default model, or
the harness's most recently used model, else its first. Switching project keeps
the new thread open and shows that repository in the inspector. While it is
open, a **New thread** row marks where it will land in the sidebar. Opening
another thread sets it aside with its text, which Cmd-N brings back; Escape or
**Discard** clears it. The text is kept while Plait is open, not across a
restart. Cmd-Shift-N opens it with the model list open. A thread with no title
yet reads **New thread**. Cmd-Option-N still creates a worktree thread at once,
since its worktree is made for it. Cmd-1 to Cmd-9
count threads in sidebar order; Settings can change this to machines or
projects. Cmd-B hides or shows the sidebar on the left; Cmd-L hides or shows the
Git inspector on the right, and that choice is remembered. On a window too narrow
for the inspector it explains where Git is instead. Drag either panel's inner
edge to resize it; the width is remembered, and double-clicking the edge restores
the default. Both shortcuts can be changed in Settings → Keyboard.

The window has one bar at the top instead of a separate title bar. The close,
minimize, and full-screen buttons sit at the top of the sidebar, and the header
runs beside them. Drag an empty part of the sidebar's top strip or the header to
move the window; double-clicking there does what **System Settings → Desktop &
Dock → Double-click a window's title bar to** is set to. With the sidebar
hidden, the header moves right to clear the buttons, except in full screen,
where macOS hides them.

Cmd-Option-period answers the open thread's approval card with **Deny once**
and Cmd-Option-Y with **Allow once**. Each button shows its shortcut as the
keymap binds it. The keys act only when the card is on screen and its buttons
would: in the open thread's conversation, with no palette or picker open over
it, the host connected, no other control request in flight, the approval not
yet answered, and its exact arguments shown. Otherwise they do nothing. A held
key answers one approval at most; release it before answering the next. Both
are in the **Thread** menu and the command palette, and can be changed in
Settings → Keyboard.

Above the message box, a card lists the files the thread wrote that are not
committed yet, with lines added and removed against HEAD. It updates as the
thread's edits finish and as Git status refreshes. Click its header to fold it,
or a file to see that file in Changes. Settings → Appearance, or **Toggle Changed
Files** in the View menu, hides it. Files changed only by commands are not
listed, and the card needs a host from this release or later.

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
supported levels. Both are locked during a run: they stay readable, and hover
says when they can be changed. In a Pi thread, changing the approval mode
during a run applies from your next message; until then an amber line says
which mode the run is under. An empty message box names its two triggers, `@`
for files and `/` for commands; a line of key hints appears under the draft
only during a run or when sending is off. New threads start at **Settings → New threads → Default
thinking for new threads** (Medium unless changed), or the closest level their
model offers; a model without reasoning starts with thinking off.

The run-settings menu beside them chooses this thread's approval mode: **Use
default** follows Settings → New threads, or pick **Ask before actions** or
**Auto-approve actions** for this thread alone. The choice is saved with the
thread's draft and applies from its next message; it does not change an active
run or previously queued messages. The button and the menu name only this
thread's approval mode. Whether commands are sandboxed is shared by every
thread in the checkout and is set in the project's settings.

Context usage shows the latest response, not cumulative cost. Hover or activate
it for exact token counts. At 75% it says high; at 90% it says nearly full. It
stays hidden when context usage or the window size is unknown. Older hosts keep
their existing send-when-idle behavior and have no per-thread model controls.

Providers send replies in uneven bursts. Rather than drawing each burst as it
lands, the conversation shows streamed text and thinking flowing in at the
display's own frame rate, trailing what has arrived by about a tenth of a
second; a larger burst drains faster. It redraws every display frame only while
text is catching up, and stops when it has. Opening a thread shows what has
already arrived at once.

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

### Staging, commits, and history

In **Changes**, hover a file and choose **Stage** or **Unstage**, or use **Stage
file** in the diff header. To take part of a file, hover a hunk's `@@` line and
choose **Stage hunk** (or **Unstage hunk** in the Staged view). If the file
changed since its diff was shown, Plait refuses, rereads it, and asks you to
try again. New, renamed, and conflicted files, and files that pass through a
Git filter such as LFS, stage whole.

**Discard…** in the diff header (Unstaged view) throws away a file's unstaged
changes and returns it to its staged or committed version; staged work is
kept. For an untracked file it reads **Delete…**. Hover a hunk for **Discard
hunk…** to drop just that part. Each asks first in a macOS alert. Before
discarding, Plait saves the file's exact bytes in the repository's object
store, and **Undo** beside the result puts them back until your next Git
action. Conflicted files cannot be discarded here. **Stage all** and **Unstage all** sit above the commit
message. Type a one-line message and press Return or **Commit** (Cmd-Shift-C
moves to the message). Only staged changes are committed. Plait runs your
repository's own Git configuration for these: clean filters such as LFS run when
staging, and commit hooks run when committing; a failing hook's output is shown
and nothing is committed. One write runs at a time. If a thread is running in
the same checkout, Changes says so, since its edits may be unfinished. During a
merge, an unmerged file offers **Mark resolved** instead of **Stage**; Commit
stays unavailable until every conflict is marked resolved. If a hook or filter
takes longer than its limit (60 seconds for staging, 120 for a commit), Plait
stops it and says so.

To ask a thread about specific lines, click a line number in the diff, and
Shift-click another to extend the pick. **Ask about this** puts a reference such
as `src/main.rs:10-12` and the picked lines, as a fenced diff, at the end of the
selected thread's message and opens the conversation. Nothing is sent until you
send it. **Clear** drops the pick; choosing another file or side ignores it.

Above Stage all, Changes shows how the branch stands against its upstream (for
example "2 to push") as of the last fetch, with **Fetch** and **Push**. A branch
with no upstream offers **Publish** instead, which pushes it to `origin` (or
`remote.pushDefault`, or the only remote) and tracks it there. When the branch
is only behind, the button reads **Pull**: Plait fetches and moves the branch
forward, never merging or rebasing, and refuses if that would overwrite
uncommitted changes. When both sides have new commits, Push and Pull are
unavailable; merge or rebase in Terminal. Sign-in uses your existing Git setup: SSH keys, credential
helpers, and, for HTTPS GitHub remotes, `gh auth login` if the GitHub CLI is
installed. Plait never stores or reads the token. Push, Pull, and Fetch are
also in the **Git** menu.

The branch name at the top of Changes, and of the Git inspector, opens the
branch list (**Switch Branch…**, Cmd-Shift-B, in the **Git** menu). It lists the
checkout's local branches, most recently committed first, with the current one
marked; a branch checked out in another worktree says which. Choose one to
switch to it. Type a name that is not a branch and the first row reads
**Create branch** *name*: it makes that branch at the current commit and
switches to it. While what you type is part of a listed name, Enter takes the
best match; **New Branch…** in the Git menu always starts on Create. Uncommitted
changes come along as Git carries them. If switching would overwrite them, Git
refuses and its message is the result line under the commit box; commit,
stash, or discard them and try again. Git checks a new name first (no spaces,
no `..`, no leading dash), and an existing name is refused rather than reused.
The list shows up to 500 branches and says so when there are more. The branch
name is locked, in hint color, while another Git action runs and while a thread
is running in this checkout, since switching would change the files it is
editing; hover says why. Plait acts on the checkout that is open, which may be
a thread's worktree. There is no force, no remote-tracking checkout, and no
branch deletion or renaming.

**History** (Cmd-Shift-3) lists commits on the current branch, newest first, 100
at a time. Select one to see its message and patch against its first parent.
There is no merge, rebase, or amend yet; use your own Git tools for those.

### Files

**Files** shows one checkout file, read-only, with line numbers and the same
syntax colors as the conversation. Open one with **Open File…** (Cmd-P), which
searches the checkout's files by name and folder, or with **Open** in a Changes
diff header. **Find** (Cmd-F) highlights every match and counts them;
Enter or Cmd-G moves to the next, Shift-Enter or Cmd-Shift-G to the previous,
wrapping at the ends, and Escape closes it; opening it again searches the same
text again. As in the conversation, Find ignores letter case, including
outside ASCII, and it counts up to 1,000 matches.

Plait rereads an open file when it may have changed: after a Git status read,
when the window comes forward, and when you return to Files. It compares size
and modification time first, so an unchanged file is not reread, and it never
polls. A file changed on disk is redrawn with **Updated from disk**; one that was
deleted keeps its last version on screen with a note. Binary files, files over
8 MB, and text that is not UTF-8 are described instead of drawn. Files open
inside the checkout only, and never through a symbolic link. There is no editing.

### Editor, Finder, and terminal

Plait hands off to the tools you already use. **Open in Editor**
(Cmd-Option-E) opens the file on screen at its line: in Files, the current find
match, or else the first line on screen; in Changes, the selected file at the
line you picked, or else at its diff's first change. **Show in Finder** reveals
the same file, and **Open Terminal Here** (Cmd-Option-T) opens the open
project's checkout. All three are in the File menu and the command palette;
the Files header has **Editor** and **Finder**, and a Changes diff header has
**Editor**. Hover over a control to see what it will open, at which line, and
with what. A file deleted from the checkout has nothing to open.

**Settings → Editor** chooses what runs:

- **Open files with**: **System default** (the app macOS uses for the file's
  type, without a line), **Neovim in Rex** (`nvim +line -- file` in a new Rex
  tab at the checkout), or **Custom**.
- **Open terminals with**: **Terminal**, **Rex tab** (a Rex tab at the
  checkout), or **Custom**.

A custom command is one line, such as `zed {file}:{line}` or
`/opt/homebrew/bin/wezterm start --cwd {project}`. `{file}` is the file's full
path, `{line}` its line, and `{project}` the checkout's path; a placeholder can
sit inside a word. Single quotes, double quotes, and backslashes group and
escape words as in a shell, but the command is not run through a shell, so `$`,
`~`, globs, and backticks are plain text. A file command needs `{file}`; a
terminal command can use only `{project}`. A command that cannot be used is not
saved, and the line beneath the field says why. Plait does not read your
shell's `PATH`, so name a program outside `/usr/bin` and `/bin` by its full
path. The choices are kept in `desktop.json`.

The Rex choices talk to Rex through AppleScript, addressed by its bundle ID, so
Rex and Rex Beta both answer. The first time, macOS asks whether Plait may
control Rex; the answer can be changed in System Settings → Privacy & Security →
Automation, and until it is allowed, using a Rex choice says so. Plait's builds
are signed ad hoc, so macOS may ask again after an update. Plait does not need
Rex: without it the choice stays, and using it says
that Rex is not installed. Plait starts the command and does not wait for it or
read its output. Only a command that cannot start, or that exits with an error
within two seconds, is reported, in one sentence above the workspace.

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
Installer packages is root-owned. Run **Install Plait.command** from the
latest DMG, or use the command-line installer above, to migrate once. It asks
for administrator access only for that migration; no source build is needed. Release builds compare versions; a source build is offered the
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
it. Commands under the default [trusted command mode](HOST.md#command-policy)
run as you and can write there; a project set to sandboxed commands cannot.

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
nothing. The job's only secret is the update signing key, which signs
`update.json` for in-app updates (see [Releasing](RELEASING.md)); the app and
packages are not Developer ID signed. [Issue #6](https://github.com/heath0xFF/plait/issues/6)
tracks Developer ID signing and notarization.
