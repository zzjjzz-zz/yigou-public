#!/usr/bin/env bash
# Build a native Windows PE SSH worker with Windows CPython + PyInstaller under Wine.
# The Linux PyInstaller artifact is never reused for the Windows bundle.
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
cd "$YIGOU_ROOT"
py_version="3.12.10"
# Supply-chain pin: SHA256 of python-3.12.10-embed-amd64.zip, verified 2026-11-07
# by re-downloading from https://www.python.org/ftp/python/3.12.10/ (HTTP 200,
# bytes=11133606) and comparing byte-identical with the previous cache.
# Any change to this value must be re-verified against python.org before pushing it.
py_embed_sha256="4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3"
root="$YIGOU_ROOT/builds/windows"
runtime="$root/python-$py_version-win64"
dist="$root/worker-dist"
spec="$root/worker-spec"
work="$root/worker-build"
wineprefix="$root/wine64"
archive="$root/python-$py_version-embed-amd64.zip"
target="$YIGOU_ROOT/app/src-tauri/binaries/yigou-worker-x86_64-pc-windows-msvc.exe"
mkdir -p "$root" "$dist" "$spec" "$work" "$wineprefix"
verify_archive() {
  local actual
  actual="$(sha256sum "$archive" | cut -d' ' -f1)"
  if [[ "$actual" != "$py_embed_sha256" ]]; then
    echo "PYTHON_EMBED_SHA256_MISMATCH: got $actual, want $py_embed_sha256" >&2
    return 1
  fi
}
if [[ ! -f "$archive" ]]; then
  curl --fail --location --retry 3 --connect-timeout 15 --max-time 150 \
    "https://www.python.org/ftp/python/$py_version/python-$py_version-embed-amd64.zip" -o "$archive"
fi
verify_archive
# Reconstruct generated runtime from verified bytes on every build; a valid ZIP
# alone cannot authenticate a previously extracted or modified runtime.
rm -rf -- "$runtime"
mkdir -p "$runtime/Lib/site-packages"
unzip -tq "$archive"
unzip -oq "$archive" -d "$runtime"
# Windows embeddable CPython disables site-packages by default. Explicitly enable
# the Windows target wheels installed below without reading host Python packages.
printf '%s\n' 'python312.zip' '.' 'Lib\site-packages' 'import site' > "$runtime/python312._pth"
# Resolve only from the reviewed lock and keep wheel hashes for Windows.
# --require-hashes verifies each downloaded wheel, including transitive deps.
uv lock --project worker --check --locked
locked_requirements="$root/worker-locked-requirements.txt"
uv export --project worker --frozen --group dev --no-emit-project \
  --format requirements-txt --output-file "$locked_requirements"
uv pip install --python /usr/bin/python3 --python-version 3.12 \
  --python-platform x86_64-pc-windows-msvc \
  --target "$runtime/Lib/site-packages" --only-binary :all: --require-hashes \
  --requirement "$locked_requirements"
export WINEPREFIX="$wineprefix"
export WINEARCH=win64
export WINEDEBUG=-all
# UI-less subprocess; Xvfb is only for Wine initialization, not application testing.
xvfb-run -a wine "$runtime/python.exe" -c \
  "import sys,netmiko,paramiko,PyInstaller; print('WINDOWS_PYTHON_WORKER_DEPS_READY',sys.version.split()[0],netmiko.__version__)"
source_path="$(xvfb-run -a winepath -w "$YIGOU_ROOT/worker/src" 2>/dev/null | head -n1 | tr -d '\r')"
entry_path="$(xvfb-run -a winepath -w "$YIGOU_ROOT/worker/entry.py" 2>/dev/null | head -n1 | tr -d '\r')"
dist_path="$(xvfb-run -a winepath -w "$dist" 2>/dev/null | head -n1 | tr -d '\r')"
spec_path="$(xvfb-run -a winepath -w "$spec" 2>/dev/null | head -n1 | tr -d '\r')"
work_path="$(xvfb-run -a winepath -w "$work" 2>/dev/null | head -n1 | tr -d '\r')"
xvfb-run -a wine "$runtime/python.exe" -m PyInstaller \
  --clean --noconfirm --onefile --name yigou-worker \
  --paths "$source_path" --collect-all netmiko --collect-all ntc_templates \
  --distpath "$dist_path" --workpath "$work_path" --specpath "$spec_path" \
  "$entry_path"
test -s "$dist/yigou-worker.exe"
file "$dist/yigou-worker.exe"
cp "$dist/yigou-worker.exe" "$target"
printf 'WINDOWS_WORKER_READY %s\n' "$target"
