"""Read-only upstream version detection. Does not change locks, branches or services."""
import datetime
import json
import pathlib
import tomllib
import urllib.request

root = pathlib.Path(__file__).resolve().parents[1]
items = []


def fetch(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'yigou-dependency-check/0.1'})
    with urllib.request.urlopen(request, timeout=25) as response:
        return json.load(response)


locks = tomllib.loads((root / 'app/src-tauri/Cargo.lock').read_text())
for name in ['tauri', 'rig-core', 'rig-agent', 'rusqlite', 'keyring']:
    locked = next(p['version'] for p in locks['package'] if p['name'] == name)
    try:
        latest = fetch(f'https://crates.io/api/v1/crates/{name}')['crate']['max_stable_version']
        items.append({'name': name, 'manager': 'cargo', 'locked': locked, 'latest_stable': latest, 'different': locked != latest})
    except Exception as error:
        items.append({'name': name, 'status': 'CHECK_FAILED', 'error_type': type(error).__name__})
python_lock = tomllib.loads((root / 'worker/uv.lock').read_text())
locked = next(p['version'] for p in python_lock['package'] if p['name'] == 'netmiko')
try:
    latest = fetch('https://pypi.org/pypi/netmiko/json')['info']['version']
    items.append({'name': 'netmiko', 'manager': 'uv', 'locked': locked, 'latest': latest, 'different': locked != latest})
except Exception as error:
    items.append({'name': 'netmiko', 'status': 'CHECK_FAILED', 'error_type': type(error).__name__})
package = json.loads((root / 'app/package.json').read_text())
for name in package['dependencies']:
    try:
        installed = json.loads((root / 'app/node_modules' / name / 'package.json').read_text())['version']
        latest = fetch('https://registry.npmjs.org/' + name.replace('/', '%2f'))['dist-tags']['latest']
        items.append({'name': name, 'manager': 'pnpm', 'locked': installed, 'latest': latest, 'different': installed != latest})
    except Exception as error:
        items.append({'name': name, 'status': 'CHECK_FAILED', 'error_type': type(error).__name__})
result = {'checked_at': datetime.datetime.now(datetime.UTC).isoformat(), 'automatic_merge': False, 'dependencies': items}
path = root / 'builds/dependency-updates.json'
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
print(path)
