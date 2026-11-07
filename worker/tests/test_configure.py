"""Actual Netmiko writes over local SSH; this does not qualify vendor hardware."""

import json

import pytest
from yigou_worker.__main__ import Worker
from yigou_worker.query import WorkerError


@pytest.fixture
def connected(ssh_peer):
    worker = Worker()
    sessions = []

    def open_device(vendor="huawei"):
        ssh_peer.vendor = ssh_peer.device["vendor"] = vendor
        session = worker.dispatch(
            {
                "op": "connect",
                "device": ssh_peer.device,
                "credential": ssh_peer.credential,
                "known_hosts": ssh_peer.known_hosts,
            }
        )["session"]
        sessions.append(session)
        return worker, session, ssh_peer

    yield open_device
    for session in sessions:
        worker.close(session)


def request(session, **extra):
    return {
        "op": "configure",
        "session": session,
        "commands": ["interface GE0/0/1", "description AI-LAB"],
        "save": True,
        "checks": [{"query": "config", "contains": ["description AI-LAB"], "not_contains": []}],
        **extra,
    }


@pytest.mark.parametrize("vendor", ["huawei", "ruijie", "h3c"])
def test_configure_save_and_read_back_over_actual_ssh(vendor, connected):
    worker, session, peer = connected(vendor)
    result = worker.dispatch(request(session))
    assert result["status"] == "succeeded", result
    assert peer.description == "description AI-LAB"
    assert peer.saved == [vendor]
    assert result["save_status"] == "verified"
    assert result["attempted_commands"] == result["confirmed_commands"] == 2
    assert {check["query"] for check in result["checks"]} == {"config", "startup"}
    assert all(check["passed"] for check in result["checks"])
    assert "fixture-sensitive" in result.pop("after_content")
    assert "fixture-sensitive" not in json.dumps(result)
    output = worker.dispatch({"op": "query", "session": session, "query": "startup"})["output"]
    assert "description AI-LAB" in output


@pytest.mark.parametrize(
    "failure", ["partial", "verification_failed", "save_failed", "applied_unverified", "unknown"]
)
def test_configure_reports_real_failures_without_replaying(failure, connected):
    worker, session, peer = connected()
    args = request(session)
    if failure == "partial":
        args["commands"] += ["bad-command", "description MUST-NOT-RUN"]
    elif failure == "verification_failed":
        peer.ignore_commands.add("description AI-LAB")
    elif failure == "save_failed":
        peer.fail_save = True
    elif failure == "applied_unverified":
        args["checks"] = [{"query": "startup", "contains": ["description AI-LAB"], "not_contains": []}]
    else:
        args["commands"] += ["drop-connection", "description MUST-NOT-RUN"]
    result = worker.dispatch(args)
    assert result["status"] == failure, result
    assert result["error"]
    assert "description MUST-NOT-RUN" not in peer.commands
    assert peer.commands.count("description AI-LAB") == 1
    if failure == "partial":
        assert result["confirmed_commands"] == 2
        assert not peer.saved
    if failure == "unknown":
        assert session not in worker.sessions


def test_untargeted_evidence_cannot_certify_a_change_that_never_ran(connected):
    """A weak check on version/banner output must not turn a dropped command into success."""
    worker, session, peer = connected()
    peer.ignore_commands.add("description AI-LAB")
    # The check echoes text that exists regardless of the change, so even though
    # every condition matches, the dropped command cannot be certified.
    args = request(
        session,
        checks=[
            {"query": "version", "contains": ["Huawei"], "not_contains": []},
            {"query": "config", "contains": ["LAB"], "not_contains": []},
        ],
    )
    result = worker.dispatch(args)
    assert result["status"] == "applied_unverified", result
    assert "VERIFY_INCOMPLETE" in result["error"]
    assert peer.commands.count("description AI-LAB") == 1
    # A targeted check on the same dropped command is a hard verification failure.
    targeted = request(session)
    result = worker.dispatch(targeted)
    assert result["status"] == "verification_failed", result
    assert peer.commands.count("description AI-LAB") == 2


def test_decoupled_check_matching_pre_existing_text_is_not_success(connected):
    """Checks that only echo pre-existing config text stay applied_unverified, not succeeded."""
    worker, session, peer = connected()
    args = request(
        session,
        checks=[{"query": "config", "contains": ["sysname LAB"], "not_contains": []}],
    )
    result = worker.dispatch(args)
    assert result["status"] == "applied_unverified", result
    assert "VERIFY_INCOMPLETE" in result["error"]
    assert peer.description == "description AI-LAB"


@pytest.mark.parametrize(
    "weak",
    [
        # No read-back condition at all: only version evidence.
        [{"query": "version", "contains": ["Huawei"], "not_contains": []}],
        # Only ports evidence; nothing reads the configuration back.
        [{"query": "ports", "contains": ["GE0/0/1"], "not_contains": []}],
        # Checks list entirely empty.
        [],
    ],
)
def test_no_config_readback_is_rejected_before_any_command_is_sent(weak, connected):
    """Without a config read-back there is no evidence path that could prove the goal."""
    worker, session, peer = connected()
    args = request(session, checks=weak)
    with pytest.raises(WorkerError):
        worker.dispatch(args)
    assert "system-view" not in peer.commands
    assert "description AI-LAB" not in peer.commands
    assert "interface GE0/0/1" not in peer.commands


def test_startup_only_check_is_not_full_verification_when_saving(connected):
    """save=true still needs a running-config read-back naming the written value."""
    worker, session, peer = connected()
    args = request(
        session,
        checks=[
            {"query": "startup", "contains": ["description AI-LAB"], "not_contains": []}
        ],
    )
    result = worker.dispatch(args)
    assert result["status"] == "applied_unverified", result
    assert peer.saved
    assert not any(c["query"] == "config" for c in result["checks"])


@pytest.mark.parametrize(
    "bad",
    [
        {"commands": ["interface GE0/0/1\rreboot"]},
        {"commands": []},
        {"save": "yes"},
        {"checks": [{"query": "config", "command": "reboot"}]},
        # Device strings must never become new instructions or unrelated success evidence.
        {"commands": ["description AI-LAB"], "checks": [{"query": "version", "contains": ["Huawei Versatile Routing Platform Software"], "not_contains": []}]},
    ],
)
def test_malformed_config_never_enters_config_mode(bad, connected):
    worker, session, peer = connected()
    with pytest.raises(WorkerError):
        worker.dispatch(request(session, **bad))
    assert "system-view" not in peer.commands


# ---- pure-function evidence gate -------------------------------------------------


def test_context_selection_command_is_not_written_value_evidence():
    """Entering interface mode restates an existing entity, not a written value."""
    from yigou_worker.configure import checks_cover_written_values

    checks = [{"query": "config", "contains": ["interface GE0/0/1"], "not_contains": []}]
    commands = ["interface GE0/0/1", "description AI-LAB"]
    assert checks_cover_written_values(checks, commands) is False


@pytest.mark.parametrize(
    "commands",
    [
        ["interface GE0/0/1", "description AI-LAB"],  # context command before the write
        ["description AI-LAB", "interface GE0/0/1"],  # order must not matter
    ],
)
def test_only_mode_entry_commands_certify_nothing(commands):
    """A batch without a recognized value-write command cannot be certified."""
    from yigou_worker.configure import checks_cover_written_values

    checks = [{"query": "config", "contains": ["interface GE0/0/1"], "not_contains": []}]
    assert checks_cover_written_values(checks, commands) is False


@pytest.mark.parametrize(
    "contains",
    [
        ["description AI-LAB"],
        ["interface GE0/0/1\ndescription AI-LAB"],
        ["AI-LAB"],  # the written literal itself is enough evidence text
    ],
)
def test_contains_naming_written_description_value_still_ties(contains):
    from yigou_worker.configure import checks_cover_written_values

    checks = [{"query": "config", "contains": contains, "not_contains": []}]
    commands = ["interface GE0/0/1", "description AI-LAB"]
    assert checks_cover_written_values(checks, commands) is True


def test_unknown_command_syntax_certifies_nothing():
    """Commands outside the closed vocabulary never become success evidence."""
    from yigou_worker.configure import checks_cover_written_values

    checks = [{"query": "config", "contains": ["whatever"], "not_contains": []}]
    assert checks_cover_written_values(checks, ["whatever value"]) is False


def test_sysname_write_ties_to_sysname_contains():
    from yigou_worker.configure import checks_cover_written_values

    checks = [{"query": "config", "contains": ["sysname NEW"], "not_contains": []}]
    assert checks_cover_written_values(checks, ["sysname NEW"]) is True
    # Existing pre-batch text (old sysname) still cannot certify a rename.
    assert checks_cover_written_values(checks, ["sysname OLD"]) is False


def test_startup_query_ties_to_written_value():
    from yigou_worker.configure import checks_cover_written_values

    checks = [{"query": "startup", "contains": ["description AI-LAB"], "not_contains": []}]
    commands = ["interface GE0/0/1", "description AI-LAB"]
    assert checks_cover_written_values(checks, commands) is True
    checks = [{"query": "version", "contains": ["description AI-LAB"], "not_contains": []}]
    assert checks_cover_written_values(checks, commands) is False


# ---- end-to-end over scripted SSH ------------------------------------------------


def test_dropped_description_with_context_only_check_is_not_success(connected):
    """Device ignores the write; matching only the interface line must not say succeeded."""
    worker, session, peer = connected()
    peer.existing_interfaces.add("GE0/0/1")
    peer.ignore_commands.add("description AI-LAB")
    # GE0/0/1 exists in the read-back whether or not the description command ran,
    # so this check passes even though the value was never written.
    args = request(
        session,
        checks=[{"query": "config", "contains": ["interface GE0/0/1"], "not_contains": []}],
    )
    result = worker.dispatch(args)
    assert result["status"] == "applied_unverified", result
    assert "VERIFY_INCOMPLETE" in result["error"]
    assert peer.description == ""  # the write was genuinely dropped
    assert all(check["passed"] for check in result["checks"])


def test_targeted_description_check_over_ssh_still_succeeds(connected):
    """The honest checks keep earning succeeded after the stricter evidence gate."""
    worker, session, peer = connected()
    result = worker.dispatch(request(session))
    assert result["status"] == "succeeded", result
    assert peer.description == "description AI-LAB"
    assert result["save_status"] == "verified"


@pytest.mark.parametrize("command", ["conf t", "SY", "q", "save force", "reload in 1", "reboot", "description ok; reboot"])
def test_batch_rejects_control_and_destructive_commands(command):
    from yigou_worker.configure import validate_request

    with pytest.raises(WorkerError):
        validate_request(request("unused", commands=[command]))


def test_one_matching_value_cannot_certify_other_writes():
    from yigou_worker.configure import checks_cover_written_values

    checks = [{"query": "config", "contains": ["description FIRST"], "not_contains": []}]
    assert not checks_cover_written_values(checks, ["description FIRST", "description SECOND"])
    assert not checks_cover_written_values(checks, ["description FIRST", "unknown change"])


def test_readback_must_prove_the_correct_interface_and_vlan_section():
    from yigou_worker.configure import scoped_writes_verified

    commands = ["interface GE0/0/1", "description UPLINK-A"]
    wrong = "#\ninterface GE0/0/1\n description OLD\n#\ninterface GE0/0/2\n description UPLINK-A\n#\nreturn"
    right = "#\ninterface GE0/0/1\n description UPLINK-A\n#\ninterface GE0/0/2\n description OLD\n#\nreturn"
    assert not scoped_writes_verified(wrong, commands)
    assert scoped_writes_verified(right, commands)
    assert not scoped_writes_verified(right, ["interface GE0/0/2", "description UPLINK-A"])
    assert scoped_writes_verified("#\nvlan 120\n name LAB\n#\nreturn", ["vlan 120", "name LAB"])
    assert not scoped_writes_verified("#\nvlan 121\n name LAB\n#\nreturn", ["vlan 120", "name LAB"])
    assert not scoped_writes_verified(right, ["interface GE0/0/1", "undo shutdown"])


def test_duplicate_or_banner_interfaces_do_not_certify_the_target():
    from yigou_worker.configure import scoped_writes_verified

    commands = ["interface GE0/0/1", "description TARGET"]

    # Banner/login header text preceding the first section delimiter mirrors the
    # target lines; the parser must not treat pre-config text as a real section.
    banner = (
        "header login information ^\ninterface GE0/0/1\n description TARGET\n"
        "^\n#\ninterface GE0/0/1\n description OLD\n#\nreturn"
    )
    assert not scoped_writes_verified(banner, commands)

    # A duplicated interface section leaves the effective final value ambiguous.
    duplicated = (
        "#\ninterface GE0/0/1\n description TARGET\n"
        "#\ninterface GE0/0/1\n description OLD\n#\nreturn"
    )
    assert not scoped_writes_verified(duplicated, commands)

    # Two conflicting description lines inside one section: the device would
    # keep the last one, so the requested value is not the section's value.
    conflicting = "#\ninterface GE0/0/1\n description TARGET\n description OLD\n#\nreturn"
    assert not scoped_writes_verified(conflicting, commands)

    right = "#\nsysname LAB\n#\ninterface GE0/0/1\n description TARGET\n#\nreturn"
    assert scoped_writes_verified(right, commands)


def test_readback_must_preserve_the_exact_case_of_written_values():
    from yigou_worker.configure import scoped_writes_verified

    # A different global string can satisfy an overly broad contains-check,
    # while the interface itself still has the old, differently-cased value.
    config = (
        "#\nsysname uplink-a\n#\ninterface GE0/0/1\n"
        " description UPLINK-A\n#\nreturn"
    )
    assert not scoped_writes_verified(config, ["interface ge0/0/1", "description uplink-a"])
    assert scoped_writes_verified(config, ["interface ge0/0/1", "description UPLINK-A"])
    vlan = "#\nvlan 120\n name Office\n#\nreturn"
    assert not scoped_writes_verified(vlan, ["vlan 120", "name office"])
    assert scoped_writes_verified(vlan, ["vlan 120", "name Office"])


def test_case_only_mismatch_is_not_certified_by_unrelated_config_text(connected):
    worker, session, peer = connected()
    peer.outputs["display current-configuration"] = (
        "#\nsysname uplink-a\nlocal-user lab password cipher fixture-sensitive\n#\nreturn"
    )
    peer.description = "description UPLINK-A"
    peer.ignore_commands.add("description uplink-a")
    result = worker.dispatch(request(
        session, commands=["interface GE0/0/1", "description uplink-a"],
        save=False,
        checks=[{"query": "config", "contains": ["uplink-a"], "not_contains": []}],
    ))
    assert result["status"] == "applied_unverified", result
    assert all(check["passed"] for check in result["checks"])
    assert peer.description == "description UPLINK-A"


def test_every_requested_vlan_must_exist_with_its_exact_identifier():
    from yigou_worker.configure import configure, scoped_writes_verified

    commands = ["vlan 120", "interface GE0/0/1", "description TARGET"]
    readback = (
        "#\nsysname LAB\n#\nvlan 1200\n name OTHER\n#\n"
        "interface GE0/0/1\n description TARGET\n#\nreturn"
    )
    assert not scoped_writes_verified(readback, commands)
    assert scoped_writes_verified(readback.replace("vlan 1200", "vlan 120"), commands)

    class DroppedVlan:
        def send_config_set(self, commands, **kwargs):
            return "Command accepted"

        def exit_config_mode(self):
            pass

        def send_command(self, command, **kwargs):
            return readback

    # Broad contains passes on VLAN 1200, but execution must not claim success.
    result = configure(DroppedVlan(), "huawei", {
        "commands": commands, "save": False,
        "checks": [{"query": "config", "contains": ["vlan 120", "description TARGET"],
                    "not_contains": []}],
    })
    assert all(check["passed"] for check in result["checks"])
    assert result["status"] == "applied_unverified", result


def test_nested_description_and_changed_literal_whitespace_never_certify():
    from yigou_worker.configure import scoped_writes_verified

    commands = ["interface GE0/0/1", "description TARGET"]
    nested = "#\ninterface GE0/0/1\n service instance 7\n  description TARGET\n#\nreturn"
    assert not scoped_writes_verified(nested, commands)
    exact = "#\ninterface GE0/0/1\n description A  B\n#\nreturn"
    assert not scoped_writes_verified(exact, ["interface GE0/0/1", "description A B"])
    assert scoped_writes_verified(exact, ["interface GE0/0/1", "description A  B"])


def test_interface_selector_cannot_be_certified_by_an_unrelated_global_write():
    from yigou_worker.configure import configure, scoped_writes_verified

    commands = ["interface LoopBack999", "sysname NEW"]
    observed = "#\nsysname NEW\n#\ninterface GE0/0/1\n description OLD\n#\nreturn"
    assert not scoped_writes_verified(observed, commands)
    assert scoped_writes_verified(
        observed.replace("interface GE0/0/1", "interface LoopBack999"), commands
    )

    class IgnoredInterface:
        def send_config_set(self, commands, **kwargs):
            return "Configuration accepted"

        def exit_config_mode(self):
            pass

        def send_command(self, command, **kwargs):
            return observed

    result = configure(IgnoredInterface(), "huawei", {
        "commands": commands,
        "save": False,
        "checks": [{"query": "config", "contains": ["sysname NEW"], "not_contains": []}],
    })
    assert result["status"] == "applied_unverified", result


def test_vendor_rejection_cannot_count_as_a_confirmed_configuration_command():
    from yigou_worker.configure import configure

    class RejectingPeer:
        def __init__(self):
            self.commands = []

        def send_config_set(self, commands, **kwargs):
            self.commands.extend(commands)
            return "% Failed: permission denied on this interface"

        def exit_config_mode(self):
            pass

        def send_command(self, command, **kwargs):
            return "#\nsysname LAB\n#\ninterface GE0/0/1\n description TARGET\n#\nreturn"

    peer = RejectingPeer()
    result = configure(peer, "huawei", {
        "commands": ["interface GE0/0/1", "description TARGET"],
        "save": False,
        "checks": [{"query": "config", "contains": ["description TARGET"], "not_contains": []}],
    })
    assert result["status"] == "partial", result
    assert result["attempted_commands"] == 1
    assert result["confirmed_commands"] == 0
    assert peer.commands == ["interface GE0/0/1"]


@pytest.mark.parametrize("command", [
    "local-user audit password cipher EXAMPLE123",
    "description 设备密码：EXAMPLE123",
    "description X-API-Key: EXAMPLE123",
    "snmp-agent community read EXAMPLE123",
])
def test_sensitive_config_command_rejected_before_automatic_write(command):
    from yigou_worker.configure import validate_request

    with pytest.raises(WorkerError, match="凭据"):
        validate_request(request("unused", commands=[command]))


def test_wrong_interface_value_never_certifies_a_successful_batch(connected):
    worker, session, peer = connected()
    args = request(session, commands=["interface GE0/0/2", "description AI-LAB"], save=False)
    result = worker.dispatch(args)
    assert result["status"] == "applied_unverified", result
    assert "VERIFY_INCOMPLETE" in result["error"]
    assert peer.description == "description AI-LAB"  # Fake peer renders it under GE0/0/1.
