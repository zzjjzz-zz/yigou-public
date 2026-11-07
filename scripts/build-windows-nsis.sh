#!/usr/bin/env bash
# Backward-compatible entry used by README. The canonical script verifies the
# Windows sidecar, serializes builds, and checks NSIS archive integrity.
set -euo pipefail
repo="$(cd "$(dirname -- "$0")/.." && pwd)"
exec bash "$repo/scripts/build-windows.sh"
