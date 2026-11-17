# Plait desktop design

## Direction
Plait is an original native development tool, not a port or a reskin. Design
from its own workflow: Project → Machine → Thread, independent execution,
clear approvals, and fast review of real changes. Use the Plait artwork and
follow the [originality policy](docs/ORIGINALITY.md).

Judge changes by task completion, clarity, keyboard use, accessibility, and
measured responsiveness. Keep controls useful and state easy to inspect.

## Layout
Compact Project → Machine → Thread rail; conversation or diff workspace;
optional wide-window Git inspector. Keep machine and thread context visible.
Both side panels resize from their inner edge (Cmd-B and Cmd-L hide them); the
conversation keeps a readable minimum width.

- **Chrome.** One 44-point window bar, with no system title bar above it. The
  native traffic lights sit at its left, centered on it and 14 points in, over
  the rail's top strip. The rest is the header: sidebar toggle, thread title, a
  muted `project / machine` on wide windows, state dot with a text label, and the
  Conversation / Changes switch, then the Git inspector toggle, mirroring the
  sidebar toggle at the other end. With the rail hidden, the header starts clear
  of the lights; in full screen, where macOS hides them, it starts at the edge.
  The bar's empty parts move the window, and double-clicking them does what
  macOS is set to do for a title bar. No window title is drawn, no second
  header, tab strip, or permanent footer.
- **Rail.** No in-app logo: the app icon and window carry the brand. The rail
  starts with an empty 44-point strip under the traffic lights, then two actions, New thread and Open repository. New thread names the
  model it will use; the control beside it opens the model picker. Projects are plain
  section headers, ordered by their newest thread activity. The local machine
  stays in the data model and workspace context; it does not add a repeated row
  under every project. Every tree row is 28 points and one line. Category glyphs use the Plait
  vector kit in `assets/branding`: mint projects, blue machines, violet
  threads, and light Git. Do not use a generic braid or a crop from the
  reference sheet. The Dock/Finder icon uses the kit's rounded tile. Host state
  sits in a one-line footer with Settings.
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
  composer's edges. A request heads its turn: left-aligned, semibold, and a
  size above the replies, with a ring on the line. Replies, reasoning, and
  tool calls are open text beside the line; headings inside a reply stay
  smaller than or equal to the request. Every earlier turn that ended in an
  answer closes at a filled node. One that called tools carries its record
  there: the step count and the files its edits changed. Each file opens in
  Changes and is drawn as a link, blue and underlined. The line ends at one
  node, in the state's color, that names the latest turn's state and, once
  that turn has finished, its record. The state is said there once, not
  repeated in the answer above. While the rows on screen stop short of the
  latest turn (scrolled back from the newest row, or reading older history),
  the line stops short of that node. Messages carry no visible author label;
  give each row a full author/context label for assistive technology. Copy
  controls stay out of sight until the pointer is over their row or they take
  keyboard focus. Away from the newest row, one control at the bottom of the
  conversation jumps back to it.
- **Conversation text.** All message text is selectable; the selection is a
  neutral fill. Links are blue and underlined, and their address appears at the
  bottom edge of the conversation before a click. Lists hang their wrapped
  lines from the text, not the marker. Tables size columns to their content with a
  quiet header row and scroll sideways when wide.
- **History and reasoning.** Earlier pages load near the top of the conversation;
  a quiet line names loading, the beginning, or a failed read with Retry. Prepending
  preserves the current row and its scroll position. Home/Cmd-Up goes to the
  loaded beginning; End/Cmd-Down and Jump to latest return to the live end.
  Reasoning is a separate, muted disclosure, initially closed after a response
  finishes, with its chevron first, as on a tool call. Live reasoning opens in
  a bounded-height region. Both use the same
  text selection and copy behavior as answers. A turn's record holds only what
  the host reported: tool steps and the files its edits changed. Do not invent
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
- **Composer.** One raised, rounded box holds the draft, a short key hint, and
  Send. Stop remains a separate neutral control during a run. Enter follows up;
  Option-Enter steers. The draft grows from one line to about a third of the
  window, then scrolls. Waiting and unconfirmed messages form a compact,
  scrollable stack above it, with explicit mode and Cancel or Retry controls.
  Below it, wrapping controls show model, the thinking level by name alone,
  context use, and a run-settings menu labeled with the thread's approval mode
  (and trusted commands, when on). Labels name the setting, not when it applies;
  hover text says that. Model names truncate; actions remain legible.
  Project and machine stay in navigation and the header. A running turn names
  its own approval mode, distinct from the next-message choice.
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
- **Settings.** Host status and Reconnect first, then Models, Providers,
  Approvals, Keyboard, View, and About tabs. View lists light and dark themes
  side by side, each with a swatch of its background, text, and accents. The heading and tabs stay visible while
  the active section scrolls. Each setting is a label and one line
  of meaning on the left and its control on the right. Provider forms occupy their section while open; keys are masked, with explicit reveal. Trusted command access needs a
  consequence summary and a separate confirmation. Shortcut recording shows
  conflicts in place and lets the person cancel or reassign. A choice applies when
  made; its saved or failed state is a dot and a sentence beneath it.
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
  Discard hunk appear on hover.

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

System sans-serif at 11–15 points, with 13 points for body and navigation. A
request is 14 points semibold; headings inside a reply are 15, 14, and 13
points bold. Code, file paths, and commands use the system's monospaced font
(SF Mono) at 12 points, or an installed monospaced family named in Settings →
Appearance; a family that is not installed, or whose characters differ in
width, is not drawn, so code keeps one character width. Conversation content,
the composer, and settings pages are capped at 760 points; the turn line, in
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
