# Working on Plait

- Read README.md and docs/ARCHITECTURE.md. Stay inside the current milestone.
- The desktop is Rust + GPUI. TypeScript is for the runtime sidecar and probes.
  No web UI, embedded terminal, or mandatory worktrees.
- Keep models and Git free of GPUI. State owns transitions; UI owns presentation.
- Run all filesystem and Git work on the background executor, never in render.
- The app's Git interface stays read-only through M2. Disable optional locks,
  external diff, and textconv. Normal development commits are permitted.
- Keep fixture events and remote machines visibly labeled. Do not imply execution.
- Pin dependencies. Check upstream source at the pinned revision before using APIs.
- Use small functions, explicit errors, and short comments explaining why.
- Test parsers, async result guards, and repository edge cases.
- Run cargo fmt --all -- --check, cargo clippy --workspace --all-targets -- -D warnings,
  and cargo test --workspace. Build and inspect the native app after UI changes.
- When using Rex, run builds and app sessions in named panes in the current tab.
  Rex is a development tool, not a build dependency.
- Read CONTRIBUTING.md for the M2 work split. Agree before changing shared contracts.
