"""Download the apt resolver's missing packages into a private, non-installed SDK."""
import concurrent.futures
import pathlib
import re
import subprocess
import urllib.request
import sys

root = pathlib.Path(__file__).resolve().parents[1]
dest = root / 'builds/sysroot'
cache = root / 'builds/debs'
cache.mkdir(parents=True, exist_ok=True)
dest.mkdir(parents=True, exist_ok=True)
entries = re.findall(r"^'([^']+)' (\S+) (\d+) (\S+)$", (root / (sys.argv[1] if len(sys.argv)>1 else 'builds/sdk-uris.txt')).read_text(), re.M)
if not entries:
    raise SystemExit('No apt package URIs available')

def download(entry):
    url, name, length, checksum = entry
    url = url.replace('http://cn.archive.ubuntu.com', 'https://archive.ubuntu.com').replace('http://security.ubuntu.com', 'https://security.ubuntu.com')
    path = cache / name
    if not path.exists():
        with urllib.request.urlopen(url, timeout=90) as response, path.open('wb') as out:
            import shutil
            shutil.copyfileobj(response, out)
    if path.stat().st_size != int(length):
        raise RuntimeError('Size mismatch: ' + name)
    algorithm, expected = checksum.split(':', 1)
    import hashlib
    if hashlib.new(algorithm.lower().replace('sum', ''), path.read_bytes()).hexdigest() != expected:
        raise RuntimeError('Checksum mismatch: ' + name)
    return path

with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
    for i, path in enumerate(pool.map(download, entries), 1):
        subprocess.run(['dpkg-deb', '-x', str(path), str(dest)], check=True)
        print(f'{i}/{len(entries)} {path.name}', flush=True)

for path in dest.rglob('*.pc'):
    content = path.read_text()
    content = content.replace('prefix=/usr', 'prefix=' + str(dest / 'usr'))
    content = re.sub(r'(?<=[= :])(/usr/(?:lib|include|share)/)', str(dest) + r'\1', content)
    path.write_text(content)
print('Private SDK ready:', dest)

# apt does not download already-installed runtime libraries. Resolve their dev
# symlinks to the existing system files; no system paths are modified.
for path in dest.rglob('*.so'):
    if path.is_symlink() and not path.exists():
        system = pathlib.Path('/') / path.parent.relative_to(dest) / path.readlink()
        if system.exists():
            path.unlink()
            path.symlink_to(system.resolve())
