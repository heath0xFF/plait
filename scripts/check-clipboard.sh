#!/usr/bin/env bash
# Explicit macOS clipboard regression test and timing probe; no app or window.
# Saves and restores existing pasteboard representations without printing them.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
[[ $(uname -s) == Darwin ]] || { echo 'This probe requires macOS.' >&2; exit 1; }
work=$(mktemp -d)
chmod 700 "$work"
restore() {
    if [[ -f $work/clipboard ]]; then "$work/fixtures" restore "$work/clipboard"; fi
    rm -rf -- "$work"
}
trap restore EXIT
swiftc scripts/clipboard-fixtures.swift -o "$work/fixtures"
cargo build --locked --release -p plait --example clipboard_image_probe
"$work/fixtures" save "$work/clipboard"
for pair in png,tiff:png jpeg,tiff:jpeg tiff:tiff png,text:text; do
    formats=${pair%:*}
    expected=${pair#*:}
    "$work/fixtures" offer runtime/test/support/images/0.png "$formats"
    "$work/fixtures" types
    target/release/examples/clipboard_image_probe --read "$expected"
done
target/release/examples/clipboard_image_probe
