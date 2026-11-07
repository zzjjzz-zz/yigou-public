#!/usr/bin/env bash
# Retain one controlled Windows candidate build per version, with visible logs.
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
root="$YIGOU_ROOT/builds/windows"
mkdir -p "$root"
status="$root/ui-0.2.3.status"
pid_file="$root/ui-0.2.3.pid"
if [[ -s "$pid_file" ]] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
  printf 'WINDOWS_UI_BUILD_ALREADY_RUNNING pid=%s\n' "$(cat "$pid_file")"
  exit 0
fi
rm -f "$status"
setsid bash -c '
  set +e
  bash "$1/scripts/build-windows.sh" > "$2/ui-0.2.3.log" 2>&1
  code=$?
  printf "%s\n" "$code" > "$2/ui-0.2.3.status"
  exit "$code"
' _ "$YIGOU_ROOT" "$root" </dev/null >/dev/null 2>&1 &
echo "$!" > "$pid_file"
printf 'WINDOWS_UI_BUILD_STARTED pid=%s\n' "$(cat "$pid_file")"
