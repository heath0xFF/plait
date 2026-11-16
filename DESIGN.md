# Plait desktop design

## Direction
BB is the primary look-and-feel reference, not just loose inspiration. See
[the pinned reference checkout and study areas](docs/UI-REFERENCES.md).
Keep Plait's native GPUI implementation, Project → Machine → Thread hierarchy,
independent host, Git focus, safety rules, and supplied branding. This is a
working developer tool, not a marketing surface.

What Plait takes from BB: one compact chrome row, flat single-line navigation
rows, open assistant text with quiet user bubbles, compact tool disclosure, and
a raised composer with the thread's context directly beneath it. What it does
not take: BB's palette, icons, spawned-agent hierarchy, task board, plugins, or
required worktrees.

## Layout
Compact Project → Machine → Thread rail; conversation or diff workspace;
optional wide-window Git inspector. Keep machine and thread context visible.

- **Chrome.** One 44-point header: sidebar toggle, thread title, a muted
  `project / machine` on wide windows, state dot with a text label, and the
  Conversation / Changes switch. The sidebar's brand row is the same height, so
  the two align. No second header, tab strip, or permanent footer.
- **Rail.** The supplied ribbon P and outlined wordmark live in the rail, above
  two actions: New thread and Open repository. Every tree row is 28 points and
  one line. Category glyphs use the owner's vector kit in `assets/branding`:
  mint projects, blue machines, violet threads, and light Git. Do not use a
  generic braid or a crop from the reference sheet. The Dock/Finder icon uses
  the kit's rounded tile. Host state sits in a one-line footer with Settings.
- **Thread rows.** Real threads show their state as a text label and a dot.
  Example threads show a visible `mock` label and a dot; the header and the
  accessible name spell out the sample state. Selection is a neutral fill.
- **Conversation.** Right-aligned, neutral user bubbles. Agent replies remain
  open text on the conversation surface. Tool events are compact disclosure rows
  aligned to the text edge; expand them in place for details. Keep one clear
  example-thread notice and visible mock labels on tool rows, with full
  author/context labels for assistive technology.
- **Composer.** One raised, rounded box holds the draft, its hint, and a single
  send control that becomes Stop while the agent works. Directly beneath it, a
  context line names the Project, Machine, and checkout branch, the thread's
  saved model, and the approval rule. An example thread shows the same box
  visibly unavailable, with no send control.
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
- Violet #c0adf3: examples and completed sample threads.
- Amber #e4c58d: attention/limits. Rose #f0a6ac: errors/deletions.

Accents carry state, not decoration: selection and hover are neutral. The one
affirmative action in a view (Send, Create, Save) is light on dark.

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
