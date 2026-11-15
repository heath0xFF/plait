# Plait desktop design

## Direction
Follow the user's BB-inspired native conversation brief. This is a working
developer tool, not a marketing surface. Keep familiar navigation and controls.

## Layout
Compact Project → Machine → Thread rail; conversation or diff workspace;
optional wide-window Git inspector. Keep machine and thread context visible.
The supplied ribbon P and outlined wordmark live in the sidebar, not in a second
app-wide header. Use the owner's vector kit in `assets/branding`, not a generic
braid or a crop from the reference sheet. The Dock/Finder icon uses the kit's
rounded tile. Category glyphs use the kit's paths: mint projects, blue machines,
violet threads, and light Git. Keep separate state dots and readable labels.

Use right-aligned, neutral user-message bubbles. Agent replies remain open text
on the conversation surface. Tool events start as compact disclosure rows;
expand them in place for details. Keep one clear example-thread notice and
visible mock labels on tool rows, with full author/context labels for assistive
technology. The bottom composer remains visibly unavailable until M2 is real.

The supplied BB screenshots guide visual density and message treatment, not
architecture: no spawned-agent hierarchy, orchestration, or required worktrees.

## Tokens
- Background: #171819. Navigation: #1c1d1f. Panel / user bubble: #26282b.
- Border: #303238. Main text: #e8eaf0. Secondary text: #9da5b4.
- Mint #94dec5: local/read-only state, selection, additions.
- Blue #93b8f5: tool events, modifications, hunk headers.
- Violet #c0adf3: examples and completed sample threads.
- Amber #e4c58d: attention/limits. Rose #f0a6ac: errors/deletions.

System sans-serif at 11–19 points; Menlo at 12 points for code. Conversation
content is capped at 820 points; user bubbles use at most 85% of that width. Spacing follows a 4-point
base. Preserve gradients inside the supplied brand artwork only. Avoid new
chrome gradients, shadows, animation, and decorative dashboard cards.

## Behavior
Native titlebar, menus, folder picker, close/minimize controls. Keyboard focus
must remain visible. Selection is not color-only. Long lists are virtualized.
Do not draw an enabled composer or a send control until an agent can run.
