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

- **Chrome.** One 44-point header: sidebar toggle, thread title, a muted
  `project / machine` on wide windows, state dot with a text label, and the
  Conversation / Changes switch, then the Git inspector toggle, mirroring the
  sidebar toggle at the other end. The sidebar's first action row centers on the
  same 44 points. No second header, tab strip, or permanent footer.
- **Rail.** No in-app logo: the app icon and window carry the brand. The rail
  starts with two actions, New thread and Open repository. New thread names the
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
- **Conversation.** Right-aligned, neutral user bubbles. Agent replies remain
  open text on the conversation surface. Position says who is speaking, so
  messages carry no visible author label; give each row a full author/context
  label for assistive technology. Copy controls stay out of sight until the
  pointer is over their row or they take keyboard focus. The answer being
  written ends with a still dot and the word "Writing…". Away from the newest
  row, one control at the bottom of the conversation jumps back to it.
- **Conversation text.** All message text is selectable; the selection is a
  neutral fill. Links are blue and underlined, and their address appears at the
  bottom edge of the conversation before a click. Lists hang their wrapped
  lines from the text, not the marker. Tables size columns to their content with a
  quiet header row and scroll sideways when wide.
- **Tool calls.** One compact line each: disclosure chevron, tool name, the
  path or command, a short fact such as `+2 −1`, and a state label only when
  the call did not simply finish. Opened in place, `edit` shows a line diff,
  `write` the new file, `command` a `$` prompt line and its output, and `read`
  the first lines of the file. A failed command says "Failed" above output
  outlined in rose. A long run of calls folds under one line that names their
  count and kinds and any failures; a turn in progress keeps its latest calls
  in view.
- **Composer.** One raised, rounded box holds the draft, its hint, and a single
  send control that becomes Stop while the agent works. The draft is one line
  with placeholder text when empty and grows as it is written, to about a third
  of the window; past that it scrolls, with a thin position marker in the right
  padding. The hint always says what the keys do now. Directly beneath the box,
  a context line names the Project, Machine, and checkout branch, the thread's
  saved model, and the approval rule.
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
- **Settings.** Host status and Reconnect first, then ruled sections: Models,
  Approvals, Keyboard, About and updates. Each setting is a label and one line
  of meaning on the left and its control on the right. A choice applies when
  made; its saved or failed state is a dot and a sentence beneath it.
- **Approval.** A bordered card above the composer shows the exact arguments,
  the risk, protected dirty paths, and Deny once / Allow once.
- **Git.** Changed files are 28-point rows: a one-letter state, file name first,
  then its directory, then index/worktree. The accessible name carries the full
  path and state. Diffs keep separate Unstaged and Staged views.

## Tokens
- Background: #171819. Navigation: #1c1d1f. Panel / user bubble: #26282b.
- Border: #303238. Control outline: #3a3d45. Selected row: #2b2d31.
- Main text: #e8eaf0. Secondary text: #9da5b4. Labels and hints: #7f8696.
- Mint #94dec5: local/read-only state, additions, the chosen option.
- Blue #93b8f5: running work, modifications, hunk headers, keyboard focus.
- Violet #c0adf3: thread identity.
- Amber #e4c58d: attention/limits. Rose #f0a6ac: errors/deletions.

Accents carry state, not decoration: selection and hover are neutral. The one
affirmative action in a view (Send, Create, Save) is light on dark.

Code uses the same palette for syntax, and nowhere else is color read this way:
violet keywords, mint strings, amber numbers and constants, blue types, keys,
and attributes, secondary-text punctuation, and hint-colored comments. Function
names and plain identifiers stay main text. Diff lines keep the Git meaning:
a `+` or `−` gutter mark with a mint or rose line tint at 12% opacity. Selected
text is white at 18% opacity, so it reads the same on every surface. Inline
code is main text on the panel color.

System sans-serif at 11–15 points, with 13 points for body and navigation; Menlo
at 12 points for code. Conversation content, the composer, and settings pages
are capped at 760 points; user bubbles use at most 80% of that width. Spacing
follows a 4-point base. Preserve gradients inside the supplied brand artwork
only. Avoid new chrome gradients, shadows, animation, and decorative dashboard
cards.

## Behavior
Native titlebar, menus, folder picker, close/minimize controls. Keyboard focus
must remain visible: a blue ring on controls and a blue edge on the composer.
Selection is not color-only. Long lists are virtualized.
Do not draw an enabled composer or a send control until an agent can run.
Render timings are a diagnostic, shown by View → Toggle Diagnostics, not
permanent chrome.
