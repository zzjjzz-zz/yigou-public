#!/usr/bin/env bash
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
cd "$YIGOU_ROOT/app"
exec python3 "$YIGOU_ROOT/scripts/with-local-sdk.py" pnpm tauri dev
