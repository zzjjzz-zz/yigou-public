#!/usr/bin/env bash
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
cd "$YIGOU_ROOT"
uv sync --project worker --frozen
uv run --project worker pyinstaller --clean --noconfirm --onefile --name yigou-worker \
  --paths "$YIGOU_ROOT/worker/src" --collect-all netmiko --collect-all ntc_templates \
  --distpath "$YIGOU_ROOT/builds/worker-dist" --workpath "$YIGOU_ROOT/builds/pyinstaller" \
  --specpath "$YIGOU_ROOT/builds" "$YIGOU_ROOT/worker/entry.py"
YIGOU_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
mkdir -p app/src-tauri/binaries
cp builds/worker-dist/yigou-worker "app/src-tauri/binaries/yigou-worker-$YIGOU_TARGET"
