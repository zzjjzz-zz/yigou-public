import re

from .query import WorkerError, ensure_output


def validate_backup(vendor: str, output: str) -> str:
    """End-marker validation. Conservative candidate-format checks only; the
    platforms currently allowed to reach this code are the three original
    vendors (whose scripted save/read-back loop is covered by tests). Any new
    platform must not be granted the `backup` capability until it has the same
    scripted end-to-end coverage."""
    ensure_output(output)
    if re.search(r"(?i)(--\s*more\s*--|----\s*more\s*----|press any key)", output):
        raise WorkerError("BACKUP_INCOMPLETE", "配置仍包含分页标记，拒绝保存为成功备份")
    lines = [line.strip() for line in output.strip().splitlines() if line.strip()]
    # Cisco IOS/IOS-XE running-config ends with "end"; Maipu follows the same
    # convention per its manual. The original three keep their proven markers.
    ending = {
        "ruijie": "end",
        "cisco_ios": "end",
        "cisco_xe": "end",
        "maipu": "end",
        "raisecom_ros": "end",
    }.get(vendor, "return")
    if len(lines) < 3 or lines[-1].lower() != ending:
        raise WorkerError("BACKUP_INCOMPLETE", "缺少配置结束标记；当前型号需进一步验证")
    marker = {
        "ruijie": r"(?im)^\s*(?:version|hostname)\s+",
        "cisco_ios": r"(?im)^\s*(?:version|hostname)\s+",
        "cisco_xe": r"(?im)^\s*(?:version|hostname)\s+",
        "maipu": r"(?im)^\s*(?:hostname|sysname|version)\s+",
        "raisecom_ros": r"(?im)^\s*(?:hostname|sysname|version)\s+",
    }.get(
        vendor,
        r"(?im)^\s*(?:sysname|!Software Version|version)\s+",
    )
    if not re.search(marker, output):
        raise WorkerError("BACKUP_INCOMPLETE", "未识别配置主体，拒绝保存为成功备份")
    return output
