"""Configuration batches with explicit partial/unknown outcomes and read-back evidence."""

import re

from netmiko.exceptions import ConfigInvalidException

from .backup import validate_backup
from .query import COMMAND_ERROR_PATTERN, WorkerError, command_for, ensure_output, redact


def validate_request(request):
    commands = request.get("commands")
    checks = request.get("checks", [])
    if not isinstance(commands, list) or not 1 <= len(commands) <= 256:
        raise WorkerError("CONFIG_INVALID", "配置命令需为 1 至 256 行")
    if any(
        not isinstance(c, str)
        or not c.strip()
        or len(c.encode()) > 4096
        or any(ord(ch) < 32 or ord(ch) == 127 for ch in c)
        for c in commands
    ):
        raise WorkerError("CONFIG_INVALID", "每项配置必须是单行命令，不可包含终端控制字符")
    denied = ("quit", "exit", "end", "return", "save", "write", "system-view",
              "configure", "reboot", "reload", "reset", "erase", "delete", "format",
              "shell", "python", "tclsh", "boot", "startup", "copy", "do", "run", "enable", "disable")
    for command in commands:
        first = command.strip().lower().split()[0]
        if any(c in command for c in ";|&`") or any(word.startswith(first) for word in denied):
            raise WorkerError("CONFIG_INVALID", "禁止模式控制、保存、破坏性或组合命令")
        if redact(command) == "[REDACTED]":
            # AI plans are persisted as cleartext command arrays in SQLite.
            # Credential configuration must use the human-operated terminal.
            raise WorkerError("CONFIG_SENSITIVE_DENIED", "配置命令可能包含凭据，请使用人工终端")
    if sum(len(c.encode()) for c in commands) > 65536 or not isinstance(request.get("save"), bool):
        raise WorkerError("CONFIG_INVALID", "配置批次超过 64 KiB 或缺少保存选项")
    if not isinstance(checks, list) or len(checks) > 16:
        raise WorkerError("CONFIG_INVALID", "核验条件格式无效")
    for check in checks:
        if not isinstance(check, dict) or set(check) - {"query", "contains", "not_contains"}:
            raise WorkerError("CONFIG_INVALID", "核验条件包含未知字段")
        if check.get("query") not in ("version", "ports", "config", "neighbors", "startup"):
            raise WorkerError("CONFIG_INVALID", "核验查询类型无效")
        for field in ("contains", "not_contains"):
            values = check.get(field, [])
            if (
                not isinstance(values, list)
                or len(values) > 32
                or any(not isinstance(v, str) or not v or len(v.encode()) > 4096 for v in values)
            ):
                raise WorkerError("CONFIG_INVALID", "核验文本无效")
        if not check.get("contains") and not check.get("not_contains"):
            raise WorkerError("CONFIG_INVALID", "每项核验至少需要一个匹配条件")
    return [c.strip() for c in commands], checks


# Minimal closed set of "<keyword> <value>" commands whose argument is a literal
# config value shared by the supported vendors (description, sysname/hostname,
# vlan/interface names). Mode-entry commands (interface, vlan, ospf, line, ...)
# only select an entity that exists independently of this batch, and any other
# syntax cannot be classified, so neither may contribute certification values.
_VALUE_WRITE_KEYWORDS = frozenset({"description", "sysname", "hostname", "name"})


def _argument_literals(commands):
    """Concrete argument values written by unambiguous value-write commands.

    A context command such as "interface GE0/0/1" restates an entity that already
    exists on the device; matching it in a read-back cannot distinguish a real
    write from a dropped one. Only known value-write keywords contribute their
    argument text, and unknown syntax certifies nothing (fail closed).
    """
    literals = set()
    for command in commands:
        parts = command.split()
        if len(parts) >= 2 and parts[0].lower() in ("interface", "vlan"):
            continue
        if len(parts) < 2 or parts[0].lower() not in _VALUE_WRITE_KEYWORDS:
            return set()  # Unclassified writes cannot be certified by other commands.
        value = command.split(None, 1)[1].strip()
        for candidate in (value, value.strip('"').strip("'")):
            if candidate and not candidate.startswith("$"):
                literals.add(candidate)
    return literals


def verify_checks(checks, commands):
    """Structural gate before anything is sent: the batch must carry a config read-back.

    Without any config/startup condition there is no evidence path that could prove
    the goal, so sending the batch would only ever produce unverifiable results.
    Weak or decoupled contains-text is handled after execution as
    applied_unverified, because value-less commands (undo ...) are legitimate.
    """
    if not any(check["query"] in ("config", "startup") for check in checks):
        raise WorkerError(
            "CONFIG_INVALID",
            "核验不充分：缺少对运行或启动配置的回读条件，无法证明配置目标已实现",
        )


def checks_cover_written_values(checks, commands):
    """True when contains-text covers every recognized literal this batch writes.

    Read-back conditions that only match text which existed before the change
    (sysname, banners, vendor name) cannot distinguish success from no-op.
    """
    written = _argument_literals(commands)
    if not written:
        return False
    values = [value for check in checks if check["query"] in ("config", "startup")
              for value in check.get("contains", [])]
    return all(any(word in value for value in values) for word in written)


def scoped_writes_verified(config_text, commands):
    """Require every recognized change in its exact interface/VLAN block.

    This is a conservative text parser, not a universal vendor CLI parser.
    Unrecognized syntax or ambiguous sections never earns "succeeded".
    """
    required = {}
    required_sections = set()
    scope = None
    for command in commands:
        parts = command.strip().split(None, 1)
        if len(parts) != 2:
            return False
        kind, value = parts
        kind = kind.lower()
        # Keywords and interface identifiers are case-insensitive, but
        # description/name values may be case-sensitive. Never certify a
        # case-only mismatch as a successful device change.
        # Preserve literal argument whitespace: it can be part of a description.
        normalized = f"{kind} {value}"
        if kind in ("interface", "vlan"):
            scope = (kind, value.casefold())
            # Both entity selectors may create a new object. Another successful
            # write does not certify that the requested interface/VLAN exists.
            required_sections.add(scope)
        elif kind == "description" and scope and scope[0] == "interface":
            # A duplicate/conflicting target in one batch is ambiguous: the
            # device would only keep the final value, so nothing is provable.
            if required.get(scope) not in (None, normalized):
                return False
            required[scope] = normalized
        elif kind == "name" and scope and scope[0] == "vlan":
            if required.get(scope) not in (None, normalized):
                return False
            required[scope] = normalized
        elif kind in ("sysname", "hostname"):
            scope = None
            if required.get(None) not in (None, normalized):
                return False
            required[None] = normalized
        else:
            return False
    if not required:
        return False

    # A readback is evidence only if the target has one unambiguous block.
    # Banner/header literals may contain arbitrary CLI-looking lines; skip
    # their contents rather than treating them as interface sections.
    observed = {}
    global_values = set()
    seen_sections = set()
    scope = None
    banner_delimiter = None
    for raw in config_text.splitlines():
        line = raw.strip()
        if not line:
            continue
        indent = len(raw) - len(raw.lstrip(" \t"))
        if banner_delimiter is not None:
            if line == banner_delimiter:
                banner_delimiter = None
            continue

        keyword, _, value = line.partition(" ")
        kind = keyword.casefold()
        normalized = f"{kind} {value}" if value else kind
        if normalized in ("#", "!", "return", "end"):
            scope = None
        elif kind in ("header", "banner", "motd"):
            # Known multiline forms use an explicit closing delimiter. Unknown
            # banner forms cannot certify any configuration values.
            marker = value.split()[-1] if value else ""
            if marker not in ("^", "^C", "%", "~", "#", "!"):
                return False
            scope = None
            banner_delimiter = marker
        elif kind in ("interface", "vlan") and value:
            # A nested occurrence cannot authenticate a top-level entity.
            if indent:
                return False
            scope = (kind, value.casefold())
            if scope in seen_sections:
                return False  # Duplicate entity: final effective state unclear.
            seen_sections.add(scope)
            observed[scope] = {}
        elif kind in ("sysname", "hostname") and value:
            scope = None
            global_values.add(normalized)
        else:
            # An unindented top-level command leaves the previous block. Some
            # supported devices omit indentation for description and name.
            if not raw[:1].isspace() and kind not in ("description", "name"):
                scope = None
            if scope is not None:
                expected_kind = "description" if scope[0] == "interface" else "name"
                if kind == expected_kind and indent <= 1:
                    observed[scope].setdefault(kind, set()).add(normalized)

    if banner_delimiter is not None:
        return False  # Incomplete multiline text cannot authorize a change.
    # An interface or VLAN mode-entry command may create an entity. Certify
    # the exact entity even when the batch also writes unrelated global values.
    if not required_sections <= seen_sections:
        return False
    for section, target in required.items():
        if section is None:
            if target not in global_values or len(global_values) != 1:
                return False
        else:
            keyword = "description" if section[0] == "interface" else "name"
            # Exact value, correct section, no competing effective value.
            if observed.get(section, {}).get(keyword, set()) != {target}:
                return False
    return True


def configure(conn, vendor, request):
    commands, checks = validate_request(request)
    if not commands:
        raise WorkerError("CONFIG_INVALID", "配置批次没有实际命令")
    verify_checks(checks, commands)
    result = {
        "status": "running",
        "attempted_commands": 0,
        "confirmed_commands": 0,
        "save_status": "pending" if request["save"] else "not_requested",
        "checks": [],
        "output": "",
        "error": None,
    }
    phase = "configure"
    try:
        for command in commands:
            result["attempted_commands"] += 1
            output = conn.send_config_set(
                [command],
                exit_config_mode=False,
                cmd_verify=True,
                error_pattern=COMMAND_ERROR_PATTERN,
                read_timeout=45,
            )
            ensure_output(output)
            result["confirmed_commands"] += 1
            result["output"] = (result["output"] + redact(output) + "\n")[:48000]
        conn.exit_config_mode()
        phase = "save"
        if request["save"]:
            output = ensure_output(conn.save_config())
            # A confirmation prompt left unresolved is not a completed save.
            if re.search(r"(?i)(cancelled|canceled|not saved|save failed)", output):
                raise WorkerError("SAVE_FAILED", "设备未完成配置保存")
            result["output"] = (result["output"] + redact(output))[:48000]
            result["save_status"] = "acknowledged"
        phase = "verify"
        raw = validate_backup(
            vendor, conn.send_command(command_for(vendor, "config"), read_timeout=45)
        )
        result["after_content"] = raw  # private worker RPC only; Rust removes before logging/events
        cache = {"config": raw}
        expanded = list(checks)
        if request["save"]:
            startup = validate_backup(
                vendor, conn.send_command(command_for(vendor, "startup"), read_timeout=45)
            )
            cache["startup"] = startup
            expanded += [
                {**check, "query": "startup"} for check in checks if check["query"] == "config"
            ]
        for check in expanded:
            query = check["query"]
            if query not in cache:
                cache[query] = ensure_output(
                    conn.send_command(command_for(vendor, query), read_timeout=45)
                )
            output = cache[query]
            passed = all(v in output for v in check.get("contains", [])) and all(
                v not in output for v in check.get("not_contains", [])
            )
            result["checks"].append(
                {"query": query, "passed": passed, "evidence": redact(output)[:16000]}
            )
        if any(not check["passed"] for check in result["checks"]):
            result.update(
                status="verification_failed", error="VERIFY_MISMATCH: 回读结果不符合配置目标"
            )
        else:
            # "succeeded" must be earned: a running-config read-back naming what the
            # batch wrote, and persistence evidence when the user asked to save.
            config_checks = [c for c in checks if c["query"] == "config"]
            tied = checks_cover_written_values(config_checks, commands)
            scoped = scoped_writes_verified(raw, commands)
            persisted = (not request["save"]) or (
                any(c["query"] == "startup" for c in expanded)
                and scoped_writes_verified(cache.get("startup", ""), commands)
            )
            if config_checks and tied and scoped and persisted:
                result["status"] = "succeeded"
                if request["save"]:
                    result["save_status"] = "verified"
            else:
                reason = (
                    "回读条件未包含本批次写入的具体配置值"
                    if not tied else
                    "无法在目标接口或 VLAN 配置段落确认本批次全部修改"
                    if not scoped else "缺少对运行配置的回读核验"
                )
                if request["save"] and not persisted:
                    reason += "；缺少启动配置持久化核验"
                result.update(
                    status="applied_unverified",
                    error=f"VERIFY_INCOMPLETE: 命令已发送，{reason}，不能确认为成功",
                )
    except (ConfigInvalidException, WorkerError):
        result.update(
            status={"configure": "partial", "save": "save_failed", "verify": "verification_failed"}[
                phase
            ],
            error={
                "configure": "CONFIG_COMMAND_FAILED: 设备拒绝了配置命令，未继续发送后续命令",
                "save": "SAVE_FAILED: 保存未确认完成，运行配置可能已经改变",
                "verify": "VERIFY_FAILED: 回读核验失败，配置可能已经改变",
            }[phase],
        )
        if phase == "save":
            result["save_status"] = "failed"
    except Exception:  # noqa: BLE001 -- never return library exceptions containing commands/secrets
        result.update(
            status="unknown", error="CONFIG_UNKNOWN: 连接中断或超时，执行结果不确定，未自动重试"
        )
        if phase == "save":
            result["save_status"] = "unknown"
    finally:
        if result["status"] not in ("succeeded", "unknown"):
            try:
                conn.exit_config_mode()
                if "after_content" not in result:
                    result["after_content"] = validate_backup(
                        vendor, conn.send_command(command_for(vendor, "config"), read_timeout=10)
                    )
            except Exception:  # noqa: BLE001 -- outcome already explicitly failed/unknown
                pass
    return result
