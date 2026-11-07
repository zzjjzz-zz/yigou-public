"""Launch only this task's Xvfb/client processes and capture the actual WebKit window."""
import json
import os
import pathlib
import re
import select
import sqlite3
import subprocess
import sys
import time

root = pathlib.Path(__file__).resolve().parents[1]
release = len(sys.argv)>1
candidate = pathlib.Path(sys.argv[1]).resolve() if release else root / 'builds/target/debug/yigou'
release_tag = None
if release and candidate.suffix == '.deb':
    match = re.fullmatch(r'Yigou_(\d+\.\d+\.\d+)_amd64\.deb', candidate.name)
    if not match:
        raise RuntimeError('Expected Yigou_<version>_amd64.deb for package smoke test')
    release_tag = match.group(1)
    extracted = root / f'builds/package-check-{release_tag}'
    subprocess.run(['dpkg-deb', '-x', str(candidate), str(extracted)], check=True)
    binary = extracted / 'usr/bin/yigou'
else:
    binary = candidate
work = root / (f'builds/package-smoke-{release_tag}' if release_tag else ('builds/package-smoke' if release else 'builds/native-smoke'))
work.mkdir(parents=True, exist_ok=True)
runtime = work / 'runtime'
runtime.mkdir(mode=0o700, exist_ok=True)
# Distribution WebKit binaries have fixed /usr helper paths. A private mount
# namespace supplies the SDK at those paths without installing host packages.
overlay = root / 'builds/native-runtime-libs'
overlay.mkdir(exist_ok=True)
for base in [pathlib.Path('/usr/lib/x86_64-linux-gnu'), root / 'builds/sysroot/usr/lib/x86_64-linux-gnu']:
    for entry in base.iterdir():
        target = overlay / entry.name
        if target.is_symlink():
            target.unlink()
        target.symlink_to(pathlib.Path('/mnt') / entry.name if base == pathlib.Path('/usr/lib/x86_64-linux-gnu') else entry)
xlog = (work / 'xvfb.log').open('w')
display = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '1280x900x24', '-nolisten', 'tcp'], stdout=subprocess.PIPE, stderr=xlog, text=True)
client = None
try:
    if not select.select([display.stdout], [], [], 10)[0]:
        raise RuntimeError('Xvfb did not return a display')
    number = display.stdout.readline().strip()
    env = os.environ | {'DISPLAY': ':' + number, 'YIGOU_DATA_DIR': str(work / 'data'), 'YIGOU_SMOKE': '1', 'XDG_RUNTIME_DIR': str(runtime), 'XDG_DATA_HOME': str(work/'app-data'), 'XDG_CONFIG_HOME': str(work/'config'), 'XDG_CACHE_HOME': str(work/'cache'), 'WEBKIT_DISABLE_DMABUF_RENDERER': '1', 'LIBGL_ALWAYS_SOFTWARE': '1'}
    log_path = work / 'client.log'
    with log_path.open('w') as log:
        client = subprocess.Popen(['bwrap','--die-with-parent','--ro-bind','/','/','--bind',str(root),str(root),'--ro-bind','/usr/lib/x86_64-linux-gnu','/mnt','--ro-bind',str(overlay),'/usr/lib/x86_64-linux-gnu','--dev-bind','/dev','/dev','--proc','/proc','--',str(binary)], cwd=root, env=env, stdout=log, stderr=log)
        window = None
        for _ in range(40):
            if client.poll() is not None:
                raise RuntimeError(f'Client exited with {client.returncode}; see {log_path}')
            found = subprocess.run(['xdotool', 'search', '--name', '意构'], env=env, capture_output=True, text=True)
            if found.returncode == 0:
                window = found.stdout.splitlines()[0]
            if window and 'YIGOU_IPC_READY' in log_path.read_text():
                break
            time.sleep(0.5)
        else:
            raise RuntimeError('Native window / frontend-to-Rust-to-worker handshake was not verified')
        time.sleep(2)  # Allow WebKit to paint after the asynchronous IPC handshake.
        subprocess.run(['import', '-window', window, str(work / 'desktop.png')], env=env, check=True, timeout=10)
        database = work / ('app-data/io.yigou.desktop/yigou.sqlite3' if release else 'data/yigou.sqlite3')
        # Synthetic pending task in this smoke test's isolated database only.
        with sqlite3.connect(database) as connection:
            connection.execute("INSERT OR REPLACE INTO tasks(id,operation,status,started_at) VALUES('smoke-live-task','SMOKE_ONLY','running',datetime('now'))")
        second_log = work / 'second-client.log'
        with second_log.open('w') as second_output:
            second = subprocess.Popen(client.args, cwd=root, env=env, stdout=second_output, stderr=second_output)
            try:
                assert second.wait(timeout=15) != 0, 'Second instance unexpectedly accepted the same data directory'
            finally:
                if second.poll() is None:
                    second.terminate()
                    try:
                        second.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        second.kill()
                        second.wait(timeout=5)
        assert 'INSTANCE_RUNNING' in second_log.read_text(), second_log.read_text()
        assert client.poll() is None
        with sqlite3.connect(database) as connection:
            assert connection.execute("SELECT status FROM tasks WHERE id='smoke-live-task'").fetchone()[0] == 'running'
            connection.execute("DELETE FROM tasks WHERE id='smoke-live-task'")
        (work / 'result.json').write_text(json.dumps({'native_window': 'PASS', 'frontend_rust_worker_ipc': 'PASS', 'second_instance_rejected': 'PASS', 'live_task_preserved': 'PASS', 'display': 'Xvfb', 'artifact':str(binary),'packaged_build':release,'real_device': 'NOT_RUN', 'online_model': 'NOT_RUN'}) + '\n')
        print((work / 'result.json').read_text())
finally:
    for process in [client, display]:
        if process and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
