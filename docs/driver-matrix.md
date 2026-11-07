# 型号验证边界

## 第一批五组厂商适配（Netmiko 4.8.0 CLASS_MAPPER 已核实的 8 个 device_type）

本批 8 个新平台分三级能力，全部只读；仅原三厂商保留备份与 AI 自动配置。

| 平台 | 官方 device_type | 能力层级 | 只读查询白名单 | 自动备份 | AI 自动写入 | 当前证据 |
| --- | --- | --- | --- | --- | --- | --- |
| 华为 | `huawei` | 读写（原有） | version / ports / config / startup / neighbors | 可用 | 可用（回读核验） | 本地脚本化 SSH 测试；真实型号 NOT_RUN |
| 锐捷 | `ruijie_os` | 读写（原有） | 同上 | 可用 | 可用（回读核验） | 本地脚本化 SSH 测试；真实型号 NOT_RUN |
| 新华三 | `hp_comware` | 读写（原有） | 同上 | 可用 | 可用（回读核验） | 本地脚本化 SSH 测试；真实型号 NOT_RUN |
| 思科 IOS | `cisco_ios` | 1 登记会话 + 2 只读 | version `show version` / ports `show ip interface brief` / config `show running-config` / startup `show startup-config` / neighbors `show lldp neighbors detail` | 拒绝（OPERATION_UNSUPPORTED） | 拒绝 | Netmiko 驱动 ID 已核实；命令为 CLI 手册候选；真实型号 NOT_RUN；自动写入需 Netmiko Cisco 驱动仿真保存/读回闭环验收后另行开放 |
| 思科 IOS-XE | `cisco_xe` | 同思科 IOS | 同上 | 拒绝 | 拒绝 | 同上 |
| 迈普 | `maipu` | 1 登记会话 + 2 只读 | version `show version` / config `show running-config` / startup `show startup-config` | 拒绝 | 拒绝 | Netmiko `maipu` 驱动 ID 已核实；命令按迈普手册候选；真实型号 NOT_RUN |
| 瑞斯康达 ROS | `raisecom_ros` | 1 登记会话 + 2 只读 | version `show version` / config `show running-config` / startup `show startup-config` | 拒绝 | 拒绝 | Netmiko `raisecom_ros` 驱动 ID 已核实；命令按手册候选；真实型号 NOT_RUN |
| 瑞斯康达 ROAP | `raisecom_roap` | 1 登记会话 + 2 只读 | 仅 version `show version`（其余证据不足先关） | 拒绝 | 拒绝 | 同上 |
| 华为 OLT | `huawei_olt`（Netmiko 映射 `HuaweiSmartAXSSH`） | 1 登记会话 + 2 只读 | version `display version` / config `display current-configuration` | 拒绝 | 拒绝 | Netmiko 驱动 ID 已核实；真实型号 NOT_RUN |
| 华为 SmartAX | `huawei_smartax` | 1 登记会话 + 2 只读 | 同上 | 拒绝 | 拒绝 | 同上 |
| 中兴 ZXROS | `zte_zxros` | 1 登记会话 + 2 只读 | 仅 version `show version`（其余证据不足先关） | 拒绝 | 拒绝 | Netmiko `zte_zxros` 驱动 ID 已核实；真实型号 NOT_RUN |

## 各级能力定义

1. **登记与会话**：设备登记、人工核实 SSH 主机指纹、Netmiko 严格 host key SSH 会话、人工 PTY 终端、可信的简单版本查询（依据 CLI 手册 vendor version 命令文档证据）。
2. **只读扩展**：按上表白名单开放 `device_query`/`device_discover` 快捷查询。白名单外或命令证据不足的查询类型在 Rust 层与 worker 层双重 `QUERY_DENIED` 拒绝，UI 相应按钮禁用；不发明 LLDP、ports 或 startup 命令。
3. **AI 自动写入**：仅对已通过「Netmiko 驱动 + 本地脚本化 SSH 仿真」的「配置下发→保存→运行/启动配置回读→备份→重复不重放/失败不误报」闭环验收的厂商开放（当前为原三厂商）。Cisco 的仿真闭环验收未完成，故保持只读并如实说明未验收；新平台 AI 对话不注册 `device_configure` 工具，模型无法尝试配置批次。

## 门禁实现（多层，非仅 UI 限制）

- **worker `__main__.py`**：`backup`/`configure` 操作在会话建立后、任何命令字节发送前，按 `DRIVERS[vendor]["backup"/"configure"]` flag 硬拒绝（`OPERATION_UNSUPPORTED`）。
- **worker `query.py`**：`command_for` 对白名单外查询类型 `QUERY_DENIED`；新平台 flag 一律 `False`。
- **Rust `NewDevice::validate`**：8 个新 vendor 可登记；新平台 `auto_backup=true` 直接拒绝（`DEVICE_INVALID`）。
- **Rust `backup`/`configure`**：在任务打点与预备份之前按 `VENDORS` 表拒绝。
- **Rust AI**：只读平台不注册 `device_configure` 工具；工具描述与系统提示均声明只读平台不支持自动写入。
- **Vue UI**：添加设备下拉含 11 厂商并显示层级提示；侧栏按厂商分组；快捷查询/备份按钮按平台白名单禁用；只读厂商自动备份复选框禁用。

## 备份结束标记（新平台候选格式，保守校验）

Cisco IOS/IOS-XE 与迈普运行配置候选结束行 `end`、主体 `version|hostname`；瑞斯康达 ROS 候选 `end` + `hostname|sysname|version`。这些规则已实现但新平台 `backup` flag 保持关闭——在脚本化保存/回读闭环验收前不会用于判定成功备份。原三厂商的备份/指纹/成功判定机制未改动。

## 真实设备验收要求（NOT_RUN）

所有新平台真实设备均为 NOT_RUN：命令为 CLI 手册候选，首次连接需按型号记录原始脱敏输出验证。首验时还应覆盖密码与私钥认证、主机密钥变化、只读权限账号、分页输出与 `--More--` 标记、配置中厂商特有秘密字段脱敏、型号与权限差异。UI 文案已声明三层能力与真实设备未验收状态。
