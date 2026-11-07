import hashlib
import json
import sys
import uuid

from . import connection
from .backup import validate_backup
from .configure import configure
from .discovery import parse_neighbors
from .query import (
    WorkerError,
    command_for,
    ensure_capability,
    ensure_output,
    redact,
)
from .terminal import open_terminal

MAX_FRAME = 12 * 1024 * 1024


class Worker:
    def __init__(self):
        self.sessions = {}

    def close(self, session):
        entry = self.sessions.pop(session, None)
        if entry:
            try:
                entry["connection"].disconnect()
            except Exception:  # noqa: BLE001,S110 -- teardown must not expose device exception text
                pass

    def dispatch(self, request: dict):
        op = request.get("op")
        if op == "ping":
            return {"protocol": 1, "version": "0.2.0"}
        if op == "probe":
            return connection.probe_host_key(request["device"])
        if op in ("connect", "terminal_open"):
            if len(self.sessions) >= 16:
                raise WorkerError("SESSION_LIMIT", "最多允许 16 个 SSH 会话")
            device = request["device"]
            factory = open_terminal if op == "terminal_open" else connection.connect
            conn = factory(device, request["credential"], request["known_hosts"])
            session = str(uuid.uuid4())
            self.sessions[session] = {
                "connection": conn,
                "vendor": device["vendor"],
                "mode": "terminal" if op == "terminal_open" else "device",
            }
            return {
                "session": session,
                "prompt": "" if op == "terminal_open" else redact(conn.find_prompt()),
                "vendor": device["vendor"],
            }
        if op == "close":
            self.close(request.get("session"))
            return {"closed": True}
        terminal_ops = ("terminal_read", "terminal_write", "terminal_resize")
        if op not in ("query", "backup", "discover", "configure", *terminal_ops):
            raise WorkerError("OPERATION_DENIED", "未开放的设备操作")
        session = request.get("session")
        entry = self.sessions.get(session)
        if not entry:
            raise WorkerError("NOT_CONNECTED", "设备未连接或会话已失效")
        if (op in terminal_ops) != (entry["mode"] == "terminal"):
            raise WorkerError("SESSION_SCOPE", "终端会话与 AI 查询会话不能混用")
        if op in terminal_ops:
            try:
                conn = entry["connection"]
                if op == "terminal_write":
                    return conn.write(request.get("data"))
                if op == "terminal_resize":
                    return conn.resize(request.get("cols"), request.get("rows"))
                result = conn.read()
                if result["closed"]:
                    self.close(session)
                return result
            except WorkerError:
                raise
            except Exception:  # noqa: BLE001 -- no terminal input/output in exception text
                self.close(session)
                raise WorkerError("TERMINAL_IO_FAILED", "SSH 终端已断开或传输失败") from None
        vendor = entry["vendor"]
        private_config_evidence = op == "query" and request.get("include_raw_sha256") is True
        if private_config_evidence:
            if request.get("query") != "config":
                raise WorkerError("QUERY_DENIED", "私有配置证据仅支持当前运行配置")
            ensure_capability(vendor, "backup", "配置基线证据读取")
        if op in ("backup", "configure"):
            # Hard capability gate: unsupported platforms are rejected before any
            # command bytes are sent to the device, regardless of UI state.
            ensure_capability(vendor, "backup" if op == "backup" else "configure", "备份" if op == "backup" else "配置下发")
        if op == "configure":
            result = configure(entry["connection"], vendor, request)
            if result["status"] == "unknown":
                self.close(session)
            return result
        kind = (
            "config"
            if op == "backup"
            else "neighbors"
            if op == "discover"
            else request.get("query")
        )
        command = command_for(vendor, kind)
        try:
            output = ensure_output(entry["connection"].send_command(command, read_timeout=45))
        except WorkerError:
            raise
        except Exception:  # noqa: BLE001 -- library exceptions can contain credentials; fixed error only
            self.close(session)
            raise WorkerError("QUERY_FAILED", "设备查询失败或超时，会话已关闭") from None
        if op == "backup":
            content = validate_backup(vendor, output)
            # Both views derive from one read, so proposal evidence cannot
            # accidentally bind to a different live configuration snapshot.
            return {"content": content,
                    "redacted_sha256": hashlib.sha256(redact(content).encode()).hexdigest(),
                    "command": command}
        result = {"command": command, "output": redact(output)}
        if private_config_evidence:
            # Host-private evidence only: the Rust AI tool strips the digest
            # before exposing the redacted output to the model or event bus.
            validate_backup(vendor, output)
            result["raw_sha256"] = hashlib.sha256(output.encode()).hexdigest()
        if kind == "neighbors":
            result.update(parse_neighbors(result["output"]))
        return result


def main():
    # The worker protocol is UTF-8 JSON-lines on every platform. Windows console
    # pipes often default to cp1252, which cannot encode Chinese error messages.
    # stdout is protocol-only; stdin is read as raw bytes below.
    sys.stdout.reconfigure(encoding="utf-8", errors="strict", newline="\n")
    worker = Worker()
    try:
        while True:
            line = sys.stdin.buffer.readline(MAX_FRAME + 1)
            if not line:
                break
            request_id = None
            try:
                if len(line) > MAX_FRAME:
                    # Do not interpret leftover bytes as another frame.
                    raise WorkerError("FRAME_LIMIT", "请求超过协议上限")
                request = json.loads(line)
                request_id = request.get("id")
                result = worker.dispatch(request)
                response = {"id": request_id, "ok": True, "result": result}
            except WorkerError as exc:
                response = {
                    "id": request_id,
                    "ok": False,
                    "error": {"code": exc.code, "message": str(exc)},
                }
            except Exception:  # noqa: BLE001 -- private RPC boundary never serializes raw exceptions
                response = {
                    "id": request_id,
                    "ok": False,
                    "error": {
                        "code": "WORKER_ERROR",
                        "message": "设备执行程序失败；未返回敏感异常内容",
                    },
                }
            sys.stdout.write(json.dumps(response, ensure_ascii=False) + "\n")
            sys.stdout.flush()
            if len(line) > MAX_FRAME:
                break
    finally:
        for session in list(worker.sessions):
            worker.close(session)


if __name__ == "__main__":
    main()
