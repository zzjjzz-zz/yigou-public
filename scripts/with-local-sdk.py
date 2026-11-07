"""Run a development command with the private WebKit SDK, without host installation."""
import os
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parents[1]
sdk = root / 'builds/sysroot/usr/lib/x86_64-linux-gnu'
if not sys.argv[1:]:
    raise SystemExit('Usage: with-local-sdk.py <command> [args...]')
command = sys.argv[1:]
if sdk.is_dir() and not pathlib.Path('/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/WebKitWebProcess').exists():
    overlay = root / 'builds/dev-runtime-libs'
    overlay.mkdir(exist_ok=True)
    system = pathlib.Path('/usr/lib/x86_64-linux-gnu')
    merged = {p.name: pathlib.Path('/mnt') / p.name for p in system.iterdir()}
    merged.update({p.name: p for p in sdk.iterdir()})
    for name, source in merged.items():
        target = overlay / name
        if target.is_symlink() and target.readlink() != source:
            target.unlink()
        if not target.is_symlink():
            target.symlink_to(source)
    command = ['bwrap', '--die-with-parent', '--ro-bind', '/', '/', '--bind', str(root), str(root), '--ro-bind', str(system), '/mnt', '--ro-bind', str(overlay), str(system), '--dev-bind', '/dev', '/dev', '--proc', '/proc', '--', *command]
os.execvpe(command[0], command, os.environ)
