# Windows 虚拟机验证 · 0.2.1 → 0.2.2

日期：2026-10-24 UTC。服务器现有 Windows 11 Enterprise Evaluation x64 虚拟机，版本 10.0.26300，交互式用户会话。已有 WebView2 155.0.4283.62。此次运行的是实际安装后的 Windows PE 程序；设备端和模型端使用虚拟机内部的脚本化测试服务。

**结论：Windows 11 虚拟机候选验收通过。** 首先安装用户提供的 0.2.1，发现 SSH worker 会打开黑色控制台窗口；修复后构建 0.2.2，在同一虚拟机升级并完成后续验证。真实网络设备、真实模型、物理 Windows 电脑及生产部署未测试。

## 交付文件与修复

- 安装包：`builds/target/x86_64-pc-windows-msvc/release/bundle/nsis/Yigou_0.2.2_x64-setup.exe`
- 大小：24,785,575 字节。
- SHA256：`c462089d548a361f54d2c7e519a3349c797a9102ac5eb97b9b26a398aded8b9e`。
- 包内及虚拟机安装后的主程序 SHA256：`e635f55d362ca139c7c00ee13612cbf264d1c4631aa8d8f146312c115553b711`。
- Windows worker SHA256：`534d5bc7556c361c931735cc9749f01c85e09545ca206bf70183ada5e83a17ce`，与原先在 Windows 原生环境测试过的同一 worker 完全一致，私有协议仍为 1 / worker version 0.2.0。

Rust 启动 Windows worker 时增加 `CREATE_NO_WINDOW`，保留原来的 stdin/stdout JSONL 管道。查询与人工终端共用此进程启动实现；运行三种驱动和人工终端后，可见 worker 控制台窗口数量为 0。Windows 构建脚本也会重新生成许可证清单，使包内清单绑定当前三个锁文件。

## 实际测试结果

| 验证项 | 结果 | 证据与范围 |
| --- | --- | --- |
| 0.2.1 NSIS 原生安装 | PASS | 安装退出码 0，实际安装的主程序和 worker 已核验 |
| 原生 Windows worker UTF-8 JSONL | PASS | ping、拒绝无效操作、拒绝不存在的会话；中文错误文本可解析，退出码 0 |
| Tauri / WebView2 / Rust IPC | PASS | 真实窗口、原生后端、包内 worker，运行时版本 0.2.2；无浏览器后端桩 |
| 0.2.1 → 0.2.2 升级 | PASS | 安装退出码 0，注册版本 0.2.2，三个测试设备、主机信任和模型设置保留 |
| 三种 SSH 驱动 | PASS | Windows 上的华为、锐捷、新华三驱动连接本地 SSH 服务，执行端口/配置/LLDP 查询；厂商 CLI 是明确的脚本仿真 |
| Windows Credential Manager | PASS | 升级后重新连接三个设备成功；Windows 原生凭据 API 对三个已知测试 UUID 的值匹配成功，只记录匹配结果 |
| 缺少 Git 的失败路径 | PASS | 原始用户 PATH 没有 Git，备份返回 `BACKUP_GIT_UNAVAILABLE`；配置任务为 `not_started` / `write_attempted=false`，远端没有收到该批命令 |
| 成功备份与历史 | PASS | 使用虚拟机已有的便携 Git，三个设备各有 3 条成功备份；9 条记录均通过解密/完整性校验与脱敏预览 |
| AI 真实配置执行链 | PASS | Windows WebView2 IPC → Rust/Rig → 包内 Windows worker → Netmiko → SSH；三种驱动均实际下发、保存、回读，状态 `succeeded` / `save_status=verified` |
| 计划预览隔离 | PASS | `ai_plan` 返回未执行，成功备份数量不变 |
| 不重复下发、模型数据边界 | PASS | 三种仿真设备各只收到一次对应 AI 配置批次；11 次脚本模型请求均未包含样例设备密码和配置敏感字段 |
| 人工终端传输接口 | PASS | 实际 SSH PTY 经原生 IPC 完成中文描述、保存确认、运行期间 Ctrl+C、尺寸变化与关闭；未把 IPC 验证等同于完整键盘/输入法 GUI 验收 |
| Windows 双实例与任务状态 | PASS | 第二实例因 `INSTANCE_RUNNING` 退出 101；首实例同一个 AI 任务前后均为 `running`，随后因受控模型断开正常记为 `failed`，没有被改为 `interrupted` 或产生 `TASK_STATE` |
| 数据落盘审计 | PASS | 设备 UUID 集合恰为本次测试创建的三个 UUID；数据库及 11 个 `.ygb` 对象没有样例密码/敏感配置明文。11 个对象中只有 9 条登记为成功，缺少 Git 时产生的对象未伪报成功 |
| 原生卸载 | PASS | 卸载器退出码 0，主程序与卸载注册均已移除；等待 NSIS 临时卸载子进程完成后确认结果 |
| 资源收尾 | PASS | 三条测试设备凭据及本次生成的备份密钥已删除；测试目录、应用测试数据、WebView 本地缓存、测试 Python/SSH/模型进程均已清理，虚拟机继续运行 |
| Rust 回归与静态检查 | PASS | 本次 18 项 Rust 测试、格式检查、Clippy；前端类型检查与生产构建包含在 Windows 构建中。未把旧版本 84 项测试计作本轮重新运行 |

## 条件与未覆盖项

成功备份和 AI 配置测试使用既有 既有内部测试机上的 Git（Git 2.56.0.windows.2，路径已脱敏），只给测试客户端进程临时增加 PATH，没有修改系统 PATH。安装包仍不包含 Git，目标电脑需要可从 PATH 调用 Git 才能完成备份和依赖备份的配置执行。

WebView2 已预装，未卸载共享虚拟机的运行时来测试首次联网引导。未覆盖 Windows 10、中文用户名/安装路径、完整输入法/剪贴板 GUI 流程、真实厂商型号、真实模型服务或代码签名。当前安装包未签名，属于本地候选包。

第二实例目前通过 Tauri 初始化失败退出，stderr 含 setup panic 日志；已经核实首实例与任务不受影响，尚未实现自动聚焦已有窗口。

首轮卸载检查过早，只观察到了启动临时卸载子进程的父进程退出，短暂误报文件仍存在。保留首轮报告，随后只读确认主程序/注册均已移除，再继续审计与收尾，没有重复执行卸载。测试工具已增加等待实际删除完成的检查。

## 证据

汇总：`builds/validation-windows-vm-0.2.2.json`。

本轮目录：`builds/windows-vm-0.2.1-20261024/`，其中：

- `native-022-functional.png`：实际 Windows 11 / WebView2 窗口。
- `first-launch.png`：原 0.2.1 worker 黑色控制台问题。
- `functional-summary.json`：三设备配置、保存、回读和备份记录。
- `instance-final-report.json`：双实例时活跃任务状态及第二实例实际退出码。
- `guest-evidence/uninstall-report.json`、`final-observation.json`：凭据、数据审计与收尾结果。
- `package-provenance-022.json`：包内程序、实际安装程序及许可证锁文件一致性。
- `rust-fix-regression.log`、`clippy-022.log`、`windows-final-bundle-0.2.2.log`：回归和构建日志。
- `uninstall-first-report.json`、`instance-first-report.json`：保留的测试工具首轮观测，不覆盖为成功记录。

可复用的 guest 测试脚本位于 `tests/windows/`；使用单次 WebView2 CDP 调用驱动真实 Tauri IPC，未给应用源代码加入测试后端。测试脚本服务和 Windows Python 测试运行时仅用于本次验证，不安装为后台服务。
