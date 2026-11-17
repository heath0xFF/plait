# Working on Plait

- Read README.md and docs/ARCHITECTURE.md. Stay inside the current milestone.
- The desktop is Rust + GPUI. TypeScript is for the runtime sidecar and probes.
  No web UI, embedded terminal, or mandatory worktrees.
- Keep models and Git free of GPUI. State owns transitions; UI owns presentation.
- Run all filesystem and Git work on the background executor, never in render.
- Git reads in the app stay hardened: disable optional locks, external diff,
  textconv, and filters. Git writes are limited to what docs/ROADMAP.md agrees
  (staging, unstaging, committing, push, publish, fetch; hunk staging and
  confirmed discard are agreed next). Normal development commits are permitted.
- Show only real data in the app: no sample threads or placeholder machines.
  Test scenes may inject sample state, never in a shipped view. Do not imply execution.
- Build original application code and design. Do not copy or port competitor code,
  assets, or distinctive UI. Compare user outcomes, not implementation details.
  Declared, pinned dependencies are permitted; retain their licenses.
- This repository is public. Keep docs current and reader-facing; omit personal
  workstation details, session handoffs, private transcripts, and agent work logs.
  Separate shipped behavior, planned work, and verification limits. Keep provenance.
- Pin dependencies. Check upstream source at the pinned revision before using APIs.
- Use small functions, explicit errors, and short comments explaining why.
- Test parsers, async result guards, and repository edge cases.
- Run cargo fmt --all -- --check, cargo clippy --workspace --all-targets -- -D warnings,
  and cargo test --workspace. Build and inspect the native app after UI changes.
- When using Rex, run builds and app sessions in named panes in the current tab.
  Rex is a development tool, not a build dependency.
- Read CONTRIBUTING.md for the work split. Agree before changing shared contracts;
  docs/ROADMAP.md records what is agreed for the current milestone.
