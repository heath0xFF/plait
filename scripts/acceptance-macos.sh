#!/bin/bash
# See docs/VERIFICATION.md. --self-test never launches a window or sends input.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
exec python3 "$root/scripts/acceptance/run.py" "$@"
