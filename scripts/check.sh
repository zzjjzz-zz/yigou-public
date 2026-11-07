#!/usr/bin/env bash
set -euo pipefail
source "$(dirname -- "$0")/env.sh"
cd "$YIGOU_ROOT"
uv sync --project worker --frozen
pnpm --dir app install --frozen-lockfile --store-dir "$YIGOU_ROOT/builds/pnpm-store"
uv run --project worker ruff check --config worker/pyproject.toml worker tests/lab_server.py
uv run --project worker pytest -q worker/tests
cargo fmt --manifest-path app/src-tauri/Cargo.toml --check
cargo test --manifest-path app/src-tauri/Cargo.toml --lib --locked
cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-features --all-targets --locked -- -D warnings
pnpm --dir app build
pnpm --dir app test
