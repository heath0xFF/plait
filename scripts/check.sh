#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

require_sandbox=false
if [[ $# -gt 1 || (${1:-} != '' && ${1:-} != '--require-command-sandbox') ]]; then
    echo 'Usage: bash scripts/check.sh [--require-command-sandbox]' >&2
    exit 1
fi
if [[ ${1:-} == '--require-command-sandbox' ]]; then
    require_sandbox=true
fi

case "$(uname -s)" in
    Darwin)
        packages=(--workspace)
        echo 'Checking the macOS workspace and command sandbox.'
        ;;
    Linux)
        if "$require_sandbox"; then
            echo 'Linux command-sandbox acceptance is blocked. See docs/LINUX-SANDBOX.md.' >&2
            exit 1
        fi
        packages=(-p plait-core -p plait-host)
        echo 'Checking Linux headless development. Native UI and Linux command execution are not validated.'
        ;;
    *)
        echo 'Development checks support macOS and Linux only.' >&2
        exit 1
        ;;
esac

cargo fmt --all -- --check
# The daemon and shared core must stay independent of desktop dependencies.
dependencies=$(cargo tree --locked -p plait-core -p plait-host --edges normal,build,dev --prefix none)
if grep -Eq '^gpui([-_ ]|$)' <<< "$dependencies"; then
    echo 'Headless crates must not depend on GPUI.' >&2
    exit 1
fi
cargo clippy --locked "${packages[@]}" --all-targets -- -D warnings
cargo test --locked "${packages[@]}"
cargo build --locked -p plait-host
(
    cd runtime
    npm ci --ignore-scripts
    npm run check
    if "$require_sandbox"; then
        npm run test:sandbox
    else
        npm test
    fi
)
cargo build --locked --release "${packages[@]}"
