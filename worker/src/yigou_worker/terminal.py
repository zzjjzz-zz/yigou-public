"""Human-operated SSH PTY. Byte transport, with no command filtering or local echo."""

import base64
from pathlib import Path

import paramiko

from .connection import validate_device
from .query import WorkerError


class TerminalConnection:
    def __init__(self, client, channel):
        self.client = client
        self.channel = channel

    def disconnect(self):
        self.channel.close()
        self.client.close()

    def read(self):
        chunks = []
        remaining = 65536
        while remaining and self.channel.recv_ready():
            chunk = self.channel.recv(min(16384, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        data = b"".join(chunks)
        transport = self.client.get_transport()
        closed = not self.channel.recv_ready() and (
            self.channel.closed
            or self.channel.exit_status_ready()
            or transport is None
            or not transport.is_active()
        )
        return {"data": base64.b64encode(data).decode("ascii"), "closed": closed}

    def write(self, encoded):
        if not isinstance(encoded, str) or len(encoded) > 90000:
            raise WorkerError("TERMINAL_FRAME_LIMIT", "终端单次输入超过传输帧上限")
        try:
            data = base64.b64decode(encoded, validate=True)
        except ValueError:
            raise WorkerError("TERMINAL_ENCODING", "终端输入编码无效") from None
        if len(data) > 65536:
            raise WorkerError("TERMINAL_FRAME_LIMIT", "终端单次输入超过传输帧上限")
        if self.channel.closed:
            raise EOFError("closed channel")
        # Never block the JSONL worker on the SSH send window. The caller keeps
        # only the unaccepted suffix while read RPCs continue draining output.
        try:
            sent = self.channel.send(data) if data and self.channel.send_ready() else 0
        except TimeoutError:
            sent = 0
        return {"sent_bytes": sent}

    def resize(self, cols, rows):
        if (
            not isinstance(cols, int)
            or not isinstance(rows, int)
            or not (1 <= cols <= 1000 and 1 <= rows <= 1000)
        ):
            raise WorkerError("TERMINAL_SIZE", "终端尺寸无效")
        self.channel.resize_pty(width=cols, height=rows)
        return {"cols": cols, "rows": rows}


def open_terminal(device, credential, known_hosts):
    validate_device(device)
    if not Path(known_hosts).is_file():
        raise WorkerError("HOST_KEY_UNTRUSTED", "请先核实并登记 SSH 主机指纹")
    options = {
        "hostname": device["host"],
        "port": device["port"],
        "username": device["username"],
        "timeout": 10,
        "auth_timeout": 15,
        "banner_timeout": 15,
        "channel_timeout": 15,
        "allow_agent": False,
        "look_for_keys": False,
    }
    if credential.get("kind") == "password":
        options["password"] = credential.get("secret", "")
    elif credential.get("kind") == "key":
        path = credential.get("key_path")
        if not path or not Path(path).is_file():
            raise WorkerError("KEY_UNAVAILABLE", "私钥文件不可用")
        options.update(key_filename=path, passphrase=credential.get("secret") or None)
    else:
        raise WorkerError("CREDENTIAL_UNAVAILABLE", "未配置有效认证方式")
    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.RejectPolicy())
    try:
        client.load_host_keys(known_hosts)
        client.connect(**options)
        channel = client.invoke_shell(term="xterm-256color", width=120, height=30)
        channel.settimeout(0)
        return TerminalConnection(client, channel)
    except paramiko.BadHostKeyException:
        client.close()
        raise WorkerError("HOST_KEY_CHANGED", "SSH 主机密钥发生变化，已拒绝连接") from None
    except paramiko.AuthenticationException:
        client.close()
        raise WorkerError("AUTH_FAILED", "设备认证失败，请检查系统凭据库中的凭据") from None
    except TimeoutError:
        client.close()
        raise WorkerError("CONNECTION_TIMEOUT", "终端连接或登录超时") from None
    except (paramiko.SSHException, OSError, ValueError):
        client.close()
        raise WorkerError("SSH_FAILED", "SSH 握手失败、主机密钥未受信任或终端不可用") from None
