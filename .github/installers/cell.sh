#!/usr/bin/env bash
# One install cell of installers.yml on macOS or Linux: installs a component
# selection with the Qt IFW installer, then checks that exactly the selected
# components are present and that each present one runs.
#
# Usage: cell.sh <installer> <selection> <version>
#   <installer>  the .dmg on macOS, the .run on Linux
#   <selection>  all, workshop, or gateway
#   <version>    the version Workshop must print; the Gateway must print the
#                version gateway-pin/pin.json names
#
# Environment:
#   CELL_ROOT    install here (IFW --root) instead of the default location,
#                for local runs with a temporary HOME
#   CELL_LOGS    write the logs here (default: a new temporary directory)
#
# Written for bash 3.2, the bash macOS ships.
set -euo pipefail

fail() {
  echo "cell: $*" >&2
  exit 1
}

[ "$#" -eq 3 ] || fail "usage: cell.sh <installer> <all|workshop|gateway> <version>; got $# arguments"
installer=$1
selection=$2
version=$3
[ -f "$installer" ] || fail "installer $installer does not exist"
case "$installer" in
  /*) ;;
  *) installer="$PWD/$installer" ;;
esac

want_workshop=0
want_gateway=0
case "$selection" in
  all) want_workshop=1 want_gateway=1 ;;
  workshop) want_workshop=1 ;;
  gateway) want_gateway=1 ;;
  *) fail "selection must be all, workshop, or gateway; got '$selection'" ;;
esac
ids=""
[ "$want_workshop" = 0 ] || ids="$ids com.runvil.workshop"
[ "$want_gateway" = 0 ] || ids="$ids com.runvil.gateway"
# The installers carry the Gateway release the repository pins.
pin="$(cd "$(dirname "$0")/../.." && pwd)/gateway-pin/pin.json"
gateway_version=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' "$pin") ||
  fail "cannot read the Gateway version from $pin"

system=$(uname -s)
profile="$HOME/.runvil"
applications="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
case "$system" in
  Darwin)
    root=${CELL_ROOT:-/Applications/Runvil}
    workshop_item="$root/Runvil.app"
    workshop_exe="$workshop_item/Contents/MacOS/runvil-workshop"
    gateway_item="$root/Runvil Gateway.app"
    gateway_exe="$gateway_item/Contents/MacOS/runvil-gateway"
    ;;
  Linux)
    root=${CELL_ROOT:-$HOME/Runvil}
    workshop_item="$root/Runvil.AppImage"
    workshop_exe=$workshop_item
    gateway_item="$root/runvil-gateway"
    gateway_exe=$gateway_item
    ;;
  *) fail "cell.sh runs on macOS or Linux; uname -s printed $system" ;;
esac

logs=${CELL_LOGS:-$(mktemp -d)}
mkdir -p "$logs"
echo "cell: $selection on $system, logs in $logs"

mount=""
started=""
cleanup() {
  for pid in $started; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  if [ -n "$mount" ]; then
    hdiutil detach "$mount" -quiet 2>/dev/null || true
  fi
  if [ -d "$profile/logs" ]; then
    rm -rf "$logs/runvil-logs"
    cp -R "$profile/logs" "$logs/runvil-logs" 2>/dev/null || true
  fi
}
trap cleanup EXIT

canonical() {
  (cd "$(dirname "$1")" && printf '%s/%s\n' "$(pwd -P)" "$(basename "$1")")
}

# The executable of a running process, as an absolute path.
process_exe() {
  case "$system" in
    Linux) readlink "/proc/$1/exe" 2>/dev/null || true ;;
    Darwin) ps -o comm= -p "$1" 2>/dev/null || true ;;
  esac
}

# A process and all of its descendants, space separated. Workshop's
# listener may sit in a child: the AppImage runtime and xvfb-run both start
# the app below the process the cell launched.
process_tree() {
  local queue=$1 all="" pid children
  while [ -n "$queue" ]; do
    pid=${queue%% *}
    if [ "$queue" = "$pid" ]; then queue=""; else queue=${queue#* }; fi
    all="$all $pid"
    children=$(pgrep -P "$pid" 2>/dev/null | tr '\n' ' ' || true)
    queue=$(echo "$queue $children" | xargs)
  done
  echo "$all" | xargs
}

# The 127.0.0.1 ports any of the given processes listens on.
loopback_ports() {
  local pids
  pids=$(echo "$*" | tr ' ' ',')
  lsof -nP -a -p "$pids" -iTCP -sTCP:LISTEN -Fn 2>/dev/null |
    sed -n 's/^n127\.0\.0\.1:\([0-9][0-9]*\)$/\1/p' | sort -u || true
}

json_field() {
  python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$1" "$2"
}

# 1. Install at the default location, or under CELL_ROOT.
case "$system" in
  Darwin)
    mount=$(mktemp -d)
    hdiutil attach -nobrowse -readonly -mountpoint "$mount" "$installer" >"$logs/hdiutil.log" 2>&1 ||
      fail "hdiutil attach $installer failed; see $logs/hdiutil.log"
    setup="$mount/Runvil Installer.app/Contents/MacOS/Runvil Installer"
    ;;
  Linux)
    chmod +x "$installer"
    setup=$installer
    ;;
esac
[ -x "$setup" ] || fail "the installer executable $setup is missing"
set -- --accept-licenses --default-answer --no-default-installations --confirm-command
if [ -n "${CELL_ROOT:-}" ]; then
  set -- --root "$CELL_ROOT" "$@"
fi
# shellcheck disable=SC2086 # the component ids are separate words
set -- "$@" install $ids
echo "cell: installing$ids into $root"
status=0
"$setup" "$@" >"$logs/installer.log" 2>&1 || status=$?
if [ "$status" -ne 0 ]; then
  tail -n 40 "$logs/installer.log" >&2
  fail "the installer exited $status, required 0; full output in $logs/installer.log"
fi
if [ -n "$mount" ]; then
  hdiutil detach "$mount" -quiet || true
  mount=""
fi

# 2. Exactly the selected components are present, and the install wrote
# nothing into the profile: the Gateway writes its config, speech included,
# the first time it starts.
check_presence() {
  local name=$1 wanted=$2 path=$3
  if [ "$wanted" = 1 ] && [ ! -e "$path" ]; then
    fail "$name was selected, but $path is missing"
  fi
  if [ "$wanted" = 0 ] && [ -e "$path" ]; then
    fail "$name was not selected, but $path exists"
  fi
}
check_presence Workshop "$want_workshop" "$workshop_item"
check_presence Gateway "$want_gateway" "$gateway_item"
if [ "$system" = Linux ]; then
  check_presence "Workshop's desktop entry" "$want_workshop" "$applications/com.runvil.workshop.desktop"
  check_presence "the Gateway's desktop entry" "$want_gateway" "$applications/com.runvil.gateway.desktop"
  for entry in "$applications"/com.runvil.*.desktop; do
    [ -e "$entry" ] || continue
    desktop-file-validate "$entry" || fail "$entry fails desktop-file-validate"
    exec_path=$(sed -n 's/^Exec="\([^"]*\)".*/\1/p' "$entry")
    [ -n "$exec_path" ] || fail "$entry has no quoted Exec path"
    [ -x "$exec_path" ] || fail "$entry runs $exec_path, which is not an executable file"
    icon=$(sed -n 's/^Icon=//p' "$entry")
    [ -f "$icon" ] || fail "$entry names the icon $icon, which does not exist"
  done
fi

config="$profile/gateway.toml"
[ ! -e "$config" ] || fail "the install must leave the Gateway config to the Gateway's first start, but $config exists"
model_files=$(find "$profile/models" -type f 2>/dev/null || true)
[ -z "$model_files" ] || fail "the install must download nothing, but $profile/models holds: $model_files"

# 3. Every present executable prints the version.
check_version() {
  local exe=$1 expected=$2 output
  if ! output=$("$exe" --version 2>"$logs/version.stderr.log"); then
    tail -n 20 "$logs/version.stderr.log" >&2
    fail "$exe --version failed; full output in $logs/version.stderr.log"
  fi
  [ "$output" = "$expected" ] || fail "$exe --version printed '$output', required '$expected'"
}
[ "$want_workshop" = 0 ] || check_version "$workshop_exe" "runvil-workshop $version"
[ "$want_gateway" = 0 ] || check_version "$gateway_exe" "runvil-gateway $gateway_version"

# 4. Workshop serves its page, attached to the installed Gateway when there
# is one and to the fixture endpoint otherwise.
gateway_origin=""
if [ "$want_workshop" = 1 ]; then
  mkdir -p "$profile"
  if [ "$want_gateway" = 0 ]; then
    cat >"$profile/workshop.toml" <<'EOF'
[gateway]
base_url = "http://127.0.0.1:1"
api_key = "release-smoke"
EOF
  fi
  if [ "$system" = Linux ]; then
    pid_file=$(mktemp)
    # shellcheck disable=SC2016 # the inner sh expands them
    xvfb-run -a sh -c 'echo $$ > "$1"; exec "$2"' sh "$pid_file" "$workshop_exe" \
      >"$logs/workshop.stdout.log" 2>"$logs/workshop.stderr.log" &
    started="$started $!"
    for _ in $(seq 1 30); do
      [ -s "$pid_file" ] && break
      sleep 1
    done
    workshop_pid=$(cat "$pid_file")
    [ -n "$workshop_pid" ] || fail "xvfb-run never started Workshop; see $logs/workshop.stderr.log"
  else
    "$workshop_exe" >"$logs/workshop.stdout.log" 2>"$logs/workshop.stderr.log" &
    workshop_pid=$!
  fi
  started="$started $workshop_pid"
  workshop_port=""
  for _ in $(seq 1 120); do
    if ! kill -0 "$workshop_pid" 2>/dev/null; then
      tail -n 40 "$logs/workshop.stdout.log" "$logs/workshop.stderr.log" >&2
      fail "Workshop exited before serving its page"
    fi
    for port in $(loopback_ports "$(process_tree "$workshop_pid")"); do
      if curl -sf --max-time 2 "http://127.0.0.1:$port/" -o "$logs/workshop.html" &&
        grep -Fq "<title>Runvil</title>" "$logs/workshop.html"; then
        workshop_port=$port
        break
      fi
    done
    [ -z "$workshop_port" ] || break
    sleep 1
  done
  [ -n "$workshop_port" ] || fail "Workshop never served <title>Runvil</title> on a loopback port within 120 seconds"
  curl -sf --max-time 30 "http://127.0.0.1:$workshop_port/app.js" -o "$logs/app.js" ||
    fail "http://127.0.0.1:$workshop_port/app.js did not answer"
  size=$(wc -c <"$logs/app.js" | tr -d ' ')
  # The minified workbench is about 2.5 MB.
  [ "$size" -gt 0 ] && [ "$size" -lt 3000000 ] || fail "app.js must be under 3000000 bytes; it is $size"
  echo "cell: Workshop serves its page on port $workshop_port"

  if [ "$want_gateway" = 1 ]; then
    discovery="$profile/run/gateway.json"
    installed=$(canonical "$gateway_exe")
    gateway_port=""
    found="no discovery file"
    for _ in $(seq 1 120); do
      if [ -f "$discovery" ]; then
        gateway_pid=$(json_field "$discovery" pid 2>/dev/null || true)
        if [ -n "$gateway_pid" ] && kill -0 "$gateway_pid" 2>/dev/null; then
          exe=$(process_exe "$gateway_pid")
          if [ -n "$exe" ] && [ "$(canonical "$exe")" = "$installed" ]; then
            gateway_port=$(json_field "$discovery" port) || fail "$discovery names no port"
            started="$started $gateway_pid"
            break
          fi
          found="pid $gateway_pid running $exe"
        else
          found="pid ${gateway_pid:-none}, not running"
        fi
      fi
      sleep 1
    done
    [ -n "$gateway_port" ] || fail "$discovery must name a live Gateway running $installed; found $found"
    gateway_origin="http://127.0.0.1:$gateway_port"
    reported=""
    for _ in $(seq 1 30); do
      reported=$(curl -sf "http://127.0.0.1:$workshop_port/gateway/origin" | python3 -c 'import json, sys; print(json.load(sys.stdin)["origin"])' || true)
      [ "$reported" = "$gateway_origin" ] && break
      sleep 1
    done
    [ "$reported" = "$gateway_origin" ] || fail "Workshop's /gateway/origin must be $gateway_origin, the discovery file's; it is '$reported'"
    echo "cell: Workshop is attached to the installed Gateway at $gateway_origin"
  fi
fi

# 5. The Gateway serves its Settings page. Nothing launched it without
# Workshop, so the cell starts it, from a directory holding no config.
if [ "$want_gateway" = 1 ]; then
  if [ -z "$gateway_origin" ]; then
    workdir=$(mktemp -d)
    (cd "$workdir" && exec "$gateway_exe" --print-url) \
      >"$logs/gateway.stdout.log" 2>"$logs/gateway.stderr.log" &
    gateway_pid=$!
    started="$started $gateway_pid"
    # The log shares stdout with the URL, which is a line of its own.
    url=""
    for _ in $(seq 1 120); do
      url=$(grep -m 1 -E '^http://127\.0\.0\.1:[0-9]+/' "$logs/gateway.stdout.log" || true)
      [ -n "$url" ] && break
      kill -0 "$gateway_pid" 2>/dev/null || break
      sleep 1
    done
    if [ -z "$url" ]; then
      tail -n 40 "$logs/gateway.stdout.log" "$logs/gateway.stderr.log" >&2
      fail "the Gateway must print its http://127.0.0.1 URL under --print-url within 120 seconds; it printed none"
    fi
    gateway_origin=$(echo "$url" | sed -E 's#^(http://[^/]+).*#\1#')
  fi
  curl -sf --max-time 10 "$gateway_origin/config/" -o "$logs/gateway-config.html" ||
    fail "$gateway_origin/config/ did not answer"
  grep -Fq "<title>Runvil Gateway Config</title>" "$logs/gateway-config.html" ||
    fail "$gateway_origin/config/ lacks <title>Runvil Gateway Config</title>"
  echo "cell: the Gateway serves /config/ at $gateway_origin"
fi

# 6. Linux: the AppImage carries the GStreamer plugins Workshop's microphone
# needs, so dictation does not depend on the system's GStreamer.
if [ "$system" = Linux ] && [ "$want_workshop" = 1 ]; then
  extracted=$(mktemp -d)
  (cd "$extracted" && "$workshop_exe" --appimage-extract >"$logs/appimage-extract.log" 2>&1) ||
    fail "$workshop_exe --appimage-extract failed; output in $logs/appimage-extract.log"
  for plugin in libgstpulseaudio.so libgstalsa.so libgstautodetect.so libgstaudioconvert.so; do
    [ -f "$extracted/squashfs-root/usr/lib/gstreamer-1.0/$plugin" ] ||
      fail "the AppImage lacks usr/lib/gstreamer-1.0/$plugin"
  done
  grep -rqs GST_PLUGIN_SYSTEM_PATH_1_0 "$extracted/squashfs-root/apprun-hooks" ||
    fail "no AppImage AppRun hook sets GST_PLUGIN_SYSTEM_PATH_1_0"
  rm -rf "$extracted"
  echo "cell: the AppImage carries its GStreamer audio plugins"
fi

echo "cell: $selection passed"
