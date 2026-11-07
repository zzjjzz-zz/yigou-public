#!/usr/bin/env bash
# Source from build scripts; all generated files stay under the workspace.
YIGOU_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
export YIGOU_ROOT
export CARGO_HOME="$YIGOU_ROOT/builds/cargo-home"
# Locate the installed Rust toolchain without moving generated Cargo caches outside this project.
# rustc/cargo symlinks are often in ~/.cargo/bin, which non-login automation omits from PATH.
if [[ -x "$HOME/.cargo/bin/rustc" && ":$PATH:" != *":$HOME/.cargo/bin:"* ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
export CARGO_TARGET_DIR="$YIGOU_ROOT/builds/target"
export UV_CACHE_DIR="$YIGOU_ROOT/builds/uv-cache"
export TMPDIR="$YIGOU_ROOT/builds/tmp"
export XDG_CACHE_HOME="$YIGOU_ROOT/builds/cache"
mkdir -p "$TMPDIR" "$XDG_CACHE_HOME"
if [[ -d "$YIGOU_ROOT/builds/sysroot/usr" ]]; then
  yigou_prepend_path() {
    local yigou_name="$1" yigou_entry="$2" yigou_previous="${!1:-}"
    case ":$yigou_previous:" in
      *":$yigou_entry:"*) ;;
      *) export "$yigou_name=$yigou_entry${yigou_previous:+:$yigou_previous}" ;;
    esac
  }
  export PKG_CONFIG_ALLOW_SYSTEM_LIBS=1
  yigou_prepend_path PKG_CONFIG_PATH "$YIGOU_ROOT/builds/sysroot/usr/share/pkgconfig"
  yigou_prepend_path PKG_CONFIG_PATH "$YIGOU_ROOT/builds/sysroot/usr/lib/x86_64-linux-gnu/pkgconfig"
  yigou_prepend_path LD_LIBRARY_PATH "$YIGOU_ROOT/builds/sysroot/usr/lib/x86_64-linux-gnu"
  yigou_prepend_path LIBRARY_PATH "$YIGOU_ROOT/builds/sysroot/usr/lib/x86_64-linux-gnu"
  unset -f yigou_prepend_path
fi
