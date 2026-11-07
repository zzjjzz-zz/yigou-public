"""A local scripted SSH peer, never a claim of vendor hardware validation."""

import socket
import threading
from types import SimpleNamespace

import paramiko
import pytest


class SSHServer(paramiko.ServerInterface):
    def __init__(self, user_key, peer):
        self.user_key = user_key
        self.peer = peer
        self.shell_ready = threading.Event()

    def check_auth_publickey(self, username, key):
        return (
            paramiko.AUTH_SUCCESSFUL
            if username == "lab" and key == self.user_key
            else paramiko.AUTH_FAILED
        )

    def check_auth_password(self, username, password):
        return (
            paramiko.AUTH_SUCCESSFUL
            if (username, password) == ("lab", "fixture-only")
            else paramiko.AUTH_FAILED
        )

    def get_allowed_auths(self, username):
        return "password,publickey"

    def check_channel_request(self, kind, chanid):
        return (
            paramiko.OPEN_SUCCEEDED
            if kind == "session"
            else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
        )

    def check_channel_pty_request(self, channel, term, width, height, *args):
        self.peer.sizes.append((width, height))
        return True

    def check_channel_window_change_request(self, channel, width, height, *args):
        self.peer.sizes.append((width, height))
        return True

    def check_channel_shell_request(self, channel):
        self.shell_ready.set()
        return True


@pytest.fixture
def ssh_peer(tmp_path):
    key = paramiko.RSAKey.generate(2048)
    user_key = paramiko.RSAKey.generate(2048)
    key_path = tmp_path / "fixture-user-key"
    user_key.write_private_key_file(str(key_path), password="fixture-passphrase")
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(8)
    listener.settimeout(0.2)
    port = listener.getsockname()[1]
    stop = threading.Event()
    connections = []
    commands = []
    outputs = {
        "display version": "Huawei Versatile Routing Platform Software\nVRP (R) software, Version fixture",
        "show version": "Ruijie Operating System Software\nVersion fixture",
        "display interface brief": "Interface PHY Protocol\nGE0/0/1 up up",
        "show interfaces status": "Port Status\nGi0/1 connected",
        "display current-configuration": "#\nsysname LAB\nlocal-user lab password cipher fixture-sensitive\n#\nreturn",
        "show running-config": "version fixture\nhostname LAB\nusername lab secret fixture-sensitive\nend",
        "display lldp neighbor verbose": "GE0/0/1 has 1 neighbor(s):\nSystem name : PEER\nPort ID : GE0/0/2\nChassis ID : aabb-ccdd-eeff",
        "display lldp neighbor-information verbose": "LLDP neighbor-information of port 1:\nSystem name : PEER\nPort ID : 2",
        "show lldp neighbors detail": "Local Interface: Gi0/1\nSystem Name: PEER\nPort id: Gi0/2",
    }
    peer = SimpleNamespace(
        port=port,
        key=key,
        vendor="huawei",
        outputs=outputs,
        commands=commands,
        input_bytes=bytearray(),
        sizes=[],
        saved=[],
        description="",
        existing_interfaces=set(),
        startup={},
        ignore_commands=set(),
        fail_save=False,
        user_key_path=str(key_path),
    )

    def serve(sock):
        transport = paramiko.Transport(sock)
        connections.append(transport)
        try:
            transport.add_server_key(key)
            server = SSHServer(user_key, peer)
            transport.start_server(server=server)
            channel = transport.accept(10)
            if channel is None or not server.shell_ready.wait(10):
                return
            channel.settimeout(0.2)
            prompt = "LAB#" if peer.vendor == "ruijie" else "<LAB>"
            channel.sendall("\r\n" + prompt)
            buffer = bytearray()
            waiting = ""
            previous_cr = False
            escape = False
            while not stop.is_set():
                try:
                    data = channel.recv(4096)
                except TimeoutError:
                    continue
                if not data:
                    break
                peer.input_bytes.extend(data)
                for byte in data:
                    if byte == 3:  # Ctrl+C must reach the remote shell while a command runs.
                        buffer.clear()
                        waiting = ""
                        channel.sendall("^C\r\n" + prompt)
                        continue
                    if byte == 27:
                        escape = True
                        continue
                    if escape:
                        if 64 <= byte <= 126 and byte != 91:
                            escape = False
                        continue
                    if byte == 9:
                        channel.sendall("\r\nTab received\r\n" + prompt)
                        continue
                    if byte in (8, 127):
                        buffer[:] = buffer[:-1]
                        continue
                    if byte not in (10, 13):
                        buffer.append(byte)
                        previous_cr = False
                        continue
                    if byte == 10 and previous_cr:
                        previous_cr = False
                        continue
                    previous_cr = byte == 13
                    line = buffer.decode("utf-8", errors="replace").strip()
                    buffer.clear()
                    commands.append(line)
                    if waiting == "save":
                        waiting = ""
                        if line.lower() == "y" and not peer.fail_save:
                            peer.saved.append(peer.vendor)
                            peer.startup = {
                                key: render(key)
                                for key in ("display current-configuration", "show running-config")
                            }
                            channel.sendall(line + "\r\nSave complete\r\n" + prompt)
                        else:
                            channel.sendall(line + "\r\nSave cancelled\r\n" + prompt)
                        continue
                    if waiting == "secret":
                        waiting = ""
                        channel.sendall("\r\nAuthenticated\r\n" + prompt)
                        continue
                    if line in ("system-view", "configure terminal"):
                        prompt = "LAB(config)#" if peer.vendor == "ruijie" else "[LAB]"
                    elif line.startswith("interface "):
                        prompt = "LAB(config-if)#" if peer.vendor == "ruijie" else "[LAB-GE0/0/1]"
                    elif line in ("return", "end"):
                        prompt = "LAB#" if peer.vendor == "ruijie" else "<LAB>"
                    elif line == "quit":
                        prompt = "[LAB]"
                    elif line.startswith("description ") and line not in peer.ignore_commands:
                        peer.description = line
                    elif line == "bad-command":
                        channel.sendall(line + "\r\n% Invalid command\r\n" + prompt)
                        continue
                    elif line == "drop-connection":
                        channel.close()
                        return
                    elif line in ("write", "save force"):
                        if peer.fail_save:
                            channel.sendall(line + "\r\nSave failed\r\n" + prompt)
                        else:
                            peer.saved.append(peer.vendor)
                            peer.startup = {
                                key: render(key)
                                for key in ("display current-configuration", "show running-config")
                            }
                            channel.sendall(line + "\r\nSave complete\r\n" + prompt)
                        continue
                    elif line in ("save", "write memory"):
                        waiting = "save"
                        channel.sendall(line + "\r\nContinue? [Y/N]:")
                        continue
                    elif line == "ask-secret":
                        waiting = "secret"
                        channel.sendall("\r\nPassword:")
                        continue
                    elif line == "slow-demo":
                        channel.sendall("\r\nRunning until interrupted...")
                        continue
                    elif line == "logout":
                        channel.sendall("\r\nGoodbye\r\n")
                        channel.close()
                        return
                    if line in ("display saved-configuration", "show startup-config"):
                        source = (
                            "show running-config"
                            if line.startswith("show")
                            else "display current-configuration"
                        )
                        output = peer.startup.get(source, outputs[source])
                    else:
                        output = render(line)
                    channel.sendall(line + "\r\n" + output.replace("\n", "\r\n") + "\r\n" + prompt)
        except (EOFError, OSError, paramiko.SSHException):
            pass
        finally:
            transport.close()

    threads = []

    def render(command):
        output = outputs.get(command, "")
        if command in ("display current-configuration", "show running-config"):
            end = "end" if command.startswith("show") else "return"
            lines = []
            if "GE0/0/1" in peer.existing_interfaces:
                lines.append("interface GE0/0/1")
            if peer.description:
                lines.append("interface GE0/0/1\n " + peer.description)
            if lines:
                output = output.removesuffix(end) + "\n".join(lines) + "\n" + end
        return output

    def accept():
        while not stop.is_set():
            try:
                sock, _ = listener.accept()
            except TimeoutError:
                continue
            except OSError:
                break
            thread = threading.Thread(target=serve, args=(sock,), daemon=True)
            threads.append(thread)
            thread.start()

    thread = threading.Thread(target=accept, daemon=True)
    thread.start()
    known = tmp_path / "known_hosts"
    known.write_text(f"[127.0.0.1]:{port} {key.get_name()} {key.get_base64()}\n")
    peer.known_hosts = str(known)
    peer.device = {
        "id": "fixture",
        "host": "127.0.0.1",
        "port": port,
        "username": "lab",
        "vendor": "huawei",
    }
    peer.credential = {"kind": "password", "secret": "fixture-only"}
    yield peer
    stop.set()
    listener.close()
    for connection in connections:
        connection.close()
    thread.join(timeout=2)
    for child in threads:
        child.join(timeout=2)
