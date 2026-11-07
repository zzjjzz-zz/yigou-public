import hashlib
import json
import subprocess
import sys

import pytest
from yigou_worker.__main__ import Worker
from yigou_worker.backup import validate_backup
from yigou_worker.discovery import parse_neighbors
from yigou_worker.query import WorkerError, command_for, ensure_output, redact


@pytest.mark.parametrize(
    "output",
    ["% Unrecognized command found", "% Privilege level too low", "% Authorization failed"],
)
def test_device_error_output_is_not_success(output):
    with pytest.raises(WorkerError) as error:
        ensure_output(output)
    assert error.value.code == "DEVICE_COMMAND_FAILED"


@pytest.mark.parametrize(
    "command",
    [
        "system-view",
        "configure terminal",
        "reboot",
        "display version\nreboot",
        "display version; reboot",
        "show version | redirect flash:x",
        "\x03display version",
        "display version && delete /all",
    ],
)
def test_ai_queries_reject_arbitrary_commands(command):
    for vendor in ("huawei", "ruijie", "h3c"):
        with pytest.raises(WorkerError):
            command_for(vendor, command)


@pytest.mark.parametrize("op", ["config", "send_config_set", "shell", "exec", "restore"])
def test_no_write_operation(op):
    with pytest.raises(WorkerError, match="未开放"):
        Worker().dispatch({"op": op})


def test_unknown_vendor_and_query_fail_closed():
    with pytest.raises(WorkerError):
        command_for("unknown", "version")
    with pytest.raises(WorkerError):
        command_for("huawei", "arbitrary")


@pytest.mark.parametrize(
    "output",
    [
        "",
        "% Invalid input detected",
        "#\nsysname LAB\n#",
        "#\nsysname LAB\n---- More ----\nreturn",
        "nothing\nelse\nreturn",
    ],
)
def test_incomplete_backup_rejected(output):
    with pytest.raises(WorkerError):
        validate_backup("huawei", output)


@pytest.mark.parametrize("sensitive", [
    "设备密码：EXAMPLE123",
    "口令：EXAMPLE123",
    "API_KEY=EXAMPLE123",
    "X-API-Key: EXAMPLE123",
    "密码：EXAMPLE123",
])
def test_redaction_masks_localized_and_api_key_labels(sensitive):
    assert redact(sensitive) == "[REDACTED]"


def test_redaction_preserves_structure_but_not_credentials():
    raw = "sysname LAB\npassword cipher x\nsnmp-agent community read y\n-----BEGIN OPENSSH PRIVATE KEY-----\nsecretbody\n-----END OPENSSH PRIVATE KEY-----\nreturn"
    result = redact(raw)
    assert "sysname LAB" in result
    assert "secretbody" not in result
    assert "cipher x" not in result
    assert "read y" not in result


def test_private_raw_config_digest_detects_masked_only_changes_without_exposure():
    class Device:
        def __init__(self, content):
            self.content = content
            self.commands = []

        def send_command(self, command, **kwargs):
            self.commands.append(command)
            return self.content

    original = "#\nsysname LAB\nlocal-user lab password cipher OLD\n#\nreturn"
    updated = original.replace("cipher OLD", "cipher NEW")
    device = Device(original)
    worker = Worker()
    worker.sessions["fixture"] = {"connection": device, "vendor": "huawei", "mode": "device"}
    request = {"op": "query", "session": "fixture", "query": "config"}
    public = worker.dispatch(request)
    assert "raw_sha256" not in public
    first = worker.dispatch({**request, "include_raw_sha256": True})
    assert "OLD" not in first["output"]
    assert first["raw_sha256"] == hashlib.sha256(original.encode()).hexdigest()
    device.content = updated
    second = worker.dispatch({**request, "include_raw_sha256": True})
    assert first["output"] == second["output"]
    assert first["raw_sha256"] != second["raw_sha256"]
    assert "raw_sha256" not in worker.dispatch(request)

    device.commands.clear()
    worker.sessions["fixture"]["vendor"] = "cisco_ios"
    with pytest.raises(WorkerError):
        worker.dispatch({**request, "include_raw_sha256": True})
    assert not device.commands  # No read from an unsupported platform.

def test_unparsed_neighbors_do_not_claim_empty_network():
    result = parse_neighbors("unsupported format with a peer")
    assert result["parsed"] is False and result["note"]
    assert result["raw"] == "unsupported format with a peer"


def test_worker_protocol_rejects_bad_json_and_keeps_running():
    result = subprocess.run(
        [sys.executable, "-m", "yigou_worker"],
        input='invalid\n{"id":"probe","op":"ping"}\n',
        text=True,
        capture_output=True,
        timeout=10,
        check=True,
    )
    replies = [json.loads(line) for line in result.stdout.splitlines()]
    assert replies[0]["ok"] is False
    assert replies[1] == {"id": "probe", "ok": True, "result": {"protocol": 1, "version": "0.2.0"}}
    assert not result.stderr
