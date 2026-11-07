# 本地验证记录 · 0.1.1

验证日期：2026-10-23。范围为本地开发与可复现测试，不连接真实设备或真实模型服务。工作区 `~/yigou`（当时的工作区路径，示例），Linux x86_64 / Ubuntu 24.04。

本次修正人工终端权限：人工 SSH 改为完整交互式 PTY，能够进入配置模式、修改和保存，不使用命令白名单或逐条审批。AI 工具继续执行固定只读查询并提供配置计划预览。每个人工标签拥有独立的 SSH 会话和 Python 执行进程，查询重连或查询进程失败不会关闭人工终端。

| 验证项 | 状态 | 证据与范围 |
| --- | --- | --- |
| 完整本地检查 | PASS，59 项 | `builds/local-check-0.1.1.log`，`scripts/check.sh` 退出码 0；43 Python + 10 Rust + 6 UI，含格式、静态检查、前端生产构建 |
| 交互式 SSH | PASS | 实际 SSH 传输连接本地脚本化华为、锐捷、新华三 CLI；测试配置模式、多行中文粘贴、保存确认、Tab/方向键、执行期间 Ctrl+C、无回显密码提示、窗口尺寸及远端关闭 |
| 多会话及 AI 隔离 | PASS | Rust 跨进程测试：查询锁被占用时仍可新建/使用终端；关闭一个标签不影响其他标签；查询重连与查询 worker 故障保留人工会话；人工输入不进入日志、事件或模型请求；AI 只注册固定查询工具 |
| Vue/xterm 输入 | PASS | Playwright IPC 合约测试验证配置命令、控制键、多行 UTF-8 粘贴、双标签路由、关闭会话、查询重置保留终端；`builds/ui-interactive-terminal.png` 为明确标记的 UI fixture 截图 |
| 既有查询、AI、备份回归 | PASS | 原有 Netmiko 查询、Rig SSE 工具调用、加密备份/Git 历史及索引恢复均纳入上述测试；模型是本地脚本 fixture |
| Linux release 安装包 | PASS | `builds/linux-package-0.1.1.log`，退出码 0；包版本 0.1.1 / amd64 |
| 包内 SSH 执行程序 | PASS | `builds/packaged-worker-0.1.1.log`；从新包提取的 sidecar 完成三种脚本化 SSH 环境的查询、备份、LLDP、交互配置/保存、中文粘贴、Ctrl+C、PTY 尺寸及会话隔离测试 |
| 安装包独立启动 | PASS | `builds/package-smoke-0.1.1/result.json`、`builds/package-smoke-0.1.1/desktop.png`；真实 WebKit 窗口显示 0.1.1，前端 → Rust → 包内 worker 握手通过，无 Vite 开发服务器 |
| 真实设备与模型服务 | NOT_RUN | 沿用用户确定的本地测试范围，没有生产网络查询或写入 |
| Windows / macOS、系统安装卸载、远程 CI、部署 | NOT_RUN | 只构建并局部解包验证 Linux 候选包；未创建源码 commit 或推送 |

首轮完整检查在 pnpm 步骤因本机全局 store 路径与项目依赖目录不一致而中断（`builds/local-check-0.1.1-first.log`）。已在 `app/pnpm-workspace.yaml` 固定项目内 store/cache 路径，并在检查脚本中显式执行 frozen install。定向安装与构建通过（`builds/pnpm-store-fix.log`）后重新完成整套检查，没有忽略失败或更新上游版本。

人工终端显示真实原始输出，不做本地回显、伪造提示符或敏感行替换；终端输入与原始录屏不落盘，也不进入 AI 上下文。日志仅记录会话建立、关闭及安全错误信息。配置命令是否成功由设备返回判断，写入接口的 `sent_bytes` 只说明输入已发送。

原生测试使用 Xvfb 与项目内 SDK 的临时挂载命名空间，不在系统部署业务服务。旧版本验证及独立系统凭据库测试见 [0.1.0 历史记录](validation-0.1.0.md)；本次未重复与终端修正无关的凭据库验证。

待真实设备验收时需验证具体型号与账户权限。基础邻居发现仍为 LLDP，CDP 未接入；AI 自动下发与设备自动恢复仍不开放。正式发布的第三方文件核查范围见 `LICENSES/README.md`。

安装包：`builds/target/release/bundle/deb/Yigou_0.1.1_amd64.deb`，36,733,004 字节（35.03 MiB）。SHA256：`d3687d29d0d0f3c60257e66a68513d01457f2473894369e734048159a240864d`。

源码、测试与构建配置共 112 个输入文件的指纹保存在 `builds/source-manifest-0.1.1.json`；已确认从测试结束至打包结束未改变。包内 worker 与冻结构建程序 SHA256 一致，包内许可证清单与本地清单 SHA256 一致。汇总记录为 `builds/validation-0.1.1.json`，校验文件为 `builds/SHA256SUMS`；`builds/validation.json` 与 `builds/source-manifest.json` 指向本次最新记录。
