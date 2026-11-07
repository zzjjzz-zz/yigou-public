"""Conservative LLDP parser. Every edge carries its exact source block."""

import re


def parse_neighbors(output: str) -> dict:
    blocks = re.split(
        r"(?im)(?=^\s*(?:[\w/.-]+\s+has\s+\d+\s+neighbou?r|"
        r"LLDP neighbor-information of port|Local (?:Intf|Interface|port)\s*:))",
        output,
    )
    neighbors = []
    for block in blocks:
        local = re.search(
            r"(?im)(?:Local (?:Intf|Interface|port)\s*:\s*|"
            r"neighbor-information of port\s+|^\s*)([\w/.-]+)(?:\s+has\s+\d+\s+neighbou?r|\s*[:\r\n])",
            block,
        )
        remote = re.search(r"(?im)^\s*(?:System name|System Name)\s*:\s*(.+)$", block)
        port = re.search(r"(?im)^\s*Port\s*(?:ID|Id|id)\s*:\s*(.+)$", block)
        chassis = re.search(r"(?im)^\s*Chassis\s*(?:ID|Id|id)\s*:\s*(.+)$", block)
        if remote or chassis:
            neighbors.append(
                {
                    "local_port": local.group(1).strip() if local else None,
                    "remote_name": remote.group(1).strip() if remote else None,
                    "remote_port": port.group(1).strip() if port else None,
                    "chassis_id": chassis.group(1).strip() if chassis else None,
                    "protocol": "LLDP",
                    "evidence": block.strip(),
                }
            )
    return {
        "neighbors": neighbors,
        "parsed": bool(neighbors),
        "raw": output,
        "note": None if neighbors else "未解析出邻居；原始输出已保留，不能据此判定没有邻居",
    }
