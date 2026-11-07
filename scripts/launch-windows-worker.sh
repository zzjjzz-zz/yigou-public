#!/usr/bin/env bash
# Launch the deterministic Windows sidecar build without holding an interactive RPC.
# This is a project-local build job; never touches production or system services.
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
root="$YIGOU_ROOT/builds/windows"
mkdir -p "$root"
if [[ -f "$root/worker-build.pid" ]]; then
  previous="$(cat "$root/worker-build.pid")"
  if kill -0 "$previous" 2>/dev/null; then
    echo "WINDOWS_WORKER_BUILD_ALREADY_RUNNING pid=$previous"
    exit 0
  fi
fi
rm -f "$root/worker-build.status"
setsid bash -c '
  set +e
  # Wine requires valid std handles. Give it a real pseudo-terminal even when
  # the initiating client disconnects from the build service.
  script -q -e -c "bash $1/scripts/build-windows-worker.sh" "$2/worker-build.tty" > "$2/worker-build.log" 2>&1
  status=$?
  printf "%s\n" "$status" > "$2/worker-build.status"
  exit "$status"
' _ "$YIGOU_ROOT" "$root" </dev/null >/dev/null 2>&1 &
pid=$!
printf "%s\n" "$pid" > "$root/worker-build.pid"
echo "WINDOWS_WORKER_BUILD_STARTED pid=$pid log=$root/worker-build.log"
