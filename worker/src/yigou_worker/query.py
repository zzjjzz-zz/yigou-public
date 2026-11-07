"""Closed command vocabulary; never accept commands supplied by a model.

Per-platform capability table. `queries` is the read-only whitelist of query
kinds this platform has CLI-manual evidence for; a missing entry means the
command is not offered and is hard-denied with QUERY_DENIED before any bytes
reach a device. `backup`/`configure` are lightweight capability flags: only
platforms whose end-marker validation and scripted save/read-back loop pass may
carry them.
"""

import re

DRIVERS = {
    "huawei": {
        "device_type": "huawei",
        "queries": {
            "version": "display version",
            "ports": "display interface brief",
            "config": "display current-configuration",
            "startup": "display saved-configuration",
            "neighbors": "display lldp neighbor verbose",
        },
        "backup": True,
        "configure": True,
    },
    "ruijie": {
        "device_type": "ruijie_os",
        "queries": {
            "version": "show version",
            "ports": "show interfaces status",
            "config": "show running-config",
            "startup": "show startup-config",
            "neighbors": "show lldp neighbors detail",
        },
        "backup": True,
        "configure": True,
    },
    "h3c": {
        "device_type": "hp_comware",
        "queries": {
            "version": "display version",
            "ports": "display interface brief",
            "config": "display current-configuration",
            "startup": "display saved-configuration",
            "neighbors": "display lldp neighbor-information verbose",
        },
        "backup": True,
        "configure": True,
    },
    # ---- New platforms (batch 1): read-only extension tiers ----
    # Cisco IOS / IOS-XE: candidate commands per Cisco IOS configuration guides.
    "cisco_ios": {
        "device_type": "cisco_ios",
        "queries": {
            "version": "show version",
            "ports": "show ip interface brief",
            "config": "show running-config",
            "startup": "show startup-config",
            "neighbors": "show lldp neighbors detail",
        },
        "backup": False,
        "configure": False,
    },
    "cisco_xe": {
        "device_type": "cisco_xe",
        "queries": {
            "version": "show version",
            "ports": "show ip interface brief",
            "config": "show running-config",
            "startup": "show startup-config",
            "neighbors": "show lldp neighbors detail",
        },
        "backup": False,
        "configure": False,
    },
    # Maipu MyPower: per vendor manual (show version / running / startup).
    "maipu": {
        "device_type": "maipu",
        "queries": {
            "version": "show version",
            "config": "show running-config",
            "startup": "show startup-config",
        },
        "backup": False,
        "configure": False,
    },
    # Raisecom ROS (瑞斯康达), distinct from Ruijie (锐捷).
    "raisecom_ros": {
        "device_type": "raisecom_ros",
        "queries": {
            "version": "show version",
            "config": "show running-config",
            "startup": "show startup-config",
        },
        "backup": False,
        "configure": False,
    },
    # Raisecom ROAP: only `show version` has sufficient CLI evidence for now.
    "raisecom_roap": {
        "device_type": "raisecom_roap",
        "queries": {
            "version": "show version",
        },
        "backup": False,
        "configure": False,
    },
    # Huawei OLT (SmartAX): display version / display current-configuration.
    "huawei_olt": {
        "device_type": "huawei_olt",
        "queries": {
            "version": "display version",
            "config": "display current-configuration",
        },
        "backup": False,
        "configure": False,
    },
    # Huawei SmartAX switches: display version / display current-configuration.
    "huawei_smartax": {
        "device_type": "huawei_smartax",
        "queries": {
            "version": "display version",
            "config": "display current-configuration",
        },
        "backup": False,
        "configure": False,
    },
    # ZTE ZXROS: only `show version` has sufficient CLI evidence for now.
    "zte_zxros": {
        "device_type": "zte_zxros",
        "queries": {
            "version": "show version",
        },
        "backup": False,
        "configure": False,
    },
}

QUERY_KINDS = ("version", "ports", "config", "neighbors", "startup")


class WorkerError(Exception):
    def __init__(self, code: str, message: str):
        self.code = code
        super().__init__(message)


def command_for(vendor: str, query: str) -> str:
    driver = DRIVERS.get(vendor)
    if driver is None or query not in QUERY_KINDS:
        raise WorkerError("QUERY_DENIED", "查询不在只读白名单中")
    table = driver["queries"]
    if query not in table:
        raise WorkerError(
            "QUERY_DENIED", "该厂商暂不支持此查询：命令证据不足，未向设备发送任何内容"
        )
    return table[query]


def can(vendor: str, capability: str) -> bool:
    """Capability gate (backup/configure). Fails closed on unknown vendors."""
    driver = DRIVERS.get(vendor)
    return bool(driver and driver.get(capability))


def ensure_capability(vendor: str, capability: str, operation: str):
    """Hard-reject unsupported operations before any device bytes are sent."""
    if not can(vendor, capability):
        raise WorkerError(
            "OPERATION_UNSUPPORTED",
            f"该厂商暂不支持{operation}：平台处于只读阶段，未向设备发送任何内容",
        )


_SENSITIVE = re.compile(
    r"(?i)(password|passwd|secret|community|private[-_ ]?key|cipher|"
    r"authentication[- ]?(?:key|password)|pre[- ]?shared[- ]?key|"
    r"snmp-agent\s+usm-user|snmp-server\s+user|radius.*key|tacacs.*key|"
    r"key-string|token|authorization|credential|api[-_ ]?key|x[-_ ]?api[-_ ]?key|"
    r"设备密码|用户密码|管理密码|密码|口令|密钥|私钥|预共享密钥|访问令牌)"
)


def redact(text: str) -> str:
    lines = []
    in_key = False
    for line in text.splitlines():
        if re.search(r"-----BEGIN .*PRIVATE KEY-----", line):
            in_key = True
        sensitive = in_key or _SENSITIVE.search(line)
        lines.append("[REDACTED]" if sensitive else line)
        if re.search(r"-----END .*PRIVATE KEY-----", line):
            in_key = False
    return "\n".join(lines)


COMMAND_ERROR_PATTERN = (
    r"(?im)^\s*(?:%\s*(?:error|fail(?:ed|ure)?|invalid|unknown|unrecognized|incomplete|"
    r"ambiguous|unsupported|permission|authorization|access|privilege)\b|"
    r"(?:error|fail(?:ed|ure)?)\s*[:：]|unrecognized command|unknown command|"
    r"permission denied|invalid input|命令错误|无效命令|权限不足|配置失败|执行失败|拒绝执行)"
)


def ensure_output(output: str) -> str:
    if not output.strip():
        raise WorkerError("EMPTY_OUTPUT", "设备返回空内容")
    if len(output.encode()) > 8 * 1024 * 1024:
        raise WorkerError("OUTPUT_LIMIT", "设备输出超过 8 MiB 上限")
    if re.search(COMMAND_ERROR_PATTERN, output):
        raise WorkerError("DEVICE_COMMAND_FAILED", "设备拒绝查询，请确认型号与命令适配")
    return output
