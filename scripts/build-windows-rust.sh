#!/usr/bin/env bash
# Cross-compile the actual Yigou Tauri/Rust Windows executable using MSVC/LLVM.
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
bash "$YIGOU_ROOT/scripts/setup-windows-cross.sh"
export PATH="$YIGOU_ROOT/builds/windows-tools:/usr/lib/llvm-18/bin:$PATH"
export XWIN_CACHE_DIR="$YIGOU_ROOT/builds/windows/sdk-cache"
export CARGO_BUILD_JOBS=2
export XWIN_HTTP_RETRIES=2
cd "$YIGOU_ROOT"
cargo-xwin build \
  --manifest-path app/src-tauri/Cargo.toml \
  --target x86_64-pc-windows-msvc \
  --features desktop --release --locked --bin yigou
exe="$YIGOU_ROOT/builds/target/x86_64-pc-windows-msvc/release/yigou.exe"
test -s "$exe"
file "$exe"
echo "WINDOWS_RUST_MAIN_READY $exe"
