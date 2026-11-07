#!/usr/bin/env bash
# Build Windows x64 Tauri + the native Windows SSH sidecar from Linux.
# No system installation or production-device connection happens here.
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
cd "$YIGOU_ROOT"
# Prevent concurrent Tauri builds from overwriting the same NSIS artifact.
mkdir -p "$YIGOU_ROOT/builds/windows"
exec 9>"$YIGOU_ROOT/builds/windows/nsis-build.lock"
if ! flock -n 9; then
  echo "WINDOWS_BUILD_RUNNING: another NSIS build owns the project lock" >&2
  exit 1
fi
bash scripts/setup-windows-cross.sh
worker="$YIGOU_ROOT/app/src-tauri/binaries/yigou-worker-x86_64-pc-windows-msvc.exe"
# Rebuild the sidecar whenever its Python source or manifest is newer.
if [[ ! -s "$worker" ]] || [[ "$YIGOU_ROOT/worker/entry.py" -nt "$worker" ]] \
   || [[ "$YIGOU_ROOT/worker/pyproject.toml" -nt "$worker" ]] \
   || [[ "$YIGOU_ROOT/worker/uv.lock" -nt "$worker" ]] \
   || [[ -n "$(find "$YIGOU_ROOT/worker/src" -type f -name '*.py' -newer "$worker" -print -quit)" ]]; then
  bash "$YIGOU_ROOT/scripts/build-windows-worker.sh"
fi
if [[ ! -s "$worker" ]] || ! file "$worker" | grep -q "PE32+ executable"; then
  echo "Windows PE SSH worker missing: run bash scripts/build-windows-worker.sh" >&2
  exit 1
fi
# Verify that the frozen Windows sidecar speaks the UTF-8 protocol before bundling.
xvfb-run -a python3 scripts/test-windows-worker.py
# Bind the bundled inventory to the current lock files, including a version bump.
uv run --project worker python scripts/licenses.py
export PATH="$YIGOU_ROOT/builds/windows-tools:/usr/lib/llvm-18/bin:$PATH"
export XWIN_CACHE_DIR="$YIGOU_ROOT/builds/windows/sdk-cache"
export XWIN_HTTP_RETRIES=2
export CARGO_BUILD_JOBS=2
cd app
pnpm tauri build \
  --runner cargo-xwin \
  --target x86_64-pc-windows-msvc \
  --config src-tauri/tauri.windows.conf.json \
  --ci
package_dir="$YIGOU_ROOT/builds/target/x86_64-pc-windows-msvc/release/bundle/nsis"
setup="$(find "$package_dir" -maxdepth 1 -iname '*setup.exe' -type f | sort | tail -n 1)"
test -n "$setup" && test -s "$setup"
file "$setup"
7z t "$setup" >/dev/null
embedded_hash="$(7z e -so "$setup" yigou-worker.exe 2>/dev/null | sha256sum | cut -d' ' -f1)"
worker_hash="$(sha256sum "$worker" | cut -d' ' -f1)"
if [[ "$embedded_hash" != "$worker_hash" ]]; then
  echo "WINDOWS_WORKER_MISMATCH: packaged worker differs from the tested worker" >&2
  exit 1
fi
sha256sum "$setup"
printf '%s\n' 'WINDOWS_NSIS_BUNDLE_READY (7z archive and worker SHA256 verified)'
