#!/usr/bin/env bash
# Pin cargo-xwin to a verified release; keep every generated file inside Yigou.
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
cd "$YIGOU_ROOT"
version="0.23.1"
archive="cargo-xwin-v$version.x86_64-unknown-linux-musl.tar.gz"
checksum="c492c6dfb7e5ac0eee586b796e0fc950ba13077e7f0bbb046f445d71790d5360"
tools="$YIGOU_ROOT/builds/windows-tools"
mkdir -p "$tools" "$YIGOU_ROOT/builds/windows"
if [[ ! -x "$tools/cargo-xwin" ]]; then
  curl --fail --location --retry 3 --connect-timeout 15 --max-time 180 \
    "https://github.com/rust-cross/cargo-xwin/releases/download/v$version/$archive" -o "$tools/$archive"
  printf '%s  %s\n' "$checksum" "$tools/$archive" | sha256sum -c -
  tar -tzf "$tools/$archive"
  tar -xzf "$tools/$archive" -C "$tools"
fi
"$tools/cargo-xwin" --version
for binary in llvm-rc lld-link clang; do
  if [[ ! -x "/usr/lib/llvm-18/bin/$binary" ]]; then
    echo "Missing required LLVM tool /usr/lib/llvm-18/bin/$binary" >&2
    exit 1
  fi
done
# LLVM's clang switches to the Microsoft command-line driver via argv[0].
# Ubuntu distributes clang-18 without a clang-cl symlink; keep one project-local.
if [[ ! -e "$tools/clang-cl" ]]; then
  ln -s /usr/lib/llvm-18/bin/clang "$tools/clang-cl"
fi
export PATH="$tools:/usr/lib/llvm-18/bin:$PATH"
if ! command -v makensis >/dev/null; then
  echo "NSIS is required for the Linux-to-Windows setup.exe build" >&2
  exit 1
fi
rustup target list --installed | grep -Fx x86_64-pc-windows-msvc
echo "WINDOWS_CROSS_TOOLS_READY"
