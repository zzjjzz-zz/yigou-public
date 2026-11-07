"""Interactive PTY tests use actual SSH transport and an explicitly scripted peer."""

import base64
import time
from pathlib import Path

import paramiko
import pytest
from yigou_worker.__main__ import Worker
from yigou_worker.query import WorkerError
from yigou_worker.terminal import open_terminal


def read_until(worker, session, expected):
    output = bytearray()
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        result = worker.dispatch({"op": "terminal_read", "session": session})
        output.extend(base64.b64decode(result["data"]))
        if expected in output:
            return bytes(output), result["closed"]
        assert not result["closed"], output
        time.sleep(0.01)
    pytest.fail(f"Missing terminal output {expected!r} in {bytes(output)!r}")


def write(worker, session, data):
    result = worker.dispatch(
        {"op": "terminal_write", "session": session, "data": base64.b64encode(data).decode()}
    )
    assert result["sent_bytes"] == len(data)


@pytest.mark.parametrize("vendor", ["huawei", "ruijie", "h3c"])
def test_interactive_config_save_controls_password_and_remote_close(vendor, ssh_peer):
    peer = ssh_peer
    peer.vendor = peer.device["vendor"] = vendor
    worker = Worker()
    session = worker.dispatch(
        {
            "op": "terminal_open",
            "device": peer.device,
            "credential": peer.credential,
            "known_hosts": peer.known_hosts,
        }
    )["session"]
    try:
        read_until(worker, session, b"LAB#" if vendor == "ruijie" else b"<LAB>")
        command = b"configure terminal" if vendor == "ruijie" else b"system-view"
        write(worker, session, command + b"\r")
        read_until(worker, session, b"LAB(config)#" if vendor == "ruijie" else b"[LAB]")
        # Multiline UTF-8 paste crosses the real SSH channel intact.
        write(worker, session, "interface GE0/0/1\r\ndescription 测试上联\r\n".encode())
        read_until(worker, session, "description 测试上联".encode())
        write(worker, session, b"end\r" if vendor == "ruijie" else b"return\r")
        read_until(worker, session, b"LAB#" if vendor == "ruijie" else b"<LAB>")
        write(worker, session, b"write memory\r" if vendor == "ruijie" else b"save\r")
        read_until(worker, session, b"[Y/N]:")
        write(worker, session, b"y\r")
        read_until(worker, session, b"Save complete")
        assert peer.saved == [vendor]
        write(worker, session, b"\x1b[A\t")
        read_until(worker, session, b"Tab received")
        assert b"\x1b[A\t" in peer.input_bytes
        write(worker, session, b"slow-demo\r")
        read_until(worker, session, b"Running until interrupted")
        write(worker, session, b"\x03")
        read_until(worker, session, b"^C")
        write(worker, session, b"ask-secret\r")
        read_until(worker, session, b"Password:")
        write(worker, session, b"manual-secret-no-local-echo\r")
        output, _ = read_until(worker, session, b"Authenticated")
        assert b"manual-secret-no-local-echo" not in output
        worker.dispatch({"op": "terminal_resize", "session": session, "cols": 90, "rows": 25})
        for _ in range(100):
            if (90, 25) in peer.sizes:
                break
            time.sleep(0.01)
        assert (90, 25) in peer.sizes
        with pytest.raises(WorkerError, match="不能混用"):
            worker.dispatch({"op": "query", "session": session, "query": "config"})
        write(worker, session, b"logout\r")
        read_until(worker, session, b"Goodbye")
        for _ in range(100):
            if session not in worker.sessions:
                break
            worker.dispatch({"op": "terminal_read", "session": session})
            time.sleep(0.01)
        assert session not in worker.sessions
    finally:
        worker.close(session)


@pytest.mark.parametrize("issue", ["unknown", "changed", "password"])
def test_interactive_ssh_auth_and_host_key_verification(issue, ssh_peer, tmp_path):
    peer = ssh_peer
    if issue == "unknown":
        known = tmp_path / "empty"
        known.write_text("")
        peer.known_hosts = str(known)
    elif issue == "changed":
        key = paramiko.RSAKey.generate(2048)
        Path(peer.known_hosts).write_text(
            f"[127.0.0.1]:{peer.port} {key.get_name()} {key.get_base64()}\n"
        )
    else:
        peer.credential["secret"] = "never-include-in-errors"
    with pytest.raises(WorkerError) as error:
        open_terminal(peer.device, peer.credential, peer.known_hosts)
    assert (
        error.value.code
        == {"unknown": "SSH_FAILED", "changed": "HOST_KEY_CHANGED", "password": "AUTH_FAILED"}[
            issue
        ]
    )
    assert "never-include-in-errors" not in str(error.value)


def test_interactive_encrypted_private_key_authentication(ssh_peer):
    peer = ssh_peer
    terminal = open_terminal(
        peer.device,
        {"kind": "key", "key_path": peer.user_key_path, "secret": "fixture-passphrase"},
        peer.known_hosts,
    )
    try:
        assert terminal.client.get_transport().is_authenticated()
    finally:
        terminal.disconnect()


def test_interactive_disconnect_error_is_safe():
    class Broken:
        disconnected = False

        def read(self):
            raise OSError("never-include-terminal-content")

        def disconnect(self):
            self.disconnected = True

    worker = Worker()
    connection = Broken()
    worker.sessions["broken"] = {"connection": connection, "mode": "terminal"}
    with pytest.raises(WorkerError) as error:
        worker.dispatch({"op": "terminal_read", "session": "broken"})
    assert error.value.code == "TERMINAL_IO_FAILED"
    assert "never-include" not in str(error.value)
    assert connection.disconnected and not worker.sessions
