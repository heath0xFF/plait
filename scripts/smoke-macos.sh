#!/bin/bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
binary="$root/target/release/plait"
if pgrep -x plait >/dev/null; then
    echo 'Close Plait first. This check launches and stops only its own test processes.' >&2
    exit 1
fi
if [ ! -x "$binary" ]; then
    echo 'Run cargo build --locked --release first.' >&2
    exit 1
fi
fixture=$(mktemp -d -t plait-smoke)
app_pid=''
stop_app() {
    if [ -n "$app_pid" ]; then
        kill "$app_pid" 2>/dev/null || true
        wait "$app_pid" 2>/dev/null || true
        app_pid=''
    fi
}
cleanup() {
    result=$?
    stop_app
    if [ "$result" -eq 0 ]; then
        rm -rf "$fixture"
    else
        echo "Check failed. Disposable fixtures and app logs retained at: $fixture" >&2
    fi
}
trap cleanup EXIT
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
mkdir "$fixture/dirty" "$fixture/clean" "$fixture/not-a-repository"
for name in dirty clean; do
    git -C "$fixture/$name" init -q -b main
    git -C "$fixture/$name" config user.name 'Plait Test'
    git -C "$fixture/$name" config user.email test@example.invalid
    git -C "$fixture/$name" config commit.gpgsign false
done
# All writes stay in a new disposable repository, never in the checkout under review.
printf 'pub fn refresh() {\n    read_status();\n}\n' > "$fixture/dirty/review.rs"
git -C "$fixture/dirty" add review.rs
git -C "$fixture/dirty" commit -qm fixture
printf 'pub fn refresh() {\n    queue_status();\n}\n' > "$fixture/dirty/review.rs"
git -C "$fixture/dirty" add review.rs
printf 'pub fn refresh() {\n    queue_status();\n    notify_when_ready();\n}\n' > "$fixture/dirty/review.rs"
printf '# Read-only review\n' > "$fixture/dirty/notes.md"
cp "$fixture/dirty/.git/index" "$fixture/index-before"
cp "$fixture/dirty/review.rs" "$fixture/file-before"
for mode in dirty clean not-a-repository; do
    "$binary" "$fixture/$mode" > "$fixture/$mode.log" 2>&1 &
    app_pid=$!
    osascript "$root/scripts/smoke-macos.applescript" "$mode"
    stop_app
done
cmp "$fixture/index-before" "$fixture/dirty/.git/index"
cmp "$fixture/file-before" "$fixture/dirty/review.rs"
echo 'PASS: index and working file unchanged.'
