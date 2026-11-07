import base64
import hashlib
import ipaddress
import re
import socket
from pathlib import Path

import paramiko
from netmiko import ConnectHandler
from netmiko.exceptions import NetmikoAuthenticationException, NetmikoTimeoutException

from .query import DRIVERS, WorkerError


def validate_device(device: dict):
    host = device.get("host", "")
    try:
        ipaddress.ip_address(host)
    except ValueError:
        if not re.fullmatch(r"[a-zA-Z0-9](?:[a-zA-Z0-9.-]{0,251}[a-zA-Z0-9])?", host):
            raise WorkerError("INVALID_DEVICE", "设备地址必须是 IP 或主机名") from None
    if device.get("vendor") not in DRIVERS or not 1 <= device.get("port", 0) <= 65535:
        raise WorkerError("INVALID_DEVICE", "无效的厂商或端口")
    if not device.get("username") or any(ord(c) < 32 for c in device["username"]):
        raise WorkerError("INVALID_DEVICE", "无效用户名")


def probe_host_key(device: dict) -> dict:
    validate_device(device)
    with socket.create_connection((device["host"], device["port"]), timeout=10) as sock:
        transport = paramiko.Transport(sock)
        try:
            transport.start_client(timeout=10)
            key = transport.get_remote_server_key()
            digest = base64.b64encode(hashlib.sha256(key.asbytes()).digest()).decode().rstrip("=")
            return {
                "algorithm": key.get_name(),
                "key": key.get_base64(),
                "fingerprint": "SHA256:" + digest,
            }
        finally:
            transport.close()


def connect(device: dict, credential: dict, known_hosts: str):
    validate_device(device)
    if not Path(known_hosts).is_file():
        raise WorkerError("HOST_KEY_UNTRUSTED", "请先核实并登记 SSH 主机指纹")
    options = {
        "device_type": DRIVERS[device["vendor"]]["device_type"],
        "host": device["host"],
        "port": device["port"],
        "username": device["username"],
        "ssh_strict": True,
        "system_host_keys": False,
        "alt_host_keys": True,
        "alt_key_file": known_hosts,
        "allow_agent": False,
        "use_keys": False,
        "conn_timeout": 10,
        "auth_timeout": 15,
        "banner_timeout": 15,
        "read_timeout_override": 45,
        "session_log": None,
    }
    if credential.get("kind") == "password":
        options["password"] = credential.get("secret", "")
    elif credential.get("kind") == "key":
        options.update(
            use_keys=True,
            key_file=credential.get("key_path"),
            passphrase=credential.get("secret") or None,
        )
        if not options["key_file"] or not Path(options["key_file"]).is_file():
            raise WorkerError("KEY_UNAVAILABLE", "私钥文件不可用")
    else:
        raise WorkerError("CREDENTIAL_UNAVAILABLE", "未配置有效认证方式")
    try:
        return ConnectHandler(**options)
    except NetmikoAuthenticationException:
        raise WorkerError("AUTH_FAILED", "设备认证失败，请检查系统凭据库中的凭据") from None
    except paramiko.BadHostKeyException:
        raise WorkerError("HOST_KEY_CHANGED", "SSH 主机密钥发生变化，已拒绝连接") from None
    except NetmikoTimeoutException as exc:
        # Netmiko wraps Paramiko's SSH failures, including host-key mismatch.
        cause = exc.__cause__ or exc.__context__
        if isinstance(cause, paramiko.BadHostKeyException):
            raise WorkerError("HOST_KEY_CHANGED", "SSH 主机密钥发生变化，已拒绝连接") from None
        if isinstance(cause, paramiko.SSHException):
            raise WorkerError("SSH_FAILED", "SSH 握手失败或主机密钥未受信任") from None
        raise WorkerError("CONNECTION_TIMEOUT", "设备连接或登录超时") from None
    except paramiko.SSHException:
        raise WorkerError("SSH_FAILED", "SSH 握手失败或主机密钥未受信任") from None
