# Plait product context

<!-- impeccable:product-schema 1 -->

## Platform
Native macOS desktop (Rust + GPUI). This is not a web surface.

## Product and users
A development environment for AI coding-agent work. The developer moves between
Project → Machine → Thread, reads conversations, and inspects Git changes.

## Scope and truth
README.md is the product authority. The desktop shows only real data: the
opened checkout's read-only Git and this machine's host threads. Remote
machines are not shown until remote execution exists.
No remote connection, agent execution, terminal, or persistence is claimed.

## Brand commitments
BB-inspired conversation layout, compact sidebar, charcoal surfaces, mint,
blue and violet accents. Use the owner's ribbon-P vector kit for branding and
category icons. Native keyboard behavior. No decorative animation.

## Principles
Responsiveness first. No blocking I/O on the UI thread. No mandatory worktrees.
No hidden execution or repository writes. Small, testable vertical slices.

## Decisions
The user's brief supplies the product and visual direction and explicitly
delegates M1 implementation decisions. No additional product assumptions needed.
