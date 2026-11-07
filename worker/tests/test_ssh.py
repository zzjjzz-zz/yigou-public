from pathlib import Path

import paramiko
import pytest
from yigou_worker.__main__ import Worker
from yigou_worker.connection import connect, probe_host_key
from yigou_worker.query import WorkerError


@pytest.mark.parametrize("vendor", ["huawei", "ruijie", "h3c"])
def test_netmiko_real_ssh_transport_with_scripted_vendor(vendor, ssh_peer):
    peer = ssh_peer
    peer.vendor = vendor
    peer.device["vendor"] = vendor
    worker = Worker()
    result = worker.dispatch(
        {
            "op": "connect",
            "device": peer.device,
            "credential": peer.credential,
            "known_hosts": peer.known_hosts,
        }
    )
    session = result["session"]
    try:
        query = worker.dispatch({"op": "query", "session": session, "query": "ports"})
        assert "up" in query["output"] or "connected" in query["output"]
        config = worker.dispatch({"op": "query", "session": session, "query": "config"})
        assert "fixture-sensitive" not in config["output"]
        backup = worker.dispatch({"op": "backup", "session": session})
        assert "fixture-sensitive" in backup["content"]
        discovery = worker.dispatch({"op": "discover", "session": session})
        assert discovery["neighbors"][0]["remote_name"] == "PEER"
        assert discovery["neighbors"][0]["evidence"] in discovery["raw"]
        with pytest.raises(WorkerError, match="不能混用"):
            worker.dispatch({"op": "terminal_read", "session": session})
        peer.outputs[
            "show running-config" if vendor == "ruijie" else "display current-configuration"
        ] = "partial configuration"
        with pytest.raises(WorkerError):
            worker.dispatch({"op": "backup", "session": session})
        assert not any(
            c in ("system-view", "configure terminal", "write", "save", "reboot")
            for c in peer.commands
        )
    finally:
        worker.close(session)


def test_unknown_host_key_is_rejected(ssh_peer, tmp_path):
    empty = tmp_path / "empty"
    empty.write_text("")
    with pytest.raises((WorkerError, paramiko.SSHException)):
        connect(ssh_peer.device, ssh_peer.credential, str(empty))


def test_changed_host_key_is_rejected(ssh_peer):
    replacement = paramiko.RSAKey.generate(2048)
    Path(ssh_peer.known_hosts).write_text(
        f"[127.0.0.1]:{ssh_peer.port} {replacement.get_name()} {replacement.get_base64()}\n"
    )
    with pytest.raises(WorkerError, match="密钥发生变化"):
        connect(ssh_peer.device, ssh_peer.credential, ssh_peer.known_hosts)


def test_wrong_password_is_rejected_without_secret_in_error(ssh_peer):
    with pytest.raises(WorkerError) as error:
        connect(
            ssh_peer.device, {"kind": "password", "secret": "DO-NOT-LOG-ME"}, ssh_peer.known_hosts
        )
    assert error.value.code == "AUTH_FAILED"
    assert "DO-NOT-LOG-ME" not in str(error.value)


def test_probe_has_no_authentication(ssh_peer):
    result = probe_host_key(ssh_peer.device)
    assert result["key"] == ssh_peer.key.get_base64()
    assert result["fingerprint"].startswith("SHA256:")


def test_encrypted_private_key_authentication(ssh_peer):
    session = connect(
        ssh_peer.device,
        {"kind": "key", "key_path": ssh_peer.user_key_path, "secret": "fixture-passphrase"},
        ssh_peer.known_hosts,
    )
    try:
        assert "Huawei" in session.send_command("display version")
    finally:
        session.disconnect()


def test_connection_timeout_has_safe_error(monkeypatch, ssh_peer):
    from netmiko.exceptions import NetmikoTimeoutException
    from yigou_worker import connection

    def timeout(**kwargs):
        raise NetmikoTimeoutException("unsafe secret echoed by a library")

    monkeypatch.setattr(connection, "ConnectHandler", timeout)
    with pytest.raises(WorkerError) as error:
        connect(ssh_peer.device, ssh_peer.credential, ssh_peer.known_hosts)
    assert error.value.code == "CONNECTION_TIMEOUT"
    assert "unsafe secret" not in str(error.value)


def test_query_failure_closes_session_without_returning_secret():
    class Broken:
        disconnected = False

        def send_command(self, *args, **kwargs):
            raise TimeoutError("unsafe secret echoed by a library")

        def disconnect(self):
            self.disconnected = True

    worker = Worker()
    broken = Broken()
    worker.sessions["test"] = {"connection": broken, "vendor": "huawei", "mode": "device"}
    with pytest.raises(WorkerError) as error:
        worker.dispatch({"op": "query", "session": "test", "query": "ports"})
    assert broken.disconnected and not worker.sessions
    assert "unsafe secret" not in str(error.value)
