# Windows 0.2.1：Linux 交叉构建与验证

此工程的 Windows 发布目标是 **Windows x64 / Tauri 2 / MSVC / NSIS**。源码、编译缓存与安装包均在 `~/yigou`（当时的工作区路径，示例） 下保存。

## 构建产物（2026-10-24）

- **安装文件**：`builds/target/x86_64-pc-windows-msvc/release/bundle/nsis/Yigou_0.2.1_x64-setup.exe`
- **字节数**：24,786,506
- **SHA256**：`b72e77ca326cf84224552cb3f74752161749cf2ed46bea08f25ed0056c2aceb6`
- **主程序**：`yigou.exe`，Windows x64 PE GUI 程序，约 23 MB
- **设备驱动**：`yigou-worker.exe`，Windows x64 PE 控制台程序，约 18.9 MB
- **设备驱动 SHA256**：`534d5bc7556c361c931735cc9749f01c85e09545ca206bf70183ada5e83a17ce`

核验记录：

- `cargo-xwin` / MSVC 目标 Rust Release 构建退出码 **0**。
- PyInstaller Windows 设备驱动冻结完成，`worker-build.status` 为 **0**。
- Linux 的 `file` 确认 Windows 主程序、worker 为 PE32+ x86-64。
- `7z t` 完整校验 NSIS 安装包 **通过**，总计 1,086 个打包文件。
- 从安装包直接流式解出的 worker SHA256 与原始 Windows worker **完全一致**。
- Windows 主程序在 NSIS 中仅因 Tauri bundle 标记从 UNK 改为 NSS，其他字节与交叉编译产物一致。
- Wine 下的原生 Windows worker 协议测试 **通过**：`ping`（协议 1）、拒绝无效操作、拒绝不存在的会话。未连接设备。
- 修复了 Windows 控制台默认 cp1252 编码导致中文错误响应 `UnicodeEncodeError` 的问题；Windows worker 的 stdout 明确按 UTF-8 JSON Lines 输出。

## 从 Linux 重复构建

使用项目本地 Rust、LLVM 18、NSIS 和 pinned `cargo-xwin 0.23.1`：

```bash
cd ~/yigou
bash scripts/setup-windows-cross.sh
bash scripts/build-windows-worker.sh
xvfb-run -a python3 scripts/test-windows-worker.py
bash scripts/build-windows.sh
```

主程序的 Windows 目标是 `x86_64-pc-windows-msvc`；Windows Python 运行时是 3.12.10，PyInstaller/Netmiko 由 Windows 兼容 Wheel 在独立 Wine 环境中冻结；**绝不把 Linux PyInstaller 二进制直接放进 Windows 安装包**。

`scripts/build-windows.sh` 负责单实例打包锁、调用真正的 Windows worker 协议测试、生成 NSIS 安装包以及校验打包后完整性与 worker SHA256。Windows 平台专用配置：`app/src-tauri/tauri.windows.conf.json`；Linux 的 DEB 构建配置保持独立。

## 已验证与仍需 Windows 本机验收的边界

**已完成：** 跨平台编译、Windows PE 主程序与驱动生成、NSIS 安装程序生成、包内文件一致性、Wine 下的 driver 协议测试。既有 Linux/Tauri 集成和前端自动化回归另行验证。

**尚未完成：**

1. **Windows 10/11 实机或 Windows 虚拟机安装、卸载以及 WebView2 窗口操作**。Wine 启动 worker 成功不能代替 Windows 原生 Tauri/WebView2 验收。
2. **真实华为、锐捷和新华三网络设备**的 Windows SSH、加密备份、主机密钥和配置执行。
3. **Windows Credential Manager**（Windows 系统凭据库）与多会话、故障注入的实机验收。
4. **安装文件的 Authenticode 数字签名**。当前无签名配置，Windows SmartScreen 可能警告。
5. **WebView2 运行时**：本安装程序采用 Tauri 的 `downloadBootstrapper` 模式；如果系统没有 WebView2，安装时需要联网。它不是全离线 WebView2 安装包。
6. **Git for Windows**：加密配置备份使用 Git 存储历史；目标 Windows 必须能从 PATH 运行 `git.exe`，否则备份和依赖备份的自动配置会失败。现有 NSIS 未打包 Git。
7. **AI 写配置安全审批**：现版后端还没有强制的逐命令人工确认，不应对生产网络直接授权自动写配置。

安装包为**Windows 候选版本**，不能仅凭交叉编译或 Wine 测试就宣称已完成生产级 Windows 验收。Git 主仓库尚无首个提交或远程推送。
