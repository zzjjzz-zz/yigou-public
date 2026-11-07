# 许可证核对与待审清单（公开前）

> 本文件是对 `LICENSES/README.md` / `dependency-inventory.json` 的**人工核对记录**（2026-11-07，共 669 个包），由 `scripts/licenses.py` 重新生成时**不会**覆盖。它不构成法律意见，也不宣称全部合规。

## 一、清单状态核对

- `dependency-inventory.json` 的 `locks_sha256` 与三个锁文件当前 SHA256 一致（本次实测相符）。
- 669 包中：4 包 `license` 字段为空（见下）；其余均有 SPDX 或文本许可证标识。
- `LICENSES/README.md` 中"未附独立许可证正文"列表（45 项）仍然有效，正式分发前需人工补正文。

## 二、license 字段为空的 4 包（已核对随附正文）

| 包 | 实际许可证（依据其随附 LICENSE 文件） | 结论 |
|---|---|---|
| uv / markdown-it-py 4.2.0 | MIT（正文为标准 MIT 文本） | 可补：元数据缺失，正文明确 MIT |
| uv / mdurl 0.1.2 | MIT（正文为标准 MIT 文本） | 可补：同上 |
| uv / pyinstaller-hooks-contrib 2026.9 | **GPL-2.0-or-later（hooks 部分）+ Apache-2.0（runtime hooks 部分，见其 LICENSE 文件分段说明）** | 需人工确认适用分段；见第五节 |
| uv / yigou-worker 0.2.0 | 本项目自身（MIT，`texts/uv-yigou-worker-0.2.0/LICENSE` 即仓库 LICENSE） | 无需外部处理 |

前三项属于"元数据缺失但正文可判定"；正式发布时建议在 `dependency-inventory.json` 修订上述 license 字段（由维护者审核后修改，本次未擅自改写生成物，避免与 `licenses.py` 再生成冲突）。

## 三、弱/copyleft 许可证与源码开源 vs 二进制分发的差异

### MPL-2.0（cargo：cssparser 0.37.0、cssparser-macros 0.7.1、dtoa-short 0.3.5、option-ext 0.2.0、selectors 0.38.0）

- **源码开源**：MPL-2.0 文件级 copyleft。以源码形式再分发这些 crate（含在源码仓库中引用）时需保留原文件许可证声明；与项目 MIT 共存可行。
- **二进制分发（安装包）**：MPL 允许与其他许可的代码静态/动态链接成闭源二进制，但**被 MPL 覆盖的单个文件源码须应请求提供**。Yigou 的 Windows/Linux 安装包静态链接这些 crate，发行时需在"关于/第三方许可"中附 MPL-2.0 文本与源码获取途径。**未做**：安装包内许可聚合页尚未逐一核对——列入待审。

### LGPL-2.1（uv：paramiko 5.1.0、scp 0.16.2；r-efi 的可选项之一）

- **源码开源**：依赖声明无问题。
- **二进制分发**：paramiko/scp 以**独立冻结进程**（PyInstaller 打包的 `yigou-worker` 可执行文件）方式分发，进程边界隔离通常满足 LGPL 的"独立作品"条件，但**需要保留 LGPL 文本与对应源码 offer**（PyInstaller 打包的集合按其 bootloader 例外见下）。r-efi 属于 Rust 静态链接的构建依赖（仅 Windows 目标引入），实际分发选择的是其 `MIT OR Apache-2.0` 分支之一，风险低但仍需在二进制许可清单中如实列出所有选项。
- **待人工确认**：PyInstaller onefile 打包 paramiko 后 LGPL 合规说明（ LGPL-2.1 §4/§6 对"聚合体"的要求）。

### PyInstaller 6.22.3（GPLv2-or-later + bootloader 例外）

- 其许可证带**明确例外**：允许用 PyInstaller 构建和分发非自由程序。Yigou 的 worker（MIT）经其打包**不被传染**为 GPL，这是该例外的设计用途。
- pyinstaller-hooks-contrib 2026.9 的构建期 hooks 与 runtime hooks 适用不同许可声明；后者可能进入冻结程序。应检查最终打包文件清单，不能一概认定 hooks 均不会随二进制分发。

### 其他需注意项

- OFL-1.1（2 项，字体）：正式分发时检查字体版权文本、保留字体名称（如适用）和修改条件。
- CDLA-Permissive-2.0（3 项）、Python-2.0（CPython 运行时）：宽松，源码开源无阻碍；二进制需附文本。

## 四、精确待审清单（发布前必须人工完成，不可由脚本代替）

1. **[人工]** 安装包内第三方许可聚合（NSIS/DEB 中嵌入的 LICENSES 汇总）逐项核对：MPL-2.0 五包、LGPL 两包、OFL 字体、PyInstaller 例外声明。
2. **[人工]** MPL-2.0 "文件源码应请求提供"的响应渠道写入 SECURITY.md/README 或发行说明。
3. **[人工]** LGPL-2.1 与 PyInstaller 聚合体的合规结论（建议外部法律复核，本文件不构成法律意见）。
4. **[人工]** `LICENSES/README.md` 45 个"无独立许可证正文"包补正文（多为许可证写在 README/元数据的包，如 objc2 系列、jni 系列、@esbuild 平台二进制）。
5. **[人工]** 修订 `dependency-inventory.json` 中 4 个空 license 字段（见第二节）——直接编辑生成物会被 `scripts/licenses.py` 覆盖，正确做法是给 `licenses.py` 增加包级 license 映射表后重新生成，属于后续维护任务，本次未做以避免破坏生成器行为。
6. **[确认无侵权指控后才能分发]** 本清单仅记录许可证元数据，**不**验证上游许可证声明真实性；无证据表明存在侵权，也不应据此宣称"已合规"。

## 五、结论

- **源码开源（MIT）**：目前未发现依赖许可构成明确阻碍，但本审查未完成全部第三方授权核验，不应视为最终法律结论。
- **二进制分发**：可行但需完成第四节 1–4 项；在完成前不应宣称"安装包已满足全部第三方许可义务"。
