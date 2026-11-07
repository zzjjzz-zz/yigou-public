# 公开源码检查清单

Yigou 当前开发仓库仍为私有。不得直接修改其公开状态，也不得推送或重写原有历史。公开版本应从**已审核、已提交且工作树干净**的源码建立新的独立仓库。

## 无历史导出（在私有仓库根目录执行）

以下命令只创建临时目录，不删除既有目录、不修改原仓库、不建立远端：

```bash
set -euo pipefail
test -z "$(git status --porcelain)"
EXPORT="$(mktemp -d "${TMPDIR:-/tmp}/yigou-public.XXXXXXXX")"
git archive --format=tar HEAD | tar -xf - -C "$EXPORT"

# 以下测试脚本专门操作内部虚拟机及共享目录，不属于通用公开源码。
rm -rf -- "$EXPORT/tests/windows"
rm -f -- "$EXPORT/scripts/stage-windows-023-review.py" \
  "$EXPORT/scripts/stage-windows-024-review.py" \
  "$EXPORT/scripts/stage-windows-025-review.py" \
  "$EXPORT/scripts/verify-windows-channel-025.py"
rm -f -- "$EXPORT/docs/windows-025-channel-diagnosis.md" \
  "$EXPORT/docs/validation-windows-025-launcher.md"

# 阻断已知的私人标识，连同本说明文件一并扫描。
python3 - "$EXPORT" <<'PY'
from pathlib import Path
import sys

root = Path(sys.argv[1])
needles = ("/home/" + "zz" + "jjzz", "host" + ".lan", "Rust" + "Desk", "zz" + "jjzz")
for file in root.rglob("*"):
    if not file.is_file():
        continue
    try:
        content = file.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        continue
    for needle in needles:
        if needle in content:
            raise SystemExit(f"PRIVATE_IDENTIFIER_FOUND: {file.relative_to(root)}")
if any(path.name == ".git" for path in root.rglob(".git")):
    raise SystemExit("GIT_HISTORY_FOUND")
print("PUBLIC_SOURCE_SCAN_PASS")
PY
printf 'Reviewed export: %s\n' "$EXPORT"
```

注意：扫描只涵盖当前约定的标识，不等于穷尽秘密扫描。还必须人工检查导出树中的域名、内部地址、作者信息、第三方素材及可执行构建逻辑。发现命中应停止公开、在私有分支修复后重新导出，不能删除报错来绕过检查。

## 对外发布前的人工验收

- 导出目录没有原仓库的 `.git` 历史。新仓库从一个新的初始提交开始，提交作者须使用自己已配置的公开身份或平台隐私邮箱。
- 检查 `git archive HEAD` 对应的提交及导出文件清单，确认无真实密码、密钥、证书、设备配置、私有测试工件；使用 Gitleaks（密钥扫描工具）扫描新的导出仓库。
- 从导出目录执行锁文件安装、Python/Rust/前端测试及构建，记录成功和未执行项。依赖私有实验室的 Windows 原生设备验收不包含在公开版本中。
- 核对 `LICENSES/dependency-inventory.json` 与三份锁文件的哈希，并处理 [许可证待审清单](../LICENSES/PRE-RELEASE-REVIEW.md)。公开源码与发布安装包应分别核对许可义务。
- 新仓库设置受保护的默认分支、必要的状态检查与私密漏洞报告渠道。工作流文件不代表这些平台设置已经启用。
- 未完成真实厂商设备、正式 Windows 安装包和在线模型验收前，不得宣传为生产网络已验收版本。

## 私有保留范围

原仓库的历史提交、工作流历史、内部 Windows 虚拟机控制脚本及其配套测试均只保留在原私有仓库；上面的导出只排除公开版本中的文件，不删除私有原件。

不应同时维护两套长期分叉的业务实现：公开仓库建立后，业务代码应有唯一的正式维护源，内部环境脚本与凭据继续独立保存。
