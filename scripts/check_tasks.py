#!/usr/bin/env python3
"""Check real gen/fix scheduling with a local model and real Cargo (macOS/Linux)."""
import argparse
import fcntl
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time


def sources(root):
    return {str(p.relative_to(root)): p.read_bytes()
            for folder in ['src', 'tests'] for p in (root / folder).rglob('*.rs')}


def cargo_wrapper():
    """Observe compilation without replacing Cargo or its diagnostics."""
    args = sys.argv[1:]
    env = os.environ.copy()
    command = [env['PALM_REAL_CARGO'], *args]
    monitor = Path(env['PALM_TASKS_MONITOR'])
    if not args or args[0] not in ['build', 'test'] or env.get('PALM_MONITOR_CHILD'):
        os.execv(command[0], command)
    env['PALM_MONITOR_CHILD'] = '1'
    before = sources(Path.cwd())
    with (monitor / 'compiler.json').open('r+') as file:
        fcntl.flock(file, fcntl.LOCK_EX)
        state = json.load(file)
        state['active'] += 1
        state['total'] += 1
        state['peak'] = max(state['peak'], state['active'])
        first = state['total'] == 1
        file.seek(0)
        json.dump(state, file)
        file.truncate()
    if first and env.get('PALM_HOLD_FIRST_BUILD'):
        (monitor / 'build-started').touch()
        deadline = time.monotonic() + 20
        while not (monitor / 'release-build').exists():
            if time.monotonic() > deadline:
                raise RuntimeError('backpressure fixture did not release compilation')
            time.sleep(0.02)
    result = subprocess.run(command, env=env)
    with (monitor / 'compiler.json').open('r+') as file:
        fcntl.flock(file, fcntl.LOCK_EX)
        state = json.load(file)
        state['active'] -= 1
        if sources(Path.cwd()) != before:
            state['source_changes'] += 1
        file.seek(0)
        json.dump(state, file)
        file.truncate()
    sys.exit(result.returncode)


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=repo / 'target/debug')
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='palm-tasks-')).resolve()
    root = work / 'crate'
    (root / 'src').mkdir(parents=True)
    for name in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']:
        shutil.copyfile(repo / 'examples/minimal' / name, root / name)
    count = 9  # More than 2N even for the default N=4.
    original = ''.join(f'pub fn f{i}(x: i32) -> i32 {{ x + 1 }}\n' for i in range(count))
    (root / 'src/lib.rs').write_text(original)
    (root / 'src/unrelated.rs').write_text('// unrelated source file\n')
    unrelated = root / 'src/unrelated.rs.bak'
    unrelated.write_bytes(b'previous recovery material\n')
    wrapper = work / 'bin'
    wrapper.mkdir()
    shutil.copyfile(__file__, wrapper / 'cargo')
    (wrapper / 'cargo').chmod(0o755)
    env = os.environ.copy()
    env.pop('CARGO_TARGET_DIR', None)
    for name in ['PALM_CONFIG', 'PALM_API_BASE', 'PALM_API_KEY', 'PALM_MODEL']:
        env.pop(name, None)
    env['PALM_REAL_CARGO'] = shutil.which('cargo')
    env['PATH'] = os.pathsep.join([str(wrapper), str(args.bin_dir.resolve()), env['PATH']])
    for name in ['NO_PROXY', 'no_proxy']:
        env[name] = ','.join(filter(None, [env.get(name, ''), '127.0.0.1', 'localhost']))
    print(f'Validation artifacts: {work}', flush=True)
    lock = threading.Lock()
    ready = threading.Event()
    state = {}
    summaries = {}

    class Model(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            try:
                request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                system, user = request['messages'][0]['content'], request['messages'][-1]['content']
                with lock:
                    state['active'] += 1
                    state['total'] += 1
                    serial = state['total']
                    state['peak'] = max(state['peak'], state['active'])
                    if state['active'] >= state['limit']:
                        ready.set()
                # Hold the first batch to measure overlap, including slow repair builds.
                assert ready.wait(20), 'model workers did not reach the requested concurrency'
                time.sleep(0.05)
                if 'The function to be tested is presented as follows:' in user:
                    focal = user.split('The function to be tested is presented as follows:', 1)[1]
                    function = re.search(r'\bfn (f\d+)\(', focal).group(1)
                    if 'infer the test input ranges' in system:
                        answer = 'x = 1'
                    elif 'generate accurate test oracles' in system:
                        answer = 'assert_eq!(actual, 2);'
                    else:
                        argument = '1' if state['integration'] else '"bad"'
                        body = ('let actual = ' + function + '(1);' if 'Omit test oracles' in system
                                else f'assert_eq!({function}({argument}), 2);')
                        imports = ('use palm_fixture::*;\nuse std::cmp::max;\nuse absent_crate::Missing;\n'
                                   if state['integration'] else '')
                        answer = imports + '#[test]\nfn generated() {\n    ' + body + '\n}\n'
                else:
                    file = re.search(r'You can only modify lines \d+ to \d+ in file (.+?)\. For your answer', user).group(1)
                    match = re.search(r'^\[(\d+)\](.*assert_eq!\(f\d+\("bad"\), 2\);.*)$', user, re.M)
                    assert match, 'missing numbered repair statement'
                    number, old = match.groups()
                    new = old.replace('"bad"', '1')
                    answer = f'ChangeLog:1@{file}\nFixDescription: Use an integer.\nOriginalCode@{number}-{number}:\n[{number}]{old}\nFixedCode@{number}-{number}:\n[{number}]{new}\n'
                response = {'id': 'fixture', 'object': 'chat.completion', 'created': 0, 'model': 'fixture',
                            'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': answer}, 'finish_reason': 'stop'}]}
                # Exercise a real worker panic in the existing response parser.
                if not (state['panic'] and serial == 1):
                    response['usage'] = {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}
                body = json.dumps(response).encode()
                # Count requests awaiting a response, excluding HTTP-handler teardown.
                with lock:
                    state['active'] -= 1
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            except Exception as error:
                with lock:
                    state['errors'].append(str(error))
                self.send_error(400, str(error))

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Model)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env.update(PALM_API_BASE=f'http://127.0.0.1:{server.server_port}/v1', PALM_API_KEY='fixture', PALM_MODEL='fixture')

    def run(label, command, limit=1, integration=False, panic=False, fail=False, backpressure=False):
        monitor = work / label
        monitor.mkdir()
        (monitor / 'compiler.json').write_text(json.dumps(dict(active=0, peak=0, total=0, source_changes=0)))
        env['PALM_TASKS_MONITOR'] = str(monitor)
        env.pop('PALM_HOLD_FIRST_BUILD', None)
        with lock:
            state.clear()
            state.update(active=0, peak=0, total=0, errors=[], limit=limit, integration=integration, panic=panic)
        ready.clear()
        watcher = None
        if backpressure:
            env['PALM_HOLD_FIRST_BUILD'] = '1'

            def check_queue():
                try:
                    deadline = time.monotonic() + 20
                    while not (monitor / 'build-started').exists() or state['total'] < 3:
                        assert time.monotonic() < deadline, 'queue did not fill'
                        time.sleep(0.02)
                    time.sleep(0.3)
                    # One validating, one buffered, one blocked at send; no fourth job.
                    assert state['total'] == 3, state.copy()
                except Exception as error:
                    state['errors'].append(str(error))
                finally:
                    (monitor / 'release-build').touch()
            watcher = threading.Thread(target=check_queue)
            watcher.start()
        print(label, flush=True)
        log = monitor / 'command.log'
        with log.open('w') as output:
            process = subprocess.Popen(command, cwd=root, env=env, stdout=output, stderr=subprocess.STDOUT,
                                       start_new_session=True)
            try:
                status = process.wait(timeout=240)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                raise AssertionError(f'{label}: timeout; see {log}')
        if watcher:
            watcher.join()
        compiler = json.loads((monitor / 'compiler.json').read_text())
        summaries[label] = dict(model=state.copy(), compiler=compiler, exit=status)
        (work / 'summary.json').write_text(json.dumps(summaries, indent=2) + '\n')
        assert (status != 0) == fail, f'{label}: exit {status}; see {log}\n{log.read_text()[-5000:]}'
        assert not state['errors'], state
        assert state['peak'] <= limit, state
        assert compiler['peak'] <= 1 and compiler['active'] == 0 and compiler['source_changes'] == 0, compiler
        assert (root / 'src/lib.rs').read_text() == original, label
        assert (root / 'src/unrelated.rs').read_text() == '// unrelated source file\n', label
        assert unrelated.read_bytes() == b'previous recovery material\n', label
        return state.copy()

    def reset(seed=None):
        shutil.rmtree(root / 'utgen', ignore_errors=True)
        shutil.rmtree(root / 'tests', ignore_errors=True)
        if seed:
            shutil.copytree(seed, root / 'utgen/generation/pre_fix')

    def command(mode, n, *extra):
        return ['utgen', mode, '-p', str(root), *([] if n == 4 else ['--tasks', str(n)]), *extra]

    def results(directory, compiled):
        data = [json.loads(p.read_text()) for p in (root / 'utgen' / directory).glob('*.json')]
        assert len(data) == count, (directory, len(data))
        assert all(item['tests_compiled'] == compiled for item in data), data
        if compiled:
            assert all(item['tests_run'] == 1 and item['tests_passed'] == 1
                       and item['lines_covered'] > 0 for item in data), data

    try:
        run('analyze', ['utgen', 'analyze', '-p', str(root)])
        seed = work / 'broken-candidates'
        for n in [1, 2, 4]:
            reset()
            measured = run(f'gen-{n}', command('gen', n), limit=n, backpressure=n == 1)
            assert measured['total'] == count and measured['peak'] == n, measured
            results('result', 0)
            assert not (root / 'src/lib.rs.bak').exists()
            if n == 1:
                shutil.copytree(root / 'utgen/generation/pre_fix', seed)
        for n in [1, 2, 4]:
            reset(seed)
            measured = run(f'fix-{n}', command('fix', n), limit=n)
            assert measured['total'] == count and measured['peak'] == n, measured
            results('fixed_result', 1)
            assert not (root / 'src/lib.rs.bak').exists()
        for oracle in [False, True]:
            reset()
            preserved = {}
            if oracle:
                (root / 'tests').mkdir()
                for name in ['use_check.rs', 'test_check.rs']:
                    path = root / 'tests' / name
                    path.write_text('// existing compiler input\n')
                    preserved[path] = path.read_bytes()
            measured = run(f'integration-{oracle}', command('gen', 2, '--integration', *(['--oracle'] if oracle else [])),
                           limit=2, integration=True)
            assert measured['total'] == count * (3 if oracle else 1), measured
            results('result', 1)
            for file in (root / 'utgen/generation/pre_fix').glob('*.json'):
                text = file.read_text()
                assert 'absent_crate' not in text and 'std::cmp::max' in text, file
            for name in ['use_check.rs', 'test_check.rs']:
                path = root / 'tests' / name
                if path in preserved:
                    assert path.read_bytes() == preserved[path], path
                else:
                    assert not path.exists(), path
        for mode in ['gen', 'fix']:
            reset(seed if mode == 'fix' else None)
            measured = run(f'{mode}-panic', command(mode, 1), panic=True, fail=True)
            assert measured['total'] == count, measured  # Remaining workers still finish.
            assert not (root / 'utgen' / ('result' if mode == 'gen' else 'fixed_result')).exists()
            if mode == 'fix':
                backup = root / 'src/lib.rs.bak'
                assert backup.read_text() == original
                # A later invocation must not overwrite retained recovery material.
                backup.write_bytes(b'old recovery material\n')
                measured = run('fix-existing-backup', command('fix', 1), fail=True)
                assert measured['total'] == 0
                assert backup.read_bytes() == b'old recovery material\n'
                backup.unlink()
        # A consumer failure must still drain the queue and let every sender finish.
        reset()
        backup = root / 'src/lib.rs.bak'
        backup.write_bytes(b'old recovery material\n')
        measured = run('gen-validation-failure', command('gen', 1), fail=True)
        assert measured['total'] == count
        assert backup.read_bytes() == b'old recovery material\n'
        assert not (root / 'utgen/result').exists()
        backup.unlink()
        # Fail after source insertion while the repair compilation lock is held.
        reset(seed)
        diagnostics = root / 'error_output.json'
        diagnostics.unlink(missing_ok=True)
        diagnostics.mkdir()
        measured = run('fix-io-failure', command('fix', 2), limit=2, fail=True)
        assert measured['total'] == 0
        assert (root / 'src/lib.rs.bak').read_text() == original
        assert not (root / 'utgen/fixed_result').exists()
        print('Task limits, queue backpressure, serial compilation, imports and failure cleanup passed.', flush=True)
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    if Path(sys.argv[0]).name == 'cargo':
        cargo_wrapper()
    else:
        main()
