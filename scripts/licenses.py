"""Inventory exact resolved dependencies and retain their shipped license texts."""
import hashlib
import importlib.metadata
import json
import pathlib
import re
import shutil
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[1]
destination = root / 'LICENSES'
texts = destination / 'texts'
texts.mkdir(parents=True, exist_ok=True)
records = []


def record(ecosystem, name, version, license_id, source, files):
    safe = re.sub(r'[^\w.-]', '_', f'{ecosystem}-{name}-{version}')
    copied = []
    for path in files:
        if not path.is_file():
            continue
        target = texts / safe / path.name
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists() and target.read_bytes() != path.read_bytes():
            target = target.with_name(hashlib.sha256(str(path).encode()).hexdigest()[:8] + '-' + path.name)
        shutil.copyfile(path, target)
        copied.append(str(target.relative_to(destination)))
    records.append({'ecosystem': ecosystem, 'name': name, 'version': version, 'license': license_id, 'source': source, 'license_files': sorted(set(copied))})


metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--manifest-path', str(root / 'app/src-tauri/Cargo.toml'), '--all-features', '--locked', '--format-version', '1']))
rig_core=next(p for p in metadata['packages'] if p['name']=='rig-core')
rig_path=pathlib.Path(rig_core['manifest_path']).parent
rig_commit=json.loads((rig_path/'.cargo_vcs_info.json').read_text())['git']['sha1']
for package in metadata['packages']:
    if package['name'] == 'yigou':
        continue
    base = pathlib.Path(package['manifest_path']).parent
    files = [p for p in base.iterdir() if re.match(r'(?i)(licen[cs]e|copying|notice)', p.name)]
    if package.get('license_file'):
        files.append(base / package['license_file'])
    if not files and package['name'].startswith('rig-') and package.get('repository')==rig_core.get('repository'):
        vcs=base/'.cargo_vcs_info.json'
        if vcs.exists() and json.loads(vcs.read_text())['git']['sha1']==rig_commit:
            files=[rig_path/'LICENSE']
    record('cargo', package['name'], package['version'], package.get('license'), package.get('repository'), files)

# The installed, locked pnpm store is the source of package versions and licenses.
seen = set()
pnpm_store=root / 'app/node_modules/.pnpm'
for path in list(pnpm_store.glob('*/node_modules/*/package.json'))+list(pnpm_store.glob('*/node_modules/@*/*/package.json')):
    try:
        package = json.loads(path.read_text())
    except (OSError, ValueError):
        continue
    name, version = package.get('name'), package.get('version')
    if not name or not version or (name, version) in seen:
        continue
    seen.add((name, version))
    files = [p for p in path.parent.iterdir() if re.match(r'(?i)(licen[cs]e|copying|notice)', p.name)]
    if not files:
        parent_name={'@esbuild/linux-x64':'esbuild','@rollup/rollup-linux-x64-gnu':'rollup','@tauri-apps/cli-linux-x64-gnu':'@tauri-apps/cli'}.get(name)
        if parent_name:
            parent=root/'app/node_modules'/parent_name
            if parent.exists():files=[p for p in parent.iterdir() if re.match(r'(?i)(licen[cs]e|copying|notice)',p.name)]
    record('pnpm', name, version, package.get('license'), package.get('repository'), files)

for package in importlib.metadata.distributions():
    files = [pathlib.Path(package.locate_file(p)) for p in package.files or [] if re.search(r'(?i)(^|/)(licen[cs]e|copying|notice)', str(p))]
    if package.metadata['Name']=='yigou-worker':files=[root/'LICENSE']
    license_id = package.metadata.get('License-Expression') or package.metadata.get('License')
    record('uv', package.metadata['Name'], package.version, license_id, package.metadata.get('Home-Page'), files)

python_base=pathlib.Path(sys.base_prefix)
python_licenses=list(python_base.glob('LICENSE*'))+list(python_base.glob('lib/python*/LICENSE*'))
record('runtime','CPython',sys.version.split()[0],'Python-2.0','https://www.python.org/',python_licenses)

locks = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in [root / 'app/src-tauri/Cargo.lock', root / 'app/pnpm-lock.yaml', root / 'worker/uv.lock']}
(destination / 'dependency-inventory.json').write_text(json.dumps({'scope': 'Resolved development and runtime dependencies; not a legal certification', 'locks_sha256': locks, 'packages': sorted(records, key=lambda r: (r['ecosystem'], r['name']))}, ensure_ascii=False, indent=2) + '\n')
missing = [r for r in records if not r['license_files']]
(destination / 'README.md').write_text('# 第三方依赖清单\n\n`dependency-inventory.json` 绑定三个锁文件的 SHA256，记录依赖精确版本、许可证元数据及可取得的许可证正文。包括构建和测试依赖，不等于安装包实际包含全部依赖。\n\n生成命令：`source scripts/env.sh && uv run --project worker python scripts/licenses.py`。\n\n未附独立许可证正文的包需要正式发布前复核（部分包将许可证写在 README 或元数据中）：\n\n' + '\n'.join(f'- {r["ecosystem"]} / {r["name"]} {r["version"]}' for r in missing) + '\n')
print(json.dumps({'packages': len(records), 'without_standalone_license_text': len(missing)}))
