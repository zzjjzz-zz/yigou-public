"""Regression: a small inbound SSH window plus large output must not deadlock paste."""

import base64
import socket
import threading
import time

import paramiko
from yigou_worker.__main__ import Worker


class Server(paramiko.ServerInterface):
    def __init__(self):
        self.ready = threading.Event()

    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL

    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED

    def check_channel_pty_request(self, *args):
        return True

    def check_channel_shell_request(self, channel):
        self.ready.set()
        return True


def test_backpressure_returns_partial_ack_and_drains_output_without_replay(tmp_path):
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(1)
    port = listener.getsockname()[1]
    key = paramiko.RSAKey.generate(2048)
    sent = bytes(range(256)) * 256
    output = b"output\r\n" * (3 * 1024 * 1024 // 8)
    received = bytearray()
    release = threading.Event()
    failures = []

    def serve():
        sock, _ = listener.accept()
        transport = paramiko.Transport(sock, default_window_size=32768)
        try:
            transport.add_server_key(key)
            server = Server()
            transport.start_server(server=server)
            channel = transport.accept(5)
            assert channel and server.ready.wait(5)
            channel.settimeout(10)
            received.extend(channel.recv(8192))
            channel.sendall(output)
            while len(received) < len(sent):
                data = channel.recv(8192)
                assert data
                received.extend(data)
            release.wait(5)
        except Exception as error:
            failures.append(type(error).__name__)
        finally:
            transport.close()

    thread = threading.Thread(target=serve, daemon=True)
    thread.start()
    known = tmp_path / "known_hosts"
    known.write_text(f"[127.0.0.1]:{port} {key.get_name()} {key.get_base64()}\n")
    worker = Worker()
    session = worker.dispatch(
        {
            "op": "terminal_open",
            "device": {"host": "127.0.0.1", "port": port, "username": "lab", "vendor": "huawei"},
            "credential": {"kind": "password", "secret": "fixture"},
            "known_hosts": str(known),
        }
    )["session"]
    try:
        offset, zero_acks = 0, 0
        drained = bytearray()
        deadline = time.monotonic() + 8
        # Reproduce the old write-first burst, then run independent read/write turns.
        for iteration in range(20000):
            assert time.monotonic() < deadline
            if offset < len(sent):
                frame = sent[offset : offset + 8192]
                start = time.monotonic()
                result = worker.dispatch(
                    {
                        "op": "terminal_write",
                        "session": session,
                        "data": base64.b64encode(frame).decode(),
                    }
                )
                assert time.monotonic() - start < 1
                accepted = result["sent_bytes"]
                assert 0 <= accepted <= len(frame)
                zero_acks += accepted == 0
                offset += accepted
            if iteration >= 8:
                result = worker.dispatch({"op": "terminal_read", "session": session})
                drained.extend(base64.b64decode(result["data"]))
                assert not result["closed"]
            if offset == len(sent) and len(drained) == len(output) and len(received) == len(sent):
                break
            time.sleep(0.001)
        assert zero_acks > 0
        assert received == sent
        assert drained == output
        assert session in worker.sessions
        assert not failures
    finally:
        release.set()
        worker.close(session)
        listener.close()
        thread.join(3)
