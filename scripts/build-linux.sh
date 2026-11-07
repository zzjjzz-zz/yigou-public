#!/usr/bin/env bash
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
cd "$YIGOU_ROOT"
bash scripts/build-worker.sh
uv run --project worker python scripts/licenses.py
pnpm --dir app install --frozen-lockfile --store-dir "$YIGOU_ROOT/builds/pnpm-store"
cd app
pnpm tauri build --bundles deb
cd "$YIGOU_ROOT"
