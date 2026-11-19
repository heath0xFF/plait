# Plait desktop design

## Direction
Plait is an original native development tool, not a port or a reskin. Design
from its own workflow: Project → Machine → Thread, independent execution,
clear approvals, and fast review of real changes. Use the Plait artwork and
follow the [originality policy](docs/ORIGINALITY.md).

Judge changes by task completion, clarity, keyboard use, accessibility, and
measured responsiveness. Keep controls useful and state easy to inspect.

[Thread tabs](docs/ROADMAP.md#thread-tabs) keep recent work available without
keeping hidden views loaded. Tab state belongs to the model; the native rows
present it with measured widths, words, and accessible names. Every New tab or
New thread action appends a fresh draft, even when the current one is empty.
Drafts keep independent text, images, choices, and pending creation. Closing a
nonempty draft preserves it for reopening; older closed drafts remain recoverable
when the twenty-entry view history expires. Hidden drafts draw no content, and
large overflow lists draw only visible rows. Settings and
Project settings open as window modals over the current workspace.

Worktrees is a second tab content kind, one per project per window. Its neutral
branch mark replaces the thread state dot, and its title is Worktrees. It has
no thread-view bar. The same close, reorder, overflow and restore behavior
applies; hidden content draws and reads nothing.


## Layout
Compact Project → Machine → Thread rail; conversation or diff workspace;
optional wide-window Git inspector. Keep machine and thread context visible.
Both side panels resize from their inner edge (Cmd-B and Cmd-L hide them); the
conversation keeps a readable minimum width.

- **Chrome.** A 40-point top tab bar serves as the window bar, with no system
  title bar above it. The native traffic lights sit at its left, centered on it
  and 14 points in (hidden in full screen), followed by the sidebar toggle,
  open tabs, an overflow summary when tabs exceed the bar, and `+`. Empty space
  at the right moves the window and handles title-bar double-clicks; no window
  title or permanent footer is drawn. Each tab shows a state dot, its title,
  a one-word state while active, running, needing attention, failed, stopped,
  or unread-done, and a close control on the active tab or on hover. Clicking
  the overflow summary opens a state-grouped list of all open tabs. Beneath
  the tab bar, a thread tab adds a 36-point workspace bar over the workspace:
  `project / machine · branch` (omitting machine on narrow windows), the
  Conversation / Changes / History / Files switch, and the Git inspector
  toggle. Tabs whose content has no sub-views omit this second bar.
- **Worktrees.** The main checkout's current branch forms a trunk, newest
  commits first. Worktrees occupy fixed lanes beside it. One-point neutral
  lines and small rings follow the conversation turn line; position separates
  lanes. Filled nodes are commits, hollow head nodes are uncommitted changes.
  State alone adds color. Short hashes, truncated subjects and age occupy
  virtualized 28-point rows. Head labels carry thread, state, branch, preservation
  facts and right-aligned size. Merged heads rejoin; unavailable or rewritten
  bases say Detached. Selection is a neutral fill. The detail sits beside the
  graph in wide windows and below it in narrow windows, with actions near its
  top. Its six-point storage bar shows the selected checkout's share of the
  largest, split into source and build output in two neutral tones. The project
  total sits above the graph. Last used and Size order lanes while retaining
  the main checkout first. Long graphs scroll in both directions. Unknown and
  bounded measurements say Measuring, Unavailable or over, never zero by default.
- **Rail.** No in-app logo: the app icon and window carry the brand. The rail
  sits below the 40-point tab bar and starts with two actions, New thread and
  Open repository. New thread names the model it will use; the control beside
  it opens the model picker. Projects are plain section headers, ordered by
  their newest thread activity. The local machine stays in the data model and
  workspace context; it does not add a repeated row under every project. Every
  tree row is 28 points and one line. Category glyphs use the Plait vector kit
  in `assets/branding`: mint projects, blue machines, violet threads, and light
  Git. Do not use a generic braid or a crop from the reference sheet. The
  Dock/Finder icon uses the kit's rounded tile. Host state sits in a one-line
  footer with Settings.
- **Thread rows.** A title leads, followed by a compact state word and relative
  activity time. Unread activity uses a heavier title and a dot, with a full
  accessible name. Selection is a neutral fill. A search field narrows projects
  and threads; Needs me and Archived are explicit filters. An actions control
  exposes rename and archive. Rename replaces the title with a text field.
  Long navigation lists are virtualized. Missing folders stay visible.
- **Real data only.** No sample threads or placeholder machines. With no
  thread open, the workspace says why (not connected, no threads yet, or
  nothing selected) and offers the one useful action.
- **Pills** are for metadata (machine, state), not for every label, so they
  stay meaningful.
- **Conversation.** A log of turns, not a chat. A turn is one request and
  everything up to the next request. A one-point turn line runs down the
  conversation's left margin, outside the text column, so text keeps the
  composer's edges. A request heads its turn with a ring on the line and a
  visible You label. Its text and images sit in a lightly raised neutral
  panel, left-aligned, with regular-weight body text. Quoted context has a
  one-point neutral rule and muted text inside that panel; the outer `>`
  marks are not drawn. Other request syntax stays literal. Replies carry a
  visible Plait label, including while streaming, and stay open on the
  conversation surface. Reasoning and tool calls stay compact beside the line.
  Each finished recorded run closes at a filled node, including one with no answer. Its record is drawn there: the
  step count and the files the host recorded changing, including
  command writes. Three file names and a count of the rest stay on one line;
  each name opens that exact record's before/after diff in Changes, blue and
  underlined, with added and removed lines in hint color. The step count and
  underlined N more open all files in the record; an empty run has a Changes
  link. Files remain reviewable after committing, later runs, or undo. A run includes its steering messages and closes only
  once, at its last message or tool; a queued follow-up is another run.
  Undo sits at the right end, always visible on the latest run and shown on
  hover or keyboard focus on earlier ones. One press applies it. Its line
  then says Undone, names the files put back, and offers Redo. A blocked undo
  opens a small panel at that line: Changed since this turn, the paths,
  Undo the rest, and Cancel. Excluded paths belong in hover text and that
  panel, never in a permanent extra line. Recording, Unavailable, Wait, and
  After restart name states briefly; hover text explains them. The line ends
  at one node, in the state's color, that names the latest turn's state and, once
  that turn has finished, its record. The state is said there once, not
  repeated in the answer above. While the rows on screen stop short of the
  latest turn (scrolled back from the newest row, or reading older history),
  the line stops short of that node. Give each row a full author/context label
  for assistive technology as well as its visible author. Copy controls share
  the author's header line and stay out of sight until the pointer is over
  their row or they take keyboard focus. Away from the newest row, one control
  at the bottom of the conversation jumps back to it.
- **Images.** Requests show small thumbnails below their text, aligned to its
  column. Activating one opens a larger view with Close; Escape returns to the
  conversation. The message box shows attached thumbnails with Remove before
  sending. Text only, Update host, Loading, Failed, Uploading, and Refused name
  states in a word or two; the reason is hover text. Failed originals block
  sending, and Uploading prevents duplicate sends. Pending messages name their
  image count even without text. A refusal leaves the draft intact and clears
  on the next edit. No attachment disappears silently, and no original is
  resized to fit a limit.
  A message allows up to 5 MiB of original image bytes, including a single
  full-size screenshot. PNG/JPEG, count/dimension limits, persistence, and clipboard exceptions
  follow the [host contract](docs/HOST.md#protocol-version-6)
  and [Mac guide](docs/MACOS.md#images-in-messages).
- **Conversation text.** All message text is selectable; the selection is a
  neutral fill. Links are blue and underlined, and their address appears at the
  bottom edge of the conversation before a click. Lists hang their wrapped
  lines from the text, not the marker. Tables size columns to their content with a
  quiet header row and scroll sideways when wide.
- **Find.** Cmd-F opens the Files view's find bar over the conversation: the
  field, a count ("3 of 17", "No matches"), previous, next, and close, under
  the window bar. Every match in view, here and in Files, is drawn on the
  selection's neutral fill; the current one is the same fill at twice the
  strength and underlined, so it does not depend on color. Stepping scrolls a match a third of the way down
  the list, or leaves the list alone when the match is in view. When some of
  the conversation is not loaded, the bar says "Loaded messages only", and
  hover text gives the reason.
- **History and reasoning.** Earlier pages load near the top of the conversation;
  a quiet line names loading, the beginning, or a failed read with Retry. Prepending
  preserves the current row and its scroll position. Home/Cmd-Up goes to the
  loaded beginning; End/Cmd-Down and Jump to latest return to the live end.
  Reasoning is a separate, muted disclosure, initially closed after a response
  finishes, with its chevron first, as on a tool call. Live reasoning opens in
  a bounded-height region. Both use the same
  text selection and copy behavior as answers. A turn's record holds only what
  the host reported: observed tool steps and recorded file changes. An unloaded
  request means the step count is unknown; a page ending before the live tail
  cannot close its last run. Older hosts retain tool-based summaries and offer
  no undo; hover text says a newer host is needed. Do not invent
  durations or identify who stopped a run when the host does not report that,
  and draw no control for an action that does not exist yet.
- **Run state.** One text line above the composer, on the turn line's last
  node, names waiting, thinking, tool execution, retry, completion, stop,
  interruption, or failure. Retries show the attempt and remaining seconds.
  The status and any recovery action align to the conversation column; raw
  protocol codes are not presentation.
- **Tool calls.** One compact line each: disclosure chevron, tool name, the
  path or command, a short fact such as `+2 −1`, and a state label only when
  the call did not simply finish. A long command wraps (up to four lines
  closed, in full when opened) rather than running past the column. Opened in place, `edit` shows a line diff,
  `write` the new file, `command` a `$` prompt line and its output, and `read`
  the first lines of the file. A failed call says "Failed" on its line, and
  its output is outlined in rose; the word is said once. A long run of calls folds under one line that names their
  count and kinds and any failures; a turn in progress keeps its latest calls
  in view.
- **Composer.** One raised, rounded box holds the draft and Send. Its
  placeholder names the two triggers, `@` for files and `/` for commands, so
  an idle composer carries no line of key hints. A line appears under the
  draft only when it has something to say: what the keys do during a run, a
  choice among suggestions, or why sending is off. Stop remains a separate
  neutral control during a run. Enter follows up; Option-Enter steers. The
  draft grows from one line to about a third of the window, then scrolls.
  Waiting and unconfirmed messages form a compact, scrollable stack above it,
  with explicit mode and Cancel or Retry controls. Below it is one line of
  wrapping controls: the harness's logo, model, the thinking level by name
  alone, context use, and the permissions control, which names the mode and
  nothing more ("Full access", "Ask before actions"); whether commands are
  sandboxed is the project's, in Project settings. A new thread's draft has the same line in the same place, with its
  project and harness. Labels name the setting; hover text says what a mode
  allows, what protects the checkout, when a change applies, the harness's
  version, and why a control is locked. A locked control stays readable, in
  hint color. Only one sentence joins the controls, in amber: the run is not
  under the mode the control names, because the harness used another or the
  mode was changed during the run. Model names truncate; actions remain
  legible. Project and machine stay in navigation and the header. The one
  filled button in a view is the one that can act; a button that cannot is an
  outline in hint text, and hover says what is missing.
- **Suggestions.** `@` lists files and `/` lists local commands in one compact
  list that floats just above the composer, at most eight 28-point rows and 560
  points wide. A file row shows the name first, then its directory. Matched
  characters are brighter and heavier, not colored; the selected row uses the
  neutral selection fill. Keyboard focus stays in the draft. A chosen file
  becomes its plain path in the text, so the message shows exactly what is sent.
- **Palette and picker.** One centered overlay, at most 620 points wide: a
  search field, up to twelve 28-point rows, and a footer of key hints. The
  workspace behind is dimmed, not blurred or shadowed. Selection is the neutral
  fill. Matched letters are heavier, not tinted. A row reads title, then a
  muted detail, then state in words, then its shortcut. Sections have plain
  headings before a search and disappear during one. The model picker adds a
  favorite star, outlined or filled, and Set default on every usable row; the
  current default carries a mint Default label instead.
- **A home per scope.** A setting lives where its scope is. A thread's own
  choices are under its message box, and its menus hold nothing wider. A
  project's settings are in the project's modal, opened from its sidebar menu
  (Project settings…). What applies to the whole app or host is in Settings.
  A control names its state in a word or two; what it means goes in hover
  text, not a permanent line.
- **Ways out.** Plait is not an editor or a terminal, so leaving for one is one
  action: Open in Editor (the file on screen, at its line), Show in Finder, and
  Open Terminal Here (the open checkout), in the menus, the palette, and as
  quiet Editor and Finder controls in the Files header and an Editor control
  among the Changes diff's actions. Nothing is added to the conversation.
  Hover says what will open, where, and with what. A failure is one sentence in
  the notice line, naming the command.
- **Settings.** Host status and Reconnect first, then New threads, Providers,
  Agents, Keyboard, Editor, Appearance, and About sections. New threads holds the defaults
  a new thread starts with: harness, model, thinking, and Pi's approvals.
  Agents lists each harness, its setup, and its default permissions. Editor
  holds what opens files and terminals: a ready choice, or a custom command
  whose field says beneath it why one cannot be used.
  Appearance lists light and dark themes side by side, each with a swatch of
  its background, text, and accents. The host status and section navigation stay visible while
  the active section scrolls. Each setting is a label and one line
  of meaning on the left and its control on the right. Provider forms occupy their section while open; keys are masked, with explicit reveal. Shortcut recording shows
  conflicts in place and lets the person cancel or reassign. A choice applies when
  made; its saved or failed state is a dot and a sentence beneath it. Settings
  opens in a centered window modal, with its own 44-point header and Done.
  Its size is `min(820, window width − 48)` by `min(720, window height − 64)`
  points, with 30-point horizontal padding. The window stays visible behind a
  neutral dimming layer, with its layout, selection, find, scroll and draft intact.
  Focus stays inside the modal; window shortcuts cannot change the covered
  workspace. Done, Escape, a click outside, or Cmd-comma dismiss application
  Settings and return focus to the previous control. Escape first closes an
  open picker, cancels shortcut recording, closes a provider form, or keeps
  the current command policy at the trust confirmation. Outside
  clicks do nothing during shortcut recording. A running thread keeps updating.
  Settings describes no one project, the open one included.
- **Project settings.** The same modal and content column as Settings, headed
  by the project's name with its checkout path beneath in hint color. Opening
  it from another project's menu leaves the active workspace selected. Its
  target stays fixed while a settings request is pending. Commands (sandboxed
  or trusted, for this checkout), then the Pi setup its threads are offered:
  skills, prompt templates, MCP servers and instructions. Trusted command access
  needs a consequence summary and a separate confirmation. Done, Escape and an
  outside click dismiss it; Cmd-comma opens application Settings instead.
- **Approval.** A bordered card above the composer shows the exact arguments,
  the risk, protected dirty paths, and Deny once / Allow once. The amber dot,
  title, and edge carry the caution; the risk is secondary text, read rather
  than signalled. Each button shows its shortcut from the keymap
  (Cmd-Option-period and Cmd-Option-Y by default). The shortcuts pass the
  buttons' own gate, act only while the card is on screen, and answer once
  per press.
- **Git.** Changed files are 28-point rows: a one-letter state, file name first,
  then its directory, then index/worktree. The accessible name carries the full
  path and state. Diffs keep separate Unstaged and Staged views. Stage or
  Unstage appears on a row on hover and in the diff header. The header reads
  in order: the file, the side being shown, then what can be done to that
  side. It is one 44-point line where there is room; in a narrow window the
  actions wrap beneath, so no control runs off the edge. The commit box
  sits under the file list with one message line, Commit, and the last result
  in words. History lists commits (subject, short ID, author, age) beside the
  selected commit's message and patch. A diff starts at the change: header
  lines that only repeat the path are not drawn, and in a commit's patch each
  file begins under a quiet heading row, name first and then its directory.
  Header lines that say something else (mode, rename, similarity, binary)
  stay. Hunk headers sit on a neutral fill with their ranges in hint color and
  Git's context, usually the function, in secondary text; Stage hunk and
  Discard hunk appear on hover. The branch name beside its icon, at the top
  of Changes and of the inspector, is a control with a small chevron that
  opens the palette over local branches: newest first, the current one
  marked Current and not choosable, and a first row "Create branch <name>"
  when the typed text is not a branch. While a branch write would be refused
  (another Git write, or a thread running in this checkout) the name stays
  readable in hint color, hover says why, and the palette names the reason
  with every row unavailable.

- **Recorded Changes.** A 28-point scope row below the branch names All changes
  or Turn · followed by the start of the request, shortened at a word boundary
  where possible. Without loaded request text it gives the age, such as
  Turn · 5m ago. Its picker offers the current thread's retained records as
  Original or Restored, newest first; descriptions give the request, age and
  thread. Multiple undos of a run differ by age and order. It does not imply
  an all-thread catalogue. Hover and accessible text keep the full request,
  age and thread; record IDs are never shown.
  Recorded lists every saved path, including deleted and now-clean files, with
  recorded counts and current layer availability in accessible hover text.
  Recorded/Current appears for a selected recorded file and says which comparison
  is visible. Recorded uses the immutable before/after bytes and has no file or
  hunk write buttons. Current selects a Git side with changes, shows Reading diff…
  while status loads, and says No current changes only for a known clean file.
  Switching files,
  later runs and undo never silently replace the chosen record. Loading,
  Unavailable, Offline and Older host name the read's state; details and Retry
  or Current explain the way forward. There are no origin labels, turn staging
  or review-baseline controls yet. Opening a record never marks it reviewed.

## Tokens
Colors are roles in one palette (`plait_core::theme::Palette`), so every theme
keeps the same meaning. The default dark theme is Stillwater:
- Background: #1f222e. Navigation: #242835. Panel: #2b303f.
- Border: #313547. Control outline: #464d66. Selected row: #2e3243.
- Main text: #dfe3ed. Secondary text: #b0b7cb. Labels and hints: #9da5bf.
- Mint #70d689: local/read-only state, additions, the chosen option.
- Frost #74cde2 (the blue role): running work, modifications, links,
  keyboard focus.
- Orchid #ec8ac7 (the violet role): thread identity and keywords.
- Sand #eec96e: attention/limits. Coral #ec7368: errors/deletions.

Main text is about 12:1 against the background: readable for long sessions
without the glare of near-white on near-black. Plait Dark is a neutral
graphite alternative at the same contrast (#d9dce3 on #1c1d20). Plait Light uses a warm
off-white background (#f7f7f5) with #2b2e33 text, white raised panels, and
darker accents. Stillwater is mapped by hand from its own palette. Base16
themes are mapped onto the same roles: base00 behind
base05 text, accents from base08–base0E, and grays mixed from background toward
text. A contrast floor keeps body text at 7:1, secondary text at 4.5:1, and
hints and accents at 3:1 or better whatever the scheme.

Accents carry state, not decoration: selection and hover are neutral. The one
affirmative action in a view (Send, Create, Save) is main text with the
background as its label.

Code uses the same palette for syntax, and nowhere else is color read this way:
violet keywords, mint strings, amber numbers and constants, blue types, keys,
and attributes, secondary-text punctuation, and hint-colored comments. Function
names and plain identifiers stay main text. Diff lines keep the Git meaning:
a `+` or `−` gutter mark with a faint mint or rose line tint. Selected
text is main text at low opacity, so it reads the same on every surface. Inline
code is main text on the panel color.

System sans-serif at 11–15 points, with 13 points for body and navigation.
Requests and replies both use 13-point regular text with 21-point line height;
author labels use 12-point medium text. Headings inside a reply are 15, 14,
and 13 points bold. Code, file paths, and commands use the system's monospaced
font (SF Mono) at 12 points, or an installed monospaced family named in Settings →
Appearance; a family that is not installed, or whose characters differ in
width, is not drawn, so code keeps one character width. Conversation content,
the composer, and settings modal content are capped at 760 points; the turn line, in
the control outline color, hangs 12 points left of that column, in the middle
of the margin.
Spacing follows a 4-point base. Preserve gradients inside the supplied brand artwork
only. Avoid new chrome gradients, shadows, animation, and decorative dashboard
cards.

## Behavior
Native traffic lights (over the app's own window bar), menus, and folder picker. Keyboard focus
must remain visible: a blue ring on controls and a blue edge on the composer.
Selection is not color-only. Long lists are virtualized.
Do not draw an enabled composer or a send control until an agent can run.
Render timings are a diagnostic, shown by View → Toggle Diagnostics, not
permanent chrome.
