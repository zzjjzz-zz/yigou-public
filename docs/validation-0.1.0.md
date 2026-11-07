# 本地验证记录 · 0.1.0（历史版本）

本文件仅保留旧版本证据；当前交互式 SSH 终端及安装包状态见 [最新验证记录](validation.md)。

验证日期：2026-10-23。范围为用户确认的“本地开发与可复现测试，暂不连接真实设备”。工作区 `~/yigou`（当时的工作区路径，示例），Linux x86_64 / Ubuntu 24.04 构建环境。

| 验证项 | 状态 | 证据与边界 |
| --- | --- | --- |
| 完整本地检查脚本 | PASS，退出码 0 | 最终执行 `YIGOU_CHROMIUM=<chromium路径> bash scripts/check.sh`；`builds/local-check.log`，35 项 Python + 10 项 Rust + 4 项 UI，共 49 项；含格式、静态检查和前端生产构建 |
| Rust 核心与跨进程集成 | PASS，10 项 | `builds/rust-tests.log`；含 Rig 流式工具调用 → Rust → 冻结前 Python worker → 本地脚本化 SSH 设备 |
| Python / Netmiko | PASS，35 项 | `builds/python-tests.log`；三种厂商脚本化 CLI、严格主机密钥、密码/加密私钥认证、错误凭据、超时、命令拒绝、完整性与脱敏；不识别命令、权限不足和授权失败的设备输出必须失败 |
| Vue / TypeScript 生产构建 | PASS | `builds/frontend-build.log` |
| 浏览器 UI 合约测试 | PASS，4 项 | `builds/ui-tests.log`；浏览器无后端禁用、凭据库失败、查询结果与 AI 失败、模型配置；IPC fixture 仅在测试文件内 |
| Rust 静态检查 | PASS | `builds/clippy.log`，all-features / all-targets / `-D warnings` |
| Python 静态检查 | PASS | `builds/ruff.log` |
| 原生 Tauri / WebKitGTK 窗口 | PASS | `builds/native-smoke/desktop.png`，Xvfb 显示环境；从 Vue 发起的 `system_status` 已通过 Rust 调用真实 Python worker |
| OS 系统凭据库 | PASS | `builds/keyring-smoke.log`；私有 D-Bus 会话中的真实 GNOME Secret Service 存取/删除测试，未使用用户钱包 |
| PyInstaller sidecar | PASS | `builds/frozen-worker-test.log`；独立可执行程序在三种脚本化 SSH peer 上完成连接、查询、脱敏、完整配置读取及 LLDP；不等于真实设备验收 |
| 备份索引恢复 | PASS | Rust 测试删除备份索引后，从经认证加密文件与 Git 历史重建；缺失加密密钥时拒绝替换密钥 |
| Linux release 安装包 | PASS | `builds/linux-package.log`、`builds/final-bundle.log`；`.deb` 包含主程序、冻结的 Python worker、许可证资源和桌面入口，无 systemd service/timer |
| 安装包独立启动 | PASS | `builds/package-smoke-0.1.0/result.json`、`builds/package-smoke-0.1.0/desktop.png`；关闭 Vite 后从 `.deb` 提取文件运行，原生界面 → Rust → 包内 worker 握手通过 |
| 包内设备执行程序 | PASS | `builds/packaged-worker-test.log`；直接测试 `.deb` 内 sidecar 的三种脚本化 SSH peer，含失败输出判定；其 SHA256 与最终冻结程序一致 |
| 上游更新检测 | PASS（单次本地运行） | `builds/dependency-updates.json`；仅检测，没有更新锁文件或自动合并 |
| 真实华为/锐捷/新华三设备 | NOT_RUN | 用户选择暂不连接真实设备；未发生生产网络查询或写入 |
| 在线/本地实际大模型 | NOT_RUN | 未配置真实模型密钥；Rig 使用本地可复现 SSE 模型 fixture，不是能力或费用验收 |
| Windows / macOS 安装包 | NOT_RUN | 需要对应操作系统构建与测试 |
| 远程 CI / 发布 / 部署 | NOT_RUN | 本地 Git 已初始化，无 remote、无源码 commit、未推送；提供更新检查工作流但未启用远端调度 |

原生截图来自实际 Linux WebKit 窗口；浏览器截图 `builds/ui-preview.png` 则是明确禁用设备功能的预览。两者不能混用为设备验收。

最终安装包：`builds/target/release/bundle/deb/Yigou_0.1.0_amd64.deb`，36,700,576 字节（35.00 MiB）。SHA256：`899935a66cc686fe36f97940b324869f3f5f4528ff46b5704d60685848d31c53`。这是本地开发候选包，未做系统安装、公开发布或真实设备验收。打包后通过局部解包检查与实际原生启动验证；不会将此等同于系统安装/卸载测试。

包内 `yigou` SHA256：`ec48c46d90c094697e232eb7d3920a8b844ff9cd2a7af904b6eb66b4510cf832`。包内 `yigou-worker` SHA256：`b3eb1085bb9a1e4e3fbe26f84a347b229ac6c0c9822d2e85652d7c214b78b42c`。最终候选使用完整 release 编译结果和定向修正后的 worker 重新打包，`final-bundle.log` 为最终打包记录。

汇总证据和源文件指纹位于 `builds/validation.json` 与 `builds/source-manifest.json`，安装包校验文件为 `builds/SHA256SUMS`。临时 Vite、SSH fixture、模型 fixture、Xvfb 和隔离凭据库进程均已退出。

本机未安装 GTK/WebKit 开发包到系统。SDK 放在 `builds/sysroot/`；原生测试借助临时只读 mount namespace 映射 WebKit 固定 helper 路径，不修改 `/usr`。开发 WebView 数据显式指定项目内目录，测试生成的早期 8 字节 WebKit 缓存已按精确路径移除。没有常驻业务服务。

硬件图形加速在 Xvfb 环境不可用，测试使用软件渲染。最终仍需要在用户的 Linux 桌面检查实际交互和系统凭据库解锁体验。

正式发布前边界：三种驱动尚无真实型号认证；当前基础发现只实现 LLDP，CDP 未接入；版本信息保留设备原文，不声称可靠自动识别所有型号。配置修改和设备恢复接口不存在。第三方清单记录精确版本和可取得的许可证正文，缺少独立许可证正文的包在 `LICENSES/README.md` 中列出，正式发布前需完成对应法律文件核查。
