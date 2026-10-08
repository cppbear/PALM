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
                    is_input = 'infer the test input ranges' in system
                    is_oracle = 'generate accurate test oracles' in system
                    if is_input or state['integration']:
                        assert 'use super::*;' not in system, 'unit scope leaked into another prompt'
                    else:
                        assert 'child test module inside the crate under test' in system
                        assert 'use super::*;' in system and 'crate::' in system
                        assert 'not an external dependency' in system
                    if state['integration'] and not is_input:
                        assert 'publicly accessible' in system
                        if not is_oracle:
                            assert 'separate folder' in system
                    if is_oracle:
                        assert 'do not output complete test functions' in system.lower()
                    if 'Omit test oracles' in system:
                        assert 'Omit test oracles and assertions' in system
                    if is_input:
                        answer = 'x = 1'
                    elif is_oracle:
                        answer = 'assert_eq!(actual, 2);'
                    else:
                        argument = '1' if state['integration'] or state['invalid_answers'] else '"bad"'
                        body = ('let actual = ' + function + '(1);' if 'Omit test oracles' in system
                                else f'assert_eq!({function}({argument}), 2);')
                        imports = ('use palm_fixture::*;\nuse std::cmp::max;\nuse absent_crate::Missing;\n'
                                   if state['integration'] else '')
                        answer = imports + '#[test]\nfn generated() {\n    ' + body + '\n}\n'
                        if state['invalid_answers']:
                            with lock:
                                if len(state['code_answers']) < state['invalid_answers']:
                                    answer = 'fn helper() {}'
                                answer = '```rust\n' + answer + '```'
                                state['code_answers'].append(answer)
                else:
                    file = re.search(r'You can only modify lines \d+ to \d+ in file (.+?)\. For your answer', user).group(1)
                    match = re.search(r'^\[(\d+)\](.*assert_eq!\(f\d+\("bad"\), 2\);.*)$', user, re.M)
                    assert match, 'missing numbered repair statement'
                    number, old = match.groups()
                    new = old.replace('"bad"', '1')
                    answer = f'ChangeLog:1@{file}\nFixDescription: Use an integer.\nOriginalCode@{number}-{number}:\n[{number}]{old}\nFixedCode@{number}-{number}:\n[{number}]{new}\n'
                response = {'id': 'fixture', 'object': 'chat.completion', 'created': 0, 'model': 'fixture',
                            'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': answer}, 'finish_reason': 'stop'}]}
                # Valid answers must remain usable when a service omits usage.
                if not (state['missing_usage'] and serial == 1):
                    response['usage'] = {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}
                status = 200
                if ((state['fault'] == 'request' and serial == 1)
                        or (state['fault'] == 'oracle' and 'generate accurate test oracles' in system
                            and not state['fault_sent'])):
                    response = {'error': {'message': 'fixture unauthorized'}}
                    status = 401
                    state['fault_sent'] = True
                body = json.dumps(response).encode()
                # Count requests awaiting a response, excluding HTTP-handler teardown.
                with lock:
                    state['active'] -= 1
                self.send_response(status)
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

    def run(label, command, limit=1, integration=False, missing_usage=False, fault=None, fail=False, backpressure=False,
            invalid_answers=0):
        monitor = work / label
        monitor.mkdir()
        (monitor / 'compiler.json').write_text(json.dumps(dict(active=0, peak=0, total=0, source_changes=0)))
        env['PALM_TASKS_MONITOR'] = str(monitor)
        env.pop('PALM_HOLD_FIRST_BUILD', None)
        with lock:
            state.clear()
            state.update(active=0, peak=0, total=0, errors=[], limit=limit, integration=integration, missing_usage=missing_usage, fault=fault, fault_sent=False)
            state.update(invalid_answers=invalid_answers, code_answers=[])
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
        if command[1] in ['gen', 'fix'] and state['total']:
            report = json.loads((root / f'utgen/generation/{command[1]}-requests.json').read_text())
            assert report['attempts'] == state['total'], report
            assert report['responses_without_usage'] == int(missing_usage), report
            assert report['usage_complete'] == (not missing_usage and not state['fault_sent']), report
            assert report['model'] == 'fixture', report
            assert report['invocation']['command'] == command[1], report
            assert report['invocation']['candidate_status'] == ('failed' if fail else 'completed'), report
        return state.copy()

    def reset(seed=None):
        shutil.rmtree(root / 'utgen', ignore_errors=True)
        shutil.rmtree(root / 'tests', ignore_errors=True)
        if seed:
            shutil.copytree(seed, root / 'utgen/generation/pre_fix')

    def command(mode, n, *extra):
        return ['utgen', mode, '-p', str(root), *([] if n == 4 else ['--tasks', str(n)]), *extra]

    def results(directory, compiled, names=None):
        data = [json.loads(p.read_text()) for p in (root / 'utgen' / directory).glob('*.json')]
        assert len(data) == (len(names) if names is not None else count), (directory, len(data))
        if names is not None:
            assert {item['function_name'] for item in data} == set(names), data
        assert all(item['tests_compiled'] == compiled for item in data), data
        if compiled:
            assert all(item['tests_run'] == 1 and item['tests_passed'] == 1
                       and item['lines_covered'] > 0 for item in data), data

    try:
        run('analyze', ['utgen', 'analyze', '-p', str(root)])
        # Real reply parsing, format retries and saving on a single selected function.
        nmap = json.loads((root / 'brinfo/name_map.json').read_text())
        one_name = sorted(nmap)[0]
        one_file = work / 'one-function.txt'
        one_file.write_text(one_name + '\n')
        for oracle in [False, True]:
            stage = 'prefix' if oracle else 'test'
            for case in ['recover', 'exhaust', 'budget']:
                reset()
                cap = 2 if oracle else 1  # Input inference consumes the first request in oracle mode.
                extra = ['--functions-file', str(one_file)]
                if oracle:
                    extra.append('--oracle')
                if case == 'budget':
                    extra += ['--max-requests', str(cap)]
                label = f'{stage}-without-tests-{case}'
                measured = run(label, command('gen', 1, *extra), invalid_answers=1 if case == 'recover' else 3,
                               fail=case != 'recover')
                attempts = {'recover': 2, 'exhaust': 3, 'budget': 1}[case]
                expected_requests = attempts + int(oracle) + int(oracle and case == 'recover')
                assert measured['total'] == expected_requests, measured
                assert len(measured['code_answers']) == attempts
                chain_dir = root / 'utgen/generation/answer' / nmap[one_name] / '000'
                files = sorted(chain_dir.glob(f'{stage}-attempt-*.txt'))
                assert len(files) == attempts, files
                for index, file in enumerate(files):
                    assert file.read_text() == measured['code_answers'][index], file
                accepted = chain_dir / ('prefix.rs' if oracle else 'code.rs')
                candidate = root / 'utgen/generation/pre_fix' / (nmap[one_name] + '.json')
                assert accepted.exists() == candidate.exists() == (case == 'recover')
                report = json.loads((root / 'utgen/generation/gen-requests.json').read_text())
                assert report['reported_prompt_tokens'] == expected_requests, report
                assert report['reported_completion_tokens'] == expected_requests, report
                assert report['budget_exhausted'] == (case == 'budget'), report
                log = (work / label / 'command.log').read_text()
                assert 'No #[test] function found' in log, log[-2000:]
                assert not (root / 'src/lib.rs.bak').exists()
                if case == 'recover':
                    assert '```' not in accepted.read_text() and 'fn helper()' not in accepted.read_text()
                    if oracle:
                        assert 'assert' not in accepted.read_text(), 'a prefix need not contain an oracle'
                    results('result', 1, [one_name])
                else:
                    assert not (root / 'utgen/result').exists()
        seed = work / 'broken-candidates'
        two_task_names = sorted(nmap)[:5]  # More than 2N for N=2.
        two_task_file = work / 'two-task-functions.txt'
        two_task_file.write_text('\n'.join(two_task_names) + '\n')
        task_cases = [(1, None), (2, two_task_names), (4, None)]
        for n, names in task_cases:
            reset()
            extra = ['--functions-file', str(two_task_file)] if names else []
            measured = run(f'gen-{n}', command('gen', n, *extra), limit=n, missing_usage=n == 1, backpressure=n == 1)
            assert measured['total'] == (len(names) if names else count) and measured['peak'] == n, measured
            results('result', 0, names)
            assert not (root / 'src/lib.rs.bak').exists()
            if n == 1:
                shutil.copytree(root / 'utgen/generation/pre_fix', seed)
                measured = run('gen-cached', command('gen', 1))
                assert measured['total'] == 0
                report = json.loads((root / 'utgen/generation/gen-requests.json').read_text())
                assert report['attempts'] == 0 and report['usage_complete'], report
        for n, names in task_cases:
            reset(seed)
            extra = ['--functions-file', str(two_task_file)] if names else []
            measured = run(f'fix-{n}', command('fix', n, *extra), limit=n, missing_usage=n == 1)
            assert measured['total'] == (len(names) if names else count) and measured['peak'] == n, measured
            results('fixed_result', 1, names)
            assert not (root / 'src/lib.rs.bak').exists()

        # Existing results for other functions must not enter this invocation.
        nmap = json.loads((root / 'brinfo/name_map.json').read_text())
        selected = sorted(nmap)[:2]
        functions_file = work / 'functions.txt'
        functions_file.write_text('\n' + '\n'.join(selected + [selected[0]]) + '\n')
        function_args = ['--functions-file', str(functions_file)]
        selection_args = [*function_args, '--max-requests', '2']
        reset(seed)
        pre_dir = root / 'utgen/generation/pre_fix'
        for name in selected:
            (pre_dir / (nmap[name] + '.json')).unlink()
        untouched = {p: p.read_bytes() for p in pre_dir.glob('*.json')}
        measured = run('gen-selected', command('gen', 2, *selection_args), limit=2)
        assert measured['total'] == 2 and measured['peak'] == 2, measured
        results('result', 0, selected)
        for path, content in untouched.items():
            assert path.read_bytes() == content, path
        report = json.loads((root / 'utgen/generation/gen-requests.json').read_text())
        assert report['invocation']['functions'] == selected, report
        assert report['max_requests'] == 2 and not report['budget_exhausted'], report

        missing = pre_dir / (nmap[selected[0]] + '.json')
        candidate = missing.read_bytes()
        missing.unlink()
        measured = run('fix-missing-selected', command('fix', 2, *selection_args), fail=True)
        assert measured['total'] == 0
        assert not (root / 'utgen/generation/llm_fix').exists()
        assert not (root / 'src/lib.rs.bak').exists()
        missing.write_bytes(candidate)

        fixed_dir = root / 'utgen/generation/llm_fix'
        fixed_dir.mkdir()
        other_name = sorted(nmap)[2]
        other = fixed_dir / (nmap[other_name] + '.json')
        shutil.copyfile(pre_dir / other.name, other)
        old_other = other.read_bytes()
        before_fix = {p: p.read_bytes() for p in pre_dir.glob('*.json')}
        measured = run('fix-selected', command('fix', 2, *selection_args), limit=2)
        assert measured['total'] == 2 and measured['peak'] == 2, measured
        results('fixed_result', 1, selected)
        assert len(list(fixed_dir.glob('*.json'))) == 3
        assert other.read_bytes() == old_other
        for path, content in before_fix.items():
            assert path.read_bytes() == content, path
        report = json.loads((root / 'utgen/generation/fix-requests.json').read_text())
        assert report['invocation']['functions'] == selected, report
        assert report['max_requests'] == 2 and not report['budget_exhausted'], report
        measured = run('fix-selected-cached', command('fix', 2, *selection_args))
        assert measured['total'] == 0
        report = json.loads((root / 'utgen/generation/fix-requests.json').read_text())
        assert report['attempts'] == 0 and report['invocation']['functions'] == selected, report
        assert other.read_bytes() == old_other

        reset(seed)
        for name in selected:
            (pre_dir / (nmap[name] + '.json')).unlink()
        measured = run('integration-selected', command('gen', 2, '--integration', *selection_args),
                       limit=2, integration=True)
        assert measured['total'] == 2
        results('result', 1, selected)

        # More workers than requests: every clone shares the same hard cap.
        for mode in ['gen', 'fix']:
            reset(seed if mode == 'fix' else None)
            measured = run(f'{mode}-budget', command(mode, 4, '--max-requests', '2'), limit=2, fail=True)
            assert measured['total'] == 2, measured
            report = json.loads((root / f'utgen/generation/{mode}-requests.json').read_text())
            assert report['attempts'] == 2 and report['budget_exhausted'], report
            assert not (root / 'utgen' / ('result' if mode == 'gen' else 'fixed_result')).exists()
            if mode == 'fix':
                backup = root / 'src/lib.rs.bak'
                assert backup.read_text() == original
                backup.unlink()

        for oracle in [False, True]:
            reset()
            preserved = {}
            if oracle:
                (root / 'tests').mkdir()
                for name in ['use_check.rs', 'test_check.rs']:
                    path = root / 'tests' / name
                    path.write_text('// existing compiler input\n')
                    preserved[path] = path.read_bytes()
            measured = run(f'integration-{oracle}', command('gen', 2, '--integration', *function_args, *(['--oracle'] if oracle else [])),
                           limit=2, integration=True)
            assert measured['total'] == len(selected) * (3 if oracle else 1), measured
            results('result', 1, selected)
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
            measured = run(f'{mode}-request-error', command(mode, 1, *function_args), fault='request', fail=True)
            assert measured['total'] == len(selected), measured  # The other selected function still finishes.
            assert not (root / 'utgen' / ('result' if mode == 'gen' else 'fixed_result')).exists()
            if mode == 'fix':
                backup = root / 'src/lib.rs.bak'
                assert backup.read_text() == original
                # A later invocation must not overwrite retained recovery material.
                backup.write_bytes(b'old recovery material\n')
                measured = run('fix-existing-backup', command('fix', 1, *function_args), fail=True)
                assert measured['total'] == 0
                assert backup.read_bytes() == b'old recovery material\n'
                backup.unlink()
        reset()
        measured = run('oracle-request-error', command('gen', 1, '--integration', '--oracle', *function_args),
                       integration=True, fault='oracle', fail=True)
        assert measured['total'] == len(selected) * 3 and measured['fault_sent']
        assert not (root / 'utgen/result').exists()
        reset()
        encoded = next(iter(json.loads((root / 'brinfo/name_map.json').read_text()).values()))
        blocker = root / 'utgen/generation/prompt' / encoded
        blocker.parent.mkdir(parents=True)
        blocker.write_text('not a directory')
        measured = run('gen-worker-panic', command('gen', 1), fail=True)
        assert measured['total'] == count - 1
        assert not (root / 'utgen/result').exists()
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
