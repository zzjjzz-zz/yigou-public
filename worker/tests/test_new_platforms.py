"""Regression for batch-1 read-only platforms: driver ids, whitelists, and the
hard worker-level gates on backup/configure. No device is ever reached."""

import pytest
from netmiko.ssh_dispatcher import CLASS_MAPPER
from yigou_worker.__main__ import Worker
from yigou_worker.backup import validate_backup
from yigou_worker.query import (
    DRIVERS,
    WorkerError,
    can,
    command_for,
    ensure_capability,
)

NEW_PLATFORMS = [
    "cisco_ios",
    "cisco_xe",
    "maipu",
    "raisecom_ros",
    "raisecom_roap",
    "huawei_olt",
    "huawei_smartax",
    "zte_zxros",
]
READ_WRITE = ("huawei", "ruijie", "h3c")


def test_all_eight_new_drivers_are_real_netmiko_ids():
    for vendor in NEW_PLATFORMS:
        device_type = DRIVERS[vendor]["device_type"]
        assert device_type in CLASS_MAPPER, f"{device_type} missing from Netmiko CLASS_MAPPER"
        # Also confirm our vendor key maps 1:1 where names coincide.
        if vendor in CLASS_MAPPER:
            assert device_type == vendor


def test_new_platforms_are_read_only():
    for vendor in NEW_PLATFORMS:
        assert can(vendor, "backup") is False
        assert can(vendor, "configure") is False
    for vendor in READ_WRITE:
        assert can(vendor, "backup") is True
        assert can(vendor, "configure") is True


def test_query_whitelists_match_documented_evidence():
    expected = {
        "cisco_ios": {
            "version": "show version",
            "ports": "show ip interface brief",
            "config": "show running-config",
            "startup": "show startup-config",
            "neighbors": "show lldp neighbors detail",
        },
        "cisco_xe": {
            "version": "show version",
            "ports": "show ip interface brief",
            "config": "show running-config",
            "startup": "show startup-config",
            "neighbors": "show lldp neighbors detail",
        },
        "maipu": {
            "version": "show version",
            "config": "show running-config",
            "startup": "show startup-config",
        },
        "raisecom_ros": {
            "version": "show version",
            "config": "show running-config",
            "startup": "show startup-config",
        },
        "raisecom_roap": {"version": "show version"},
        "huawei_olt": {
            "version": "display version",
            "config": "display current-configuration",
        },
        "huawei_smartax": {
            "version": "display version",
            "config": "display current-configuration",
        },
        "zte_zxros": {"version": "show version"},
    }
    for vendor, table in expected.items():
        assert DRIVERS[vendor]["queries"] == table, vendor
        for kind, command in table.items():
            assert command_for(vendor, kind) == command


def test_unevidenced_queries_fail_closed_without_device():
    denied = {
        "maipu": ("ports", "neighbors"),
        "raisecom_ros": ("ports", "neighbors"),
        "raisecom_roap": ("ports", "config", "neighbors", "startup"),
        "huawei_olt": ("ports", "neighbors", "startup"),
        "huawei_smartax": ("ports", "neighbors", "startup"),
        "zte_zxros": ("ports", "config", "neighbors", "startup"),
    }
    for vendor, kinds in denied.items():
        for kind in kinds:
            with pytest.raises(WorkerError) as error:
                command_for(vendor, kind)
            assert error.value.code == "QUERY_DENIED"


def test_capability_gate_rejects_before_any_device_traffic():
    for vendor in NEW_PLATFORMS:
        with pytest.raises(WorkerError) as backup_error:
            ensure_capability(vendor, "backup", "备份")
        with pytest.raises(WorkerError) as configure_error:
            ensure_capability(vendor, "configure", "配置下发")
        assert backup_error.value.code == "OPERATION_UNSUPPORTED"
        assert configure_error.value.code == "OPERATION_UNSUPPORTED"
    ensure_capability("huawei", "backup", "备份")  # no raise
    ensure_capability("ruijie", "configure", "配置下发")


def test_worker_rejects_backup_and_configure_for_new_platforms_without_connecting():
    class IntactConnection:
        """Any method call proves the gate failed and bytes reached a device."""

        def __getattr__(self, name):
            raise AssertionError(f"gate failed: {name} was reached")

    worker = Worker()
    for vendor in NEW_PLATFORMS:
        worker.sessions["fixture"] = {
            "connection": IntactConnection(),
            "vendor": vendor,
            "mode": "device",
        }
        for op in ("backup", "configure"):
            with pytest.raises(WorkerError) as error:
                worker.dispatch(
                    {
                        "op": op,
                        "session": "fixture",
                        "commands": ["description probe"],
                        "save": False,
                        "checks": [{"query": "config", "contains": ["probe"]}],
                    }
                    if op == "configure"
                    else {"op": op, "session": "fixture"},
                )
            assert error.value.code == "OPERATION_UNSUPPORTED"
        worker.sessions.clear()


def test_worker_query_uses_whitelist_for_new_platforms():
    class Scripted:
        def __init__(self):
            self.sent = []

        def send_command(self, command, **kwargs):
            self.sent.append(command)
            return f"fixture output for {command}"

    worker = Worker()
    connection = Scripted()
    worker.sessions["fixture"] = {
        "connection": connection,
        "vendor": "cisco_xe",
        "mode": "device",
    }
    result = worker.dispatch({"op": "query", "session": "fixture", "query": "ports"})
    assert connection.sent == ["show ip interface brief"]
    assert "show ip interface brief" in result["output"]
    # cisco_xe also allows config/startup/neighbors per its documented evidence
    worker.dispatch({"op": "query", "session": "fixture", "query": "config"})
    assert connection.sent[-1] == "show running-config"
    worker.sessions.clear()
    worker.sessions["fixture"] = {
        "connection": Scripted(),
        "vendor": "zte_zxros",
        "mode": "device",
    }
    with pytest.raises(WorkerError) as error:
        worker.dispatch({"op": "query", "session": "fixture", "query": "config"})
    assert error.value.code == "QUERY_DENIED"
    assert worker.sessions["fixture"]["connection"].sent == []


def test_unknown_vendor_fails_capability_gate():
    with pytest.raises(WorkerError):
        ensure_capability("no-such-vendor", "backup", "备份")
    with pytest.raises(WorkerError):
        command_for("no-such-vendor", "version")


def test_backup_end_marker_table_stays_conservative_for_new_platforms():
    # These formats are documented but the platforms remain backup=False until
    # the scripted save/read-back loop certifies them; marker rules must not
    # crash on the new vendor keys even if reached by mistake.
    for vendor in NEW_PLATFORMS:
        with pytest.raises(WorkerError):
            validate_backup(vendor, "not a config at all")
    cisco = "version 15.2\nhostname LAB\nend"
    assert validate_backup("cisco_ios", cisco) == cisco
    assert validate_backup("cisco_xe", cisco) == cisco
    assert validate_backup("maipu", "hostname LAB\nversion fixture\nend") is not None
    # Original three keep their proven markers.
    huawei = "#\nsysname LAB\n#\nreturn"
    assert validate_backup("huawei", huawei) == huawei
    ruijie = "version fixture\nhostname LAB\nend"
    assert validate_backup("ruijie", ruijie) == ruijie
