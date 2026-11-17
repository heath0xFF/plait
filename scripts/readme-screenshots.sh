#!/usr/bin/env bash
# Render the README screenshots into docs/images.
#
# The views are Plait's own, drawn headlessly (Metal and Core Text on macOS)
# over a disposable sample repository. Threads and tool results are written
# by hand in crates/plait/src/ui/snapshot/showcase.rs; no agent runs.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
output="$root/docs/images"
mkdir -p -- "$output"
cd "$root"
PLAIT_SNAPSHOT_DIR="$output" \
    cargo test --locked -p plait ui::snapshot::showcase::readme_screenshots -- --exact --ignored
ls -1 "$output"/*.png
