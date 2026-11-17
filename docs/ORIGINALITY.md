# Original implementation and design

Plait's application code, assets, and design must be original. Do not copy, port,
or translate competitor application code, or use another app's distinctive UI
as an implementation template. A permissive license does not override this
project rule.

## Product comparisons

Compare observable outcomes: how quickly a developer starts useful work, how
clearly the app shows effects, whether work survives a disconnect, and how safely
an update completes. Turn those observations into requirements, then implement
and test them within Plait's architecture.

Do not reproduce another app's visual identity or feature list in place of product
decisions. A comparison is not evidence of superiority. Publish measured results
with the conditions under which they were obtained.

## Dependencies and assets

Declared libraries such as GPUI and Pi are dependencies, not copied application
code. Keep them pinned, inspect their supported APIs, and retain their licenses
and notices. Use the [Plait artwork](../assets/branding/README.md) for branding;
retain its provenance and editing notes.

Each agent harness is identified by its maker's logo, as other open-source agent
clients do. The [harness logos](../assets/harnesses/README.md) name the harness
only; they are never Plait branding. Keep their source, version, and license.

## Product boundaries

- Native Rust + GPUI, not a web desktop shell.
- Project → Machine → Thread, with execution location visible.
- Independent hosts and recoverable conversations.
- No mandatory worktrees, embedded terminal, or agent orchestration.
- Read-only desktop Git through M2.
- Exact approvals, safe Stop, protected existing work, and honest state labels.
- Measured responsiveness and independent client/host updates.
