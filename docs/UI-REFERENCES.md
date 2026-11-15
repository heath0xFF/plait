# UI references

## BB: primary look-and-feel reference

The user wants Plait to look and feel like BB while keeping Plait's product
principles. Use BB as a concrete visual and interaction reference, not just a
loose source of inspiration. This does not change the current M2 execution work.

- Upstream: <https://github.com/get-bb/bb>
- Reference revision: `da3e60b81d560e114a2bec724bb59c997aaaae7f`
- Local checkout: `/Users/heath/code/bb` (outside the Plait repository)
- License at this revision: MIT, copyright 2026 Michael Yong.
- Checkout is shallow and clean. No dependencies were installed or app started.

The checkout is reference material, not a Plait dependency or submodule. Do not
copy source or assets into Plait. Keep BB's license with the reference checkout.

## What to study

The checked-in `assets/app-screenshot.png` shows a compact navigation rail,
small workspace headers, open assistant text, quiet user bubbles, restrained
borders, and a bottom composer with nearby model and execution controls.
These are the main visual reference points. A screenshot is evidence of one
state, not proof of current behavior or a complete token specification.

Useful source locations in the reference checkout:

| Area | BB path |
| --- | --- |
| Screenshot | `assets/app-screenshot.png` |
| Surface and color roles | `apps/app/src/components/ui/theme.css` |
| Navigation | `apps/app/src/components/sidebar/` |
| Conversation presentation | `apps/app/src/components/thread/` |
| Composer and execution controls | `apps/app/src/components/promptbox/` |
| Model selection | `apps/app/src/components/plugin/PluginProviderModelPicker.tsx` |
| Settings | `apps/app/src/components/settings/` |
| Git presentation | `apps/app/src/components/git-diff/` |

For a later UI pass, compare density, spacing, typography, message treatment,
tool disclosure, composer placement, keyboard flow, and scroll behavior against
rendered BB. Reimplement the useful behavior with native GPUI controls. Do not
assume that a source component's current appearance matches an older screenshot.

## What stays Plait

- Rust + GPUI, not Electron or a web UI.
- Project → Machine → Thread, with machine context visible.
- Independent host execution and recoverable threads.
- No mandatory worktrees, embedded terminal, or hidden orchestration.
- First-class Git; the desktop Git interface stays read-only through M2.
- Explicit approvals, safe Stop, protected existing work, and honest state labels.
- Plait's supplied ribbon mark and branding.

BB's dispatch, automation, plugin, and task-board features are not requirements
for Plait. A similar look and feel does not authorize copying its architecture
or widening the current milestone.
