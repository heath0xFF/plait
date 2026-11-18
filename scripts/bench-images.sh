#!/usr/bin/env bash
# Fetch and prepare a full-size thumbnail through a real host, without a model or app window.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
cargo build --locked --release -p plait-host
cargo test --locked --release -p plait --no-run
cd runtime
PLAIT_HOST_BINARY="$root/target/release/plait-host" node test/bench-images.ts
