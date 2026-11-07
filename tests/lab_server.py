"""Local integration fixtures: scripted SSH peer + scripted streaming model.

This is not a real vendor device or an online model acceptance test.
"""

import base64
import hashlib
import json
import pathlib
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

root = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(root / "worker/tests"))
from conftest import ssh_peer  # noqa: E402

requests = []


class Model(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        requests.append(body)
        if self.path.startswith("/fail/"):
            self.send_response(401)
            self.end_headers()
            self.wfile.write(b'{"error":{"message":"fixture authentication failure"}}')
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        messages = body.get("messages", [])
        # Prior turns can contain tool calls; only the active request counts.
        boundary = max((i for i, m in enumerate(messages) if m.get("role") == "user"), default=-1)
        current_turn = messages[boundary + 1:]
        has_tool = any(m.get("role") == "tool" for m in current_turn)
        configuration = self.path.startswith(("/configure/", "/force-configure/", "/weak-configure/"))
        weak_checks = self.path.startswith("/weak-configure/")
        executed_tool = any(
            c.get("function", {}).get("name") == "device_propose_plan"
            for m in current_turn
            for c in m.get("tool_calls", [])
        )
        available = any(
            t.get("function", {}).get("name") == "device_propose_plan"
            for t in body.get("tools", [])
        )
        if configuration and not available and not self.path.startswith("/force-configure/"):
            parts = [{"role":"assistant","content":"本轮普通对话只读；请先生成结构化计划，再人工确认执行。"}]
            finish = "stop"
        elif (
            configuration
            and (available or self.path.startswith("/force-configure/"))
            and not executed_tool
        ):
            tool = (
                "device_configure" if self.path.startswith("/force-configure/")
                else "device_propose_plan" if has_tool else "device_query"
            )
            if tool in ("device_configure", "device_propose_plan"):
                if weak_checks:
                    # A model that tries to certify the change with pre-existing text.
                    args = {
                        "commands": ["interface GE0/0/1", "description AI-WEAK"],
                        "save": True,
                        "checks": [
                            {
                                "query": "config",
                                "contains": ["sysname"],
                                "not_contains": [],
                            }
                        ],
                    }
                else:
                    args = {
                        "commands": ["interface GE0/0/1", "description AI-INTEGRATION"],
                        "save": True,
                        "checks": [
                            {
                                "query": "config",
                                "contains": ["description AI-INTEGRATION"],
                                "not_contains": [],
                            }
                        ],
                    }
            else:
                args = {"query": "config"}
            parts = [
                {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": "config-call"
                            if tool != "device_query"
                            else "identify-call",
                            "type": "function",
                            "function": {"name": tool, "arguments": json.dumps(args)},
                        }
                    ],
                }
            ]
            finish = "tool_calls"
        elif configuration and executed_tool:
            parts = [
                {
                    "role": "assistant",
                    "content": "配置工具已返回，请以执行记录和回读证据为准。",
                }
            ]
            finish = "stop"
        elif has_tool:
            result = next(
                m["content"] for m in reversed(body["messages"]) if m["role"] == "tool"
            )
            assert "GE0/0/1" in result, result
            parts = [
                {"role": "assistant", "content": "依据设备返回："},
                {"content": "GE0/0/1 up up。"},
            ]
            finish = "stop"
        else:
            parts = [
                {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "index": 0,
                            "id": "fixture-call",
                            "type": "function",
                            "function": {
                                "name": "device_query",
                                "arguments": '{"query":',
                            },
                        }
                    ],
                },
                {"tool_calls": [{"index": 0, "function": {"arguments": '"ports"}'}}]},
            ]
            finish = "tool_calls"
        for part in parts:
            chunk = {
                "id": "fixture-completion",
                "object": "chat.completion.chunk",
                "created": 1,
                "model": "fixture-model",
                "choices": [{"index": 0, "delta": part, "finish_reason": None}],
            }
            self.wfile.write(("data: " + json.dumps(chunk) + "\n\n").encode())
            self.wfile.flush()
        chunk = {
            "id": "fixture-completion",
            "object": "chat.completion.chunk",
            "created": 1,
            "model": "fixture-model",
            "choices": [{"index": 0, "delta": {}, "finish_reason": finish}],
        }
        self.wfile.write(
            ("data: " + json.dumps(chunk) + "\n\ndata: [DONE]\n\n").encode()
        )
        self.wfile.flush()

    def do_GET(self):
        data = json.dumps(
            {
                "commands": peer.commands,
                "description": peer.description,
                "saved": peer.saved,
            }
            if self.path == "/device-state"
            else requests
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


with tempfile.TemporaryDirectory(prefix="yigou-lab-") as directory:
    fixture = ssh_peer.__wrapped__(pathlib.Path(directory))
    peer = next(fixture)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Model)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    fingerprint = "SHA256:" + base64.b64encode(
        hashlib.sha256(peer.key.asbytes()).digest()
    ).decode().rstrip("=")
    print(
        json.dumps(
            {
                "port": peer.port,
                "key": peer.key.get_base64(),
                "algorithm": peer.key.get_name(),
                "fingerprint": fingerprint,
                "model_base": f"http://127.0.0.1:{server.server_port}",
            }
        ),
        flush=True,
    )
    try:
        sys.stdin.read()
    finally:
        server.shutdown()
        try:
            next(fixture)
        except StopIteration:
            pass
