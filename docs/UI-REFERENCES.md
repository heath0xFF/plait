# Product comparison policy

## Original implementation and design

Plait must be better through its own engineering and user experience. Do not
copy or port application code, assets, or distinctive UI from T3, BB, Orca, or
other products. Do not translate their code into Rust or use their source as an
implementation template. A permissive competitor license does not change this
product rule.

Compare observable outcomes: how quickly a developer starts useful work, how
clearly the app shows effects, whether work survives a disconnect, and how safely
an update completes. Write requirements from these observations, then implement
and test them within Plait's architecture. Do not reproduce another app's visual
identity or feature list as a substitute for product decisions.

Declared libraries such as GPUI and Pi are dependencies, not copied application
code. Keep them pinned, inspect their supported APIs, and retain their licenses
and notices. The owner's supplied Plait branding remains the source of assets.

## Historical comparison

Earlier layout work studied BB at revision
`da3e60b81d560e114a2bec724bb59c997aaaae7f` from <https://github.com/get-bb/bb>.
The separate reference checkout and historical verification notes record that
study. They are not a current instruction to match BB. No reference checkout is
a Plait dependency or submodule. Keep its license with it.

## What defines Plait

- Rust + GPUI, not a web desktop shell.
- Project → Machine → Thread, with execution location visible.
- Independent hosts and recoverable conversations.
- No mandatory worktrees, embedded terminal, or hidden orchestration.
- First-class Git; the desktop Git interface remains read-only through M2.
- Explicit approvals, safe Stop, protected existing work, and honest state labels.
- Low input latency, stable scrolling, and no unnecessary idle work.
- Simple installation and independent client/host updates.

A comparison is not evidence of superiority. Publish measured results and the
conditions under which they were obtained.
