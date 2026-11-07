"""Verify the actual packaged sidecar against local scripted SSH peers."""
import json
import base64
import pathlib
import subprocess
import sys
import tempfile
import time

root = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(root / 'worker/tests'))
from conftest import ssh_peer  # noqa: E402

executable = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else root / 'builds/worker-dist/yigou-worker'
with tempfile.TemporaryDirectory(prefix='yigou-frozen-') as directory:
    fixture = ssh_peer.__wrapped__(pathlib.Path(directory))
    peer = next(fixture)
    worker = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    request_id = 0

    def call(op, expected_ok=True, **args):
        global request_id
        request_id += 1
        worker.stdin.write(json.dumps({'id': request_id, 'op': op, **args}) + '\n')
        worker.stdin.flush()
        result = json.loads(worker.stdout.readline())
        assert result['id'] == request_id, result
        assert result['ok'] == expected_ok, result
        if not expected_ok:
            return result
        return result['result']

    def read_until(session, expected):
        output = bytearray()
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            result = call('terminal_read', session=session)
            output.extend(base64.b64decode(result['data']))
            if expected in output:
                return bytes(output)
            assert not result['closed'], output
            time.sleep(.02)
        raise AssertionError((expected, output))

    def write(session, data):
        assert call('terminal_write', session=session, data=base64.b64encode(data).decode())['sent_bytes'] == len(data)

    try:
        assert call('ping')['protocol'] == 1
        for vendor in ['huawei', 'ruijie', 'h3c']:
            peer.vendor = vendor
            peer.device['vendor'] = vendor
            session = call('connect', device=peer.device, credential=peer.credential, known_hosts=peer.known_hosts)['session']
            assert call('query', session=session, query='ports')['output']
            assert 'fixture-sensitive' not in call('query', session=session, query='config')['output']
            assert 'fixture-sensitive' in call('backup', session=session)['content']
            assert call('discover', session=session)['neighbors'][0]['remote_name'] == 'PEER'
            applied=call('configure',session=session,commands=['interface GE0/0/1',f'description PACKAGED-{vendor}'],save=True,
                         checks=[{'query':'config','contains':[f'description PACKAGED-{vendor}'],'not_contains':[]}])
            assert applied['status']=='succeeded',applied
            assert applied['save_status']=='verified'
            assert f'description PACKAGED-{vendor}' in call('query',session=session,query='startup')['output']
            assert 'fixture-sensitive' in applied.pop('after_content')
            assert 'fixture-sensitive' not in json.dumps(applied)
            ports_command='show interfaces status' if vendor=='ruijie' else 'display interface brief'
            original=peer.outputs[ports_command]
            peer.outputs[ports_command]='% Unrecognized command found'
            failed=call('query',expected_ok=False,session=session,query='ports')
            assert failed['error']['code']=='DEVICE_COMMAND_FAILED'
            peer.outputs[ports_command]=original
            call('close', session=session)
            session = call('terminal_open', device=peer.device, credential=peer.credential, known_hosts=peer.known_hosts)['session']
            read_until(session, b'LAB#' if vendor == 'ruijie' else b'<LAB>')
            write(session, b'configure terminal\r' if vendor == 'ruijie' else b'system-view\r')
            read_until(session, b'LAB(config)#' if vendor == 'ruijie' else b'[LAB]')
            write(session, 'interface GE0/0/1\rdescription 测试上联\r'.encode())
            read_until(session, 'description 测试上联'.encode())
            write(session, b'end\rwrite memory\r' if vendor == 'ruijie' else b'return\rsave\r')
            read_until(session, b'[Y/N]:')
            write(session, b'y\r')
            read_until(session, b'Save complete')
            write(session, b'slow-demo\r')
            read_until(session, b'Running until interrupted')
            write(session, b'\x03')
            read_until(session, b'^C')
            call('terminal_resize', session=session, cols=100, rows=35)
            assert call('query', expected_ok=False, session=session, query='ports')['error']['code'] == 'SESSION_SCOPE'
            call('close', session=session)
        worker.stdin.close()
        assert worker.wait(timeout=10) == 0
        print('PASS: packaged sidecar SSH + query + backup + LLDP + automatic configure/save/read-back + interactive config/save + UTF-8 paste + Ctrl+C + PTY resize + session isolation; 3 scripted vendors')
    finally:
        if worker.poll() is None:
            worker.terminate()
            worker.wait(timeout=10)
        try:
            next(fixture)
        except StopIteration:
            pass
