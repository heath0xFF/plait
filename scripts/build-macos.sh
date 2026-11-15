#!/bin/bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
if [[ $(uname -s) != Darwin || $(uname -m) != arm64 ]]; then
    echo 'This development package requires an Apple Silicon Mac.' >&2
    exit 1
fi
cargo build --locked --release --target-dir "$root/target"
cargo metadata --locked --no-deps --format-version 1 > target/macos-package-metadata.json
# License harvesting is a build tool, not a dependency shipped inside the app.
cargo install cargo-about --locked --version 0.9.3 --features cli --root target/build-tools
target/build-tools/bin/cargo-about generate --locked --fail \
    --manifest-path crates/plait/Cargo.toml --config packaging/macos/about.toml \
    --output-file target/THIRD_PARTY_NOTICES.txt packaging/macos/notices.hbs
python3 scripts/package-macos.py
