#!/bin/bash
# Install this checkout or an already-built app as this user's Plait.
# Everything installs into locations the user owns, so updates
# need no administrator access. The one exception is a one-time removal of an
# earlier system-wide (package) installation.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

app='/Applications/Plait.app'
host_base="$HOME/Library/Application Support/Plait/Host"
agent="$HOME/Library/LaunchAgents/dev.plait.host.plist"
legacy_agent='/Library/LaunchAgents/dev.plait.host.plist'
legacy_host='/Library/Application Support/Plait/Host'
label='dev.plait.host'
bundle_id='dev.plait.desktop'
store="$HOME/.local/share/plait"
uid=$(/usr/bin/id -u)

usage() {
    printf '%s\n' \
        'Usage: update-installed-macos.sh [--app /path/to/Plait.app | --no-build]' \
        '                                 [--stage-host | --activate-host]' \
        '' \
        'Builds Plait from this checkout, including uncommitted changes, and installs' \
        "it as $app with its host in your Library folder. The host is unpacked only" \
        'when its content changed. A new host replaces the running one only while no' \
        'run is active; otherwise it is staged and the script says how to finish.' \
        '' \
        '  --no-build       install the app already built in dist/' \
        '  --app PATH       install a prebuilt app; no source checkout or build tools needed' \
        '  --stage-host     install a new host but leave the running host alone' \
        '  --activate-host  only switch the running host to the installed build'
}

fail() { printf 'Plait update: %s\n' "$*" >&2; exit 1; }
field() { /usr/bin/plutil -extract "$1" raw -o - "$2"; }

# Mirrors the runtime store's recovery query: runs a host restart would interrupt.
# Query-only, not -readonly: a read-only open of a WAL store fails while no host
# is running, because SQLite cannot create the shared-memory file.
active_runs() {
    [[ -f "$store/host.sqlite" ]] || { echo 0; return; }
    /usr/bin/sqlite3 -cmd 'PRAGMA query_only=1' "$store/host.sqlite" \
        "SELECT count(*) FROM threads WHERE json_extract(snapshot,'\$.status') IN ('running','awaiting_approval')"
}

agent_loaded() { /bin/launchctl print "gui/$uid/$label" >/dev/null 2>&1; }

active_program() {
    /bin/launchctl print "gui/$uid/$label" 2>/dev/null | /usr/bin/awk -F' = ' '/^\tprogram = /{print $2; exit}'
}

stop_agent() {
    agent_loaded || return 0
    /bin/launchctl bootout "gui/$uid/$label" || true
    for _ in $(seq 50); do
        agent_loaded || return 0
        /bin/sleep 0.2
    done
    fail 'The running host did not stop.'
}

require_idle() {
    local runs
    # Called from conditions, where errexit is off: check the query explicitly.
    runs=$(active_runs) || fail 'Could not read active runs from the host store.'
    [[ $runs =~ ^[0-9]+$ ]] || fail "Unexpected active-run count: $runs"
    [[ $runs == 0 ]] && return 0
    printf '%s run(s) are in progress or awaiting approval. %s\n' "$runs" "$1"
    return 1
}

quit_client() {
    /usr/bin/pgrep -x '[Pp]lait' >/dev/null || return 1
    # Quitting saves drafts and never stops host work.
    /usr/bin/osascript -e "tell application id \"$bundle_id\" to quit" >/dev/null 2>&1 || true
    for _ in $(seq 75); do
        /usr/bin/pgrep -x '[Pp]lait' >/dev/null || return 0
        /bin/sleep 0.2
    done
    fail 'Another Plait client is still running (for example a source build). Quit it and try again.'
}

# Earlier previews installed system-wide with Installer packages. Remove those
# program files once; threads, drafts and credentials are not touched.
migrate_legacy() {
    local owner=''
    [[ -e $app ]] && owner=$(/usr/bin/stat -f %Su "$app")
    [[ -e $legacy_agent || -e $legacy_host || ( -n $owner && $owner != "$USER" ) ]] || return 0
    require_idle 'Finish or Stop them before the one-time move to a per-user install.' || exit 1
    printf 'Moving Plait from the system-wide install to your user folders (one time).\n'
    printf 'Administrator access is needed once to remove the old program files.\n'
    /usr/bin/sudo -v
    stop_agent
    /usr/bin/sudo /bin/rm -f "$legacy_agent"
    /usr/bin/sudo /bin/rm -rf "$legacy_host"
    if [[ -n $owner && $owner != "$USER" ]]; then
        /usr/bin/sudo /bin/rm -rf "$app"
    fi
    local receipt
    for receipt in $(/usr/sbin/pkgutil --pkgs | /usr/bin/grep -E '^dev\.plait\.(client|host\.release\.)' || true); do
        /usr/bin/sudo /usr/sbin/pkgutil --forget "$receipt" >/dev/null
    done
}

install_app() {
    local source=$1 next="$app.next" previous="$app.previous"
    /usr/bin/codesign --verify --deep --strict "$source" || fail "$source failed its signature check."
    /bin/rm -rf "$next" "$previous"
    /usr/bin/ditto "$source" "$next"
    [[ -e $app ]] && /bin/mv "$app" "$previous"
    /bin/mv "$next" "$app"
    /bin/rm -rf "$previous"
}

# Releases are immutable and content-addressed: a running host keeps its files.
install_host() {
    local metadata="$app/Contents/Resources/host.json" build_id staging release
    build_id=$(field build_id "$metadata")
    [[ $build_id =~ ^[0-9]+\.[0-9]+\.[0-9]+-[0-9a-f]{20}$ ]] || fail 'Invalid host build identity.'
    release="$host_base/releases/$build_id"
    /bin/mkdir -p "$host_base/releases"
    if [[ -e $release ]]; then
        printf 'Host %s is already installed; its content did not change.\n' "$build_id"
    else
        printf 'Installing host %s…\n' "$build_id"
        staging=$(/usr/bin/mktemp -d "$host_base/releases/.staging.XXXXXX")
        /usr/bin/tar -xzf "$app/Contents/Resources/Host.tar.gz" -C "$staging"
        [[ $(field build_id "$staging/$build_id/component.json") == "$build_id" ]] || fail 'Host archive does not match its build identity.'
        /bin/mv "$staging/$build_id" "$release"
        # Read-only after the move: renaming a directory needs write access to it.
        /bin/chmod -R a-w "$release"
        /bin/rmdir "$staging"
    fi
    /bin/cp "$metadata" "$host_base/installed.json.next"
    /bin/mv "$host_base/installed.json.next" "$host_base/installed.json"
}

# Keep the installed release and the running one; older releases are unused.
prune_hosts() {
    local keep running release
    keep=$(field build_id "$host_base/installed.json")
    running=$(active_program)
    for release in "$host_base"/releases/*; do
        [[ -d $release ]] || continue
        [[ ${release##*/} == "$keep" || $running == "$release/"* ]] && continue
        /bin/chmod -R u+w "$release" && /bin/rm -rf "$release"
    done
}

write_agent() {
    local program=$1 next="$agent.next"
    /bin/mkdir -p "$(dirname "$agent")"
    /bin/rm -f "$next"
    /usr/bin/plutil -create xml1 "$next"
    /usr/bin/plutil -insert Label -string "$label" "$next"
    /usr/bin/plutil -insert ProgramArguments -array "$next"
    /usr/bin/plutil -insert ProgramArguments.0 -string "$program" "$next"
    /usr/bin/plutil -insert ProgramArguments.1 -string run-user "$next"
    /usr/bin/plutil -insert RunAtLoad -bool true "$next"
    /usr/bin/plutil -insert KeepAlive -dictionary "$next"
    /usr/bin/plutil -insert KeepAlive.SuccessfulExit -bool false "$next"
    /usr/bin/plutil -insert ThrottleInterval -integer 10 "$next"
    /usr/bin/plutil -insert ProcessType -string Standard "$next"
    /usr/bin/plutil -insert EnvironmentVariables -dictionary "$next"
    /usr/bin/plutil -insert EnvironmentVariables.PATH -string /usr/bin:/bin:/usr/sbin:/sbin "$next"
    /usr/bin/plutil -lint "$next" >/dev/null
    /bin/mv "$next" "$agent"
}

activate_host() {
    [[ -f "$host_base/installed.json" ]] || fail 'No installed host was found.'
    local build_id executable
    build_id=$(field build_id "$host_base/installed.json")
    executable="$host_base/releases/$build_id/bin/plait-host"
    write_agent "$executable"
    if [[ $(active_program) == "$executable" ]]; then
        printf 'Host %s is already active.\n' "$build_id"
        prune_hosts
        return
    fi
    if agent_loaded && ! require_idle 'The new host is staged.'; then
        printf 'Finish or Stop them, then run: scripts/update-installed-macos.sh --activate-host\n'
        return
    fi
    # A run admitted between the check and shutdown is marked interrupted; it is never replayed.
    printf 'Activating host %s…\n' "$build_id"
    stop_agent
    /bin/launchctl bootstrap "gui/$uid" "$agent"
    for _ in $(seq 100); do
        if "$executable" status >/dev/null 2>&1; then
            printf 'Host %s is active.\n' "$build_id"
            prune_hosts
            return
        fi
        /bin/sleep 0.2
    done
    fail "Host did not answer after activation. Check: \"$executable\" status"
}

main() {
    local build=true prebuilt=false host_mode=activate source="$root/dist/Plait.app"
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --no-build) build=false ;;
            --app)
                [[ $# -ge 2 && $2 == /* ]] || fail '--app requires an absolute bundle path.'
                source=$2; build=false; prebuilt=true; shift ;;
            --stage-host) host_mode=stage ;;
            --activate-host) activate_host; return ;;
            --help|-h) usage; return ;;
            *) usage >&2; fail "Unknown option: $1" ;;
        esac
        shift
    done
    [[ $(/usr/bin/uname -s) == Darwin && $(/usr/bin/uname -m) == arm64 ]] || fail 'This requires an Apple Silicon Mac.'
    [[ $EUID -ne 0 ]] || fail 'Run as your normal user, not with sudo.'
    local os
    os=$(/usr/bin/sw_vers -productVersion)
    [[ ${os%%.*} -ge 15 ]] || fail 'macOS 15 or later is required.'

    if "$build"; then
        PLAIT_BUILD_TIME=$(/bin/date -u '+%Y-%m-%d %H:%M UTC')
        export PLAIT_BUILD_TIME
        bash scripts/build-macos.sh
    fi
    [[ -f "$source/Contents/Resources/host.json" ]] || fail 'The app has no embedded host.'
    /usr/bin/codesign --verify --deep --strict "$source" || fail 'The app failed its signature check.'
    [[ $(field CFBundleIdentifier "$source/Contents/Info.plist") == "$bundle_id" ]] || fail 'The bundle is not Plait.'
    if "$prebuilt" && [[ $host_mode == activate ]]; then
        require_idle 'Finish or Stop them, then run the installer again.' || exit 1
    fi

    quit_client || true
    migrate_legacy
    printf 'Installing %s…\n' "$app"
    install_app "$source"
    install_host
    if [[ $host_mode == activate ]]; then
        activate_host
    else
        printf 'Host left unchanged. Activate later: scripts/update-installed-macos.sh --activate-host\n'
    fi
    /usr/bin/open "$app"
    printf 'Done. Settings → About shows the running desktop build and host protocol.\n'
}

# Sourcing defines the steps without running them, for isolated checks.
if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
    main "$@"
fi
