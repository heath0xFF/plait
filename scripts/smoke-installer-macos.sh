#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
if [[ $(uname -s) != Darwin || ${GITHUB_ACTIONS:-} != true ]]; then
    echo 'This installs system packages and is restricted to disposable GitHub macOS runners.' >&2
    exit 1
fi
if [[ -e /Applications/Plait.app || -e '/Library/Application Support/Plait/Host' ]]; then
    echo 'Refusing to overwrite an existing Plait installation in the install smoke check.' >&2
    exit 1
fi
manifests=("$root"/dist/Plait-Release-*.json)
if [[ ${#manifests[@]} -ne 1 || ! -f ${manifests[0]} ]]; then
    echo 'Expected one freshly built release manifest.' >&2
    exit 1
fi
export PLAIT_RELEASE_MANIFEST=${manifests[0]}
combined=$(python3 -c 'import json,os; r=json.load(open(os.environ["PLAIT_RELEASE_MANIFEST"])); print(next(a["name"] for a in r["artifacts"] if a["name"].startswith("Plait-Installer-")))')
build_id=$(python3 -c 'import json,os; print(json.load(open(os.environ["PLAIT_RELEASE_MANIFEST"]))["host"]["build_id"])')
host_package=$(python3 -c 'import json,os; r=json.load(open(os.environ["PLAIT_RELEASE_MANIFEST"])); print(next(a["name"] for a in r["artifacts"] if a["name"].startswith("Plait-Host-")))')
python3 scripts/build-upgrade-fixture-macos.py "$root/dist/$host_package" "$root/dist/installer-test/host-update.pkg"
sudo -n /usr/sbin/installer -pkg "$root/dist/$combined" -target /
node="/Library/Application Support/Plait/Host/releases/$build_id/bin/node"
if ! "$node" --test scripts/smoke-installed-macos.mjs; then
    "$node" scripts/diagnose-installed-macos.mjs || true
    exit 1
fi

fixture=$(mktemp -d /tmp/plait-installed-ui.XXXXXX)
pid=''
cleanup() {
    if [[ -n $pid ]]; then
        kill -TERM "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    rm -rf -- "$fixture"
}
trap cleanup EXIT
git init -q "$fixture/plait-acceptance"
PLAIT_DESKTOP_STATE_DIR="$fixture/desktop" /Applications/Plait.app/Contents/MacOS/plait "$fixture/plait-acceptance" > dist/installed-client.log 2>&1 &
pid=$!
sleep 5
kill -0 "$pid"
python3 - "$fixture" <<'PY'
import json, os, stat, sys
root = sys.argv[1]
path = os.path.join(root, 'desktop', 'desktop.json')
with open(path) as stream:
    state = json.load(stream)
assert os.path.realpath(state['checkout']) == os.path.realpath(os.path.join(root, 'plait-acceptance')), state
assert stat.S_IMODE(os.stat(path).st_mode) == 0o600
print('Installed desktop persisted the selected checkout privately.')
PY
kill -TERM "$pid"
wait "$pid" 2>/dev/null || true
pid=''
# No checkout argument: the installed app must restore it, not inspect the script's cwd.
PLAIT_DESKTOP_STATE_DIR="$fixture/desktop" /Applications/Plait.app/Contents/MacOS/plait >> dist/installed-client.log 2>&1 &
pid=$!
sleep 5
kill -0 "$pid"
client_package=$(python3 -c 'import json,os; r=json.load(open(os.environ["PLAIT_RELEASE_MANIFEST"])); print(next(a["name"] for a in r["artifacts"] if a["name"].startswith("Plait-Client-")))')
if sudo -n /usr/sbin/installer -pkg "$root/dist/$client_package" -target /; then
    echo 'Client installer did not refuse an active client.' >&2
    exit 1
fi
kill -0 "$pid"
if ! /usr/sbin/screencapture -x dist/installed-client.png; then
    echo 'The installed client stayed running, but this runner could not capture its display.' >&2
fi
printf 'Installed client launch and packaged host/runtime checks passed.\n'

# Exercise the same prebuilt installer shipped in the app, including migration
# from the system-wide packages tested above. Never compile during this step.
kill -TERM "$pid"
wait "$pid" 2>/dev/null || true
pid=''
before_threads=$(/usr/bin/sqlite3 "$HOME/.local/share/plait/host.sqlite" 'SELECT count(*) FROM threads')
before_draft=$(shasum -a 256 "$fixture/desktop/desktop.json")
bash dist/Plait.app/Contents/Resources/install-user.sh --app "$root/dist/Plait.app"
[[ $(/usr/bin/stat -f %u /Applications/Plait.app) == "$(id -u)" ]]
[[ -w /Applications && ! -e /Library/LaunchAgents/dev.plait.host.plist ]]
user_host="$HOME/Library/Application Support/Plait/Host"
installed_id=$(/usr/bin/plutil -extract build_id raw -o - "$user_host/installed.json")
[[ $installed_id == "$build_id" ]]
"$user_host/releases/$installed_id/bin/plait-host" status
[[ $(/usr/bin/sqlite3 "$HOME/.local/share/plait/host.sqlite" 'SELECT count(*) FROM threads') == "$before_threads" ]]
[[ $(shasum -a 256 "$fixture/desktop/desktop.json") == "$before_draft" ]]
printf 'Prebuilt per-user install preserved threads and drafts; in-app update ownership requirements passed.\n'
