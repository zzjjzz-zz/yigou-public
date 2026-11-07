# 意构 Yigou 0.2.1 · Windows x64 交叉构建验证

后续已在服务器 Windows 11 虚拟机实测，并修复 worker 控制台弹窗生成 0.2.2；原生测试结果见 [Windows 虚拟机验证](validation-windows-vm-0.2.2.md)。下文保留此次交叉构建完成时的验收范围。

检查日期：2026-10-24 UTC，工作区 `~/yigou`（当时的工作区路径，示例）。

## 实际产物

- Windows 安装包：`builds/target/x86_64-pc-windows-msvc/release/bundle/nsis/Yigou_0.2.1_x64-setup.exe`
- 格式：NSIS 3 Unicode，x64 安装包；大小 **24,786,506 字节**（约 23.64 MiB）
- SHA256：`b72e77ca326cf84224552cb3f74752161749cf2ed46bea08f25ed0056c2aceb6`
- 主程序：`builds/target/x86_64-pc-windows-msvc/release/yigou.exe`，PE32+ Windows GUI x64
- Sidecar：`app/src-tauri/binaries/yigou-worker-x86_64-pc-windows-msvc.exe`，PE32+ Windows Console x64
- 源码版本：Tauri/Rust **0.2.1**。PyInstaller 冻结的私有 worker 协议版本仍返回 `version=0.2.0`，协议编号为 **1**；不应将 worker 的协议版本号误判为客户端版本。

## 构建与检查

| 项目 | 结果 | 实际证据或说明 |
|---|---|---|
| Windows Rust MSVC 编译 | **PASS** | `cargo-xwin`，`builds/windows/rust-build.status=0`；主程序经 `file` 验证为 PE32+ GUI |
| Windows Python worker | **PASS** | Windows CPython 3.12.10 + Netmiko/Paramiko + PyInstaller 6.16；`builds/windows/worker-build.status=0` |
| worker 启动与协议 | **PASS** | Wine + Xvfb 输入 `{"id":"win-worker-ping","op":"ping"}`，返回 `{"ok":true,"result":{"protocol":1,"version":"0.2.0"}}` |
| NSIS 封装 | **PASS** | Tauri `pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --config src-tauri/tauri.windows.conf.json --ci` 正常结束 |
| 安装包压缩完整性 | **PASS** | `7z t` 返回 `Everything is Ok`；1086 个文件 |
| 关键可执行文件完整性 | **PASS** | 包内含 `yigou.exe`、`yigou-worker.exe`、许可证；包内 worker 的 SHA256 与源 Windows sidecar 完全相同 |
| 主程序来源一致性 | **PASS** | NSIS 内主程序与当前编译输出长度一致，仅 Tauri bundle 标识 3 字节不同（`NSS` 对 `UNK`）；其他字节一致 |
| Windows 原生安装和图形窗口 | **NOT_RUN** | Wine 的 worker 测试不等于 WebView2 桌面客户端真实 Windows 验收 |
| Windows Credential Manager、Git 备份与真实设备 | **NOT_RUN** | 未在 Windows 本机安装运行，也未连接华为、锐捷、新华三设备 |
| 数字签名 | **NOT_SIGNED** | 未提供代码签名证书；发行后 Windows SmartScreen 可能警告 |

## 在 Linux 复现

```bash
cd ~/yigou
bash scripts/build-windows-nsis.sh
```

此脚本统一调用 `scripts/build-windows.sh`，串行锁定打包目录，准备已锁定的 cargo-xwin/LLVM/NSIS 环境，若缺少或源文件更新则重新构建 Windows Python/Netmiko worker，先通过 Wine 下的 UTF-8 JSONL 协议测试，然后用 Windows MSVC 构建 Tauri + NSIS，并检查压缩包完整性和 worker SHA256。打包使用 Windows 专用 `app/src-tauri/tauri.windows.conf.json`，不会将 Linux 的 `.deb` 配置误用到 Windows。

首次执行需要外网下载 Windows SDK、Python ZIP 和依赖 Wheel，构建缓存保存在 `builds/`；不改变主机系统的业务服务。Windows 安装器中的 WebView2 安装策略目前是 `downloadBootstrapper`，首次安装机器若没有 WebView2 需要网络。配置备份操作依赖 Git；安装程序当前未自动包含 Git。

**重要：** 当前仅是编译/打包与协议级候选验收，不能据此宣称 Windows 10/11 的界面、网络设备配置、安全审批或自动恢复已经通过生产验收。发布生产环境前，必须完成真实 Windows 机器安装/卸载、Windows 凭据存取、WebView2 和实际厂商设备端到端测试。
