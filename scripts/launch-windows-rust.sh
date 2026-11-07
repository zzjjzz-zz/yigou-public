#!/usr/bin/env bash
# Launch and record a project-local Windows cross-compile job.
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
root="$YIGOU_ROOT/builds/windows"
mkdir -p "$root"
if [[ -f "$root/rust-build.pid" ]]; then
  old="$(cat "$root/rust-build.pid")"
  if kill -0 "$old" 2>/dev/null; then
    echo "WINDOWS_RUST_BUILD_ALREADY_RUNNING pid=$old"
    exit 0
  fi
fi
rm -f "$root/rust-build.status"
setsid bash -c '
  set +e
  bash "$1/scripts/build-windows-rust.sh" > "$2/rust-build.log" 2>&1
  status=$?
  printf "%s\n" "$status" > "$2/rust-build.status"
  exit "$status"
' _ "$YIGOU_ROOT" "$root" </dev/null >/dev/null 2>&1 &
pid=$!
printf "%s\n" "$pid" > "$root/rust-build.pid"
echo "WINDOWS_RUST_BUILD_STARTED pid=$pid log=$root/rust-build.log"
