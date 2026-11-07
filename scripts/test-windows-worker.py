#!/usr/bin/env python3
"""Validate the actual frozen Windows worker under Wine with no device network access."""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PROGRAM = ROOT / "app/src-tauri/binaries/yigou-worker-x86_64-pc-windows-msvc.exe"
assert PROGRAM.is_file() and PROGRAM.stat().st_size > 100_000, "Windows PE worker missing"
ENV = os.environ | {
    "WINEPREFIX": str(ROOT / "builds/windows/wine64"),
    "WINEARCH": "win64",
    "WINEDEBUG": "-all",
}
REQUESTS = (
    {"id": "worker-ping", "op": "ping"},
    {"id": "denied-operation", "op": "unsupported"},
    {"id": "no-session", "op": "query", "query": "version", "session": "not-a-session"},
)
source = "".join(json.dumps(row, separators=(",", ":")) + "\n" for row in REQUESTS)
result = subprocess.run(
    ["wine", str(PROGRAM)],
    input=source,
    text=True,
    capture_output=True,
    timeout=120,
    env=ENV,
    check=False,
)
lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
if result.returncode != 0 or len(lines) != len(REQUESTS):
    raise SystemExit(
        f"Windows worker smoke test failed: rc={result.returncode}, "
        f"lines={len(lines)}, stdout={result.stdout[:1200]!r}, stderr={result.stderr[:1200]!r}"
    )
messages = [json.loads(line) for line in lines]
assert messages[0]["id"] == "worker-ping" and messages[0]["ok"] is True, messages[0]
assert messages[0]["result"]["protocol"] == 1, messages[0]
assert messages[1]["id"] == "denied-operation" and messages[1]["ok"] is False, messages[1]
assert messages[1]["error"]["code"] == "OPERATION_DENIED", messages[1]
assert messages[2]["id"] == "no-session" and messages[2]["ok"] is False, messages[2]
assert messages[2]["error"]["code"] == "NOT_CONNECTED", messages[2]
print(
    json.dumps(
        {
            "windows_worker": "PASS",
            "protocol_ping": "PASS",
            "disallowed_operation": "PASS",
            "unknown_session_rejected": "PASS",
            "wine_runtime": "PASS",
            "real_device": "NOT_RUN",
        },
        ensure_ascii=False,
    )
)
