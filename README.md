# 意构 · Yigou

第一版网络设备桌面工作台：**设备列表 + AI 对话 + 终端与执行结果**。前端为 Vue 3 / TypeScript，桌面壳为 Tauri 2，业务逻辑为 Rust / Tokio，AI 使用 Rig，设备查询使用独立 Python / Netmiko 执行程序，人工终端使用 Paramiko 交互式 SSH PTY。

当前为 v6.2 源码集成候选版。**2026-11-04 已完成本地 Rust/Python/前端回归与 Linux 冻结设备执行程序的脚本化 SSH 验证；尚未验收新版桌面安装包、真实厂商设备及在线模型服务。** 2026-11-03 的初始源码交付状态见 [v6.2 安全集成说明](docs/secure-integration-v62-source.md)，后续实测范围见 [验证记录](docs/validation.md)。2026 年 11 月新增了人工审批与受限自主执行流程；新版 Windows 11、macOS 原生验收尚未完成。测试 SSH 服务与测试模型仅位于测试目录；应用没有演示设备或伪造设备输出。

## 开发与启动

工作区：`~/yigou`（示例，任意本地路径）。需要 Rust 1.99、Node.js 24+、pnpm 11、uv、Git。Linux 需要 GTK 3、WebKitGTK 4.1，以及可用且已解锁的 Secret Service（例如 GNOME Keyring）。本次构建用的 SDK 解包在 `builds/sysroot/`，未安装系统业务服务。

```bash
cd ~/yigou
source scripts/env.sh
uv sync --project worker --frozen
pnpm --dir app install --frozen-lockfile --store-dir "$YIGOU_ROOT/builds/pnpm-store"
bash scripts/build-worker.sh
bash scripts/dev.sh
```

`pnpm --dir app dev` 仅启动浏览器预览，设备功能不可用。完整功能必须通过 Tauri 桌面客户端使用。在无图形桌面的服务器上，原生启动测试使用 Xvfb，不能替代用户桌面的交互验收。

本机采用项目内 SDK 时，`scripts/dev.sh` 使用已有的 bubblewrap 创建临时挂载命名空间，让 WebKit 找到固定路径的辅助程序；宿主机 `/usr` 不变。开发数据及 WebView 缓存均保存在项目内。正常已安装系统依赖的桌面无需此映射。

## 使用顺序

1. 添加设备：厂商、地址、SSH 端口、用户名、密码或私钥路径、自动备份周期。凭据进入系统凭据库；凭据库不可用时保存失败，无明文替代路径。
2. 点击“核实 SSH 指纹”。从设备控制台或可信管理员获取指纹，独立比对后输入完整 SHA256 指纹并登记。未知或变化的密钥会被拒绝。
3. 连接设备。开启自动备份的设备在连接后备份；备份失败独立显示，不能把 SSH 连接成功当作备份成功。
4. 使用设备信息、端口、配置与 LLDP 邻居查询。邻居结果含命令、原始输出和每条关系的来源文本，解析失败不会被解释成“没有邻居”。
5. 模型设置中填写兼容接口的 **base URL（通常到 `/v1`）**、精确模型 ID 和 API 密钥。需要 Chat Completions、流式输出和函数工具调用支持。远程接口必须为 HTTPS，本机 HTTP 仅允许 localhost / 127.0.0.1 / ::1。本机无认证服务可留空密钥；已有密钥时留空表示保留。
6. 输入中文指令时可以选择三种方式：默认只读对话；在“助手工作模式”选择“先给我看方案”后逐条审阅、确认风险并人工审批；或主动选择“授权自主处理”并确认本轮授权，只对当前设备和本次请求授权，免除逐条审批。自主模式要求模型能够可靠调用查询/计划工具、先回读配置，Rust 仅允许 VLAN 命名、接口描述等受限低风险命令；高风险变更自动转人工审批。两种写入路径均验证设备会话、计划哈希、有效期、前置加密备份、回读及后置备份。自主模式及授权发送后立即重置，不自动沿用；模型无法修改执行权限或决定自身是否“足够强”。结果必须以实际记录为准。

人工操作：点击 **“＋ SSH 终端”**，即可像普通 SSH 客户端一样输入命令，进入配置模式、修改和保存配置。支持多标签、Tab 补全、方向键、Ctrl+C、确认提示、密码提示、多行粘贴和终端尺寸同步。命令权限由设备账户决定，应用不设命令白名单或逐条审批。已登记主机指纹后可直接打开终端，无需先连接 AI 查询会话。

每个终端使用独立的 SSH 会话和执行进程；AI 操作、备份或查询会话重连不占用人工终端。关闭一个标签只关闭对应会话，“断开”关闭该设备的查询和人工会话。终端原始输出按设备实际内容显示，不送入 AI、不保存终端输入或原始录屏日志（包括密码提示输入），只记录会话开关和错误元数据。大段粘贴按实际接收字节继续发送，输出持续读取；Ctrl+C 清除尚未发送的粘贴余量。

配置执行详情保存在操作日志中，包含脱敏命令、执行前后备份、逐项回读证据和保存状态。仅 `succeeded` 代表核验及备份完成。部分执行、保存失败、核验不符、缺少目标核验、执行后备份失败和连接中断分别显示；结果不确定时不自动重放。执行前备份失败则不下发。每份计划只绑定一个完整配置批次（人工审批最多 256 行，自主执行最多 12 行）；计划编号一经执行即被消耗，重复请求不重新下发；自主执行请求编号持久去重。回读匹配条件反映本次检查范围，不能替代真实型号和业务连通性验收。多设备联动及自动恢复尚未实现。

查询会话初始化可能由 Netmiko 关闭分页、设置会话终端宽度；锐捷驱动可能进入 enable 模式。人工终端直接进入设备 shell，不自动发送初始化或配置命令。

## 备份与数据

- 开发数据位于 `data/`，含 SQLite、主机信任记录和 `data/backups/`；根目录 `backups/` 预留给人工导出的加密资料，不另起业务服务。
- 发布版本使用 Tauri `app_data_dir()`：Linux 通常为 `~/.local/share/io.yigou.desktop`，Windows/macOS 使用相应系统应用数据目录。安装路径不用于保存用户数据。
- 设备密码、私钥路径/口令、API 密钥、随机生成的备份加密密钥放入操作系统凭据库。现有 SSH 私钥文件由用户自行保管，不复制到项目中。
- 完整配置使用 AES-256-GCM 加密。每份 `.ygb` 有独立随机 nonce，关联设备、记录 ID、原文 SHA256。文件写入、Git 提交、重新解密校验和 SQLite 记录均成功后才报告成功。Git 历史只包含加密对象。
- 每个备份目录有独立密钥标识，并以跨进程文件锁保护密钥初始化和备份提交；旧版全局密钥格式仍可读取。迁移目录后，旧备份继续使用头部记录的原密钥，新备份使用新目录密钥；应保留所有被备份引用的密钥。
- 同一数据目录仅允许一个客户端实例。第二个实例在打开数据库和恢复任务之前被拒绝，不能把运行中任务误标为中断；进程退出会释放锁。中断的配置操作保留为结果不确定，不自动重试。
- 备份预览及 AI 查询进行脱敏；备份本体保留完整原文。脱敏是保守的规则过滤，应在接入新型号时补充相应凭据格式。
- 自动备份只在客户端运行且设备已连接时进行。失败写入任务日志，下一周期重试；关闭应用不维持后台定时服务。
- **恢复边界：** 加密文件自带经认证的元数据。备份历史中的“重建记录”可在设备记录仍存在时恢复丢失的备份索引，并检查 Git 记录和配置完整性；支持脱敏预览，不向设备下发恢复配置。整个数据库丢失时还需恢复设备记录。迁移数据需要同时迁移对应系统凭据库中的备份密钥；丢失密钥无法解密备份。跨机器密钥导入/导出界面尚未实现。
- 对话仅存在内存中，按设备和对话/计划隔离，断开设备时清除；超出上下文预算时整段重置，避免截断工具调用配对。

## 验证与构建

```bash
cd ~/yigou
source scripts/env.sh
uv run --project worker pytest -q worker/tests
cargo test --manifest-path app/src-tauri/Cargo.toml --lib --locked
uv run --project worker ruff check --config worker/pyproject.toml worker tests/lab_server.py
pnpm --dir app build
# 可指定已有 Chromium 路径，避免下载到工作区外。
YIGOU_CHROMIUM=/path/to/chrome pnpm --dir app test
bash scripts/build-linux.sh
```

浏览器 E2E 也可在独立副本中运行，但副本必须自包含：先在副本自己的 `app/` 中按 `app/pnpm-lock.yaml` 执行 `pnpm --dir app install --frozen-lockfile`，再运行测试。**不要**用目录 junction 或符号链接把另一个检出的 `app/node_modules` 链入副本。Vite 开发服务器只读取项目根内的文件，会把这类项目外字体请求以 403 拒绝，而测试可能因回退字体仍然“通过”。修复方式是安装项目内依赖，不得扩大 `server.fs.allow` 或放宽 CSP；`app/playwright.config.ts` 检测到 `app/node_modules` 解析到副本外时会直接报错。浏览器页签图标来自 `app/public/favicon.ico`，由 `app/src-tauri/icons/icon.ico` 复制，生产构建将其复制到 `dist` 并由构建检查确认存在。

Linux 安装包产物位于 `builds/target/release/bundle/deb/`。包含冻结后的 Python 执行程序，不要求用户安装 Python/uv。系统仍需 GTK/WebKitGTK、Git 和系统凭据库。包内的第三方依赖版本清单见 `LICENSES/`。

### Windows 64 位（在 Linux 上交叉打包）

```bash
cd ~/yigou
bash scripts/build-windows-nsis.sh
```

Windows 版采用 `cargo-xwin` / MSVC / LLVM 构建 Tauri 主程序，使用 Windows CPython + Wine + PyInstaller 打包独立的 SSH worker，再由 NSIS 生成安装包。当前 0.2.5 候选产物为 `builds/target/x86_64-pc-windows-msvc/release/bundle/nsis/Yigou_0.2.5_x64-setup.exe`。Windows PE worker 的三帧协议已由独立宿主执行器在 Wine 下通过（`WINE_ONLY`）；0.2.5 的 Windows 11 原生安装、WebView2、中文字体、11 平台/8 新增只读标记和窗口按钮验收仍为 **NOT_RUN**，尚无 guest 原生结果 JSON。内部测试实验室的详细通道诊断记录保留在私有开发仓库；公开源码不包含这些环境专用测试脚本。

历史 0.2.2 曾在服务器的 Windows 11 x64 虚拟机完成原生安装、升级、WebView2、Windows 凭据库、三厂商仿真 SSH、AI 配置/保存/回读、加密备份与卸载验证，并修复 worker 控制台弹窗，详见 [Windows 0.2.2 虚拟机记录](docs/validation-windows-vm-0.2.2.md)；历史交叉构建见 [0.2.1 记录](docs/validation-windows-0.2.1.md)。真实设备、真实模型和物理 Windows 尚未测试。安装包未签名；缺少 WebView2 时默认联网下载引导程序，配置备份需要可从 PATH 调用 Git。macOS 仍未构建。

## 接口与依赖升级

前端仅调用 [稳定接口](docs/api.md)，不含厂商命令或配置逻辑。Python JSONL 协议只通过子进程标准输入/输出传输，不监听端口、不直接暴露给前端。

`Cargo.lock`、`pnpm-lock.yaml`、`uv.lock` 均纳入版本管理。`python3 scripts/check-updates.py` 只检测上游并写入 `builds/dependency-updates.json`，不更新锁、不合并、不部署。仓库在 GitHub 启用 Actions 后，提供的周度检测工作流才会运行；本地不安装定时服务。

升级顺序：创建 `deps/<topic>` 分支 → 更新指定依赖与锁文件 → 编译 → 运行回归与型号验证 → 更新许可证清单 → 人工审核合并。`configs/renovate.json` 显式关闭自动合并。自研业务逻辑只依赖公开 API，不修改上游源文件。

技术参考：[Tauri sidecar](https://v2.tauri.app/develop/sidecar/)、[Rig](https://rig.rs/docs/integrations/model_providers/openai/)、[Netmiko 官方驱动](https://github.com/ktbyers/netmiko)。确切依赖 API 以锁定版本的源码与编译结果为准。

## 安全与公开

安全问题报告途径与测试边界见 [SECURITY.md](SECURITY.md)。对外公开源码必须遵循 [公开前检查清单](docs/public-release-checklist.md) 的无历史导出流程；仓库内不含真实设备凭据、私钥或生产数据。`tests/windows/` 中依赖内部 SMB 共享/QEMU 监控通道的脚本属于私有环境验收工具，公开导出时按清单排除。
