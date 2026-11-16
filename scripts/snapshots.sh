#!/usr/bin/env bash
# Render every UI scene to PNG files without opening a window.
#
# macOS draws through Metal with Core Text. Linux draws through wgpu and needs
# a software Vulkan driver such as mesa-vulkan-drivers. Scenes hold injected
# sample state; no host is contacted and no agent runs.
set -euo pipefail

if [[ $# -ne 1 || $1 == -* ]]; then
    echo 'Usage: bash scripts/snapshots.sh <output-directory>' >&2
    exit 1
fi

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
mkdir -p -- "$1"
output=$(cd -- "$1" && pwd)

# Only files written by this run are reported, so a renamed scene cannot pass
# off an older image as current.
marker=$(mktemp)
trap 'rm -f -- "$marker"' EXIT

cd "$root"
PLAIT_SNAPSHOT_DIR="$output" \
    cargo test --locked -p plait ui::snapshot::snapshot -- --exact --ignored

written=$(find "$output" -maxdepth 1 -name '*.png' -newer "$marker" | sort)
if [[ -z $written ]]; then
    echo "No snapshots were written to $output." >&2
    exit 1
fi
printf '%s\n' "$written"

stale=$(find "$output" -maxdepth 1 -name '*.png' ! -newer "$marker" | sort)
if [[ -n $stale ]]; then
    echo 'Not written by this run; left in place:' >&2
    printf '%s\n' "$stale" >&2
fi
