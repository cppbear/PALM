#!/usr/bin/env python3
"""Exercise PALM on an isolated, deterministic crate; never use a real model."""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import threading

REPO = Path(__file__).resolve().parents[1]


def snapshot(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted((root / 'src').rglob('*.rs'))} | {
                'Cargo.toml': hashlib.sha256((root / 'Cargo.toml').read_bytes()).hexdigest()}


def source_fragment(root, loc):
    file, start, col, end, last = loc.rsplit(':', 4)
    start, col, end, last = map(int, (start, col, end, last))
    lines = (root / file).read_text().splitlines()
    return '\n'.join(line[(col - 1 if n == start else 0):(last - 1 if n == end else len(line))]
                     for n, line in enumerate(lines, 1) if start <= n <= end)


def check_analysis(root):
    names = json.loads((root / 'brinfo/name_map.json').read_text())
    expected = {'palm_fixture::classify', 'palm_fixture::Gauge::label',
                'palm_fixture::nested::double', 'palm_fixture::support::positive',
                'palm_fixture::support::threshold'}
    assert set(names) == expected, names
    infos = {item['full_name']: item for item in json.loads((root / 'focxt/impl_informations.json').read_text())}
    assert set(infos) == expected, infos
    for name, encoded in names.items():
        data = json.loads((root / f'brinfo/brdata/{encoded}.json').read_text())
        assert data['name'] == name
        # brinfo removes leading indentation when exporting focal code.
        assert [line.lstrip() for line in source_fragment(root, data['loc']).splitlines()] == [line.lstrip() for line in data['code']], name
        assert (root / f"focxt/{infos[name]['encoded_name']}.rs").is_file(), name
    context = (root / f"focxt/{infos['palm_fixture::Gauge::label']['encoded_name']}.rs").read_text()
    for declaration in ['struct Gauge', 'fn label', 'fn classify', 'fn positive', 'fn threshold']:
        assert declaration in context, (declaration, context)
    branch = json.loads((root / f"brinfo/brdata/{names['palm_fixture::classify']}.json").read_text())
    assert {c['conds'][0]['value'] for c in branch['cond_chains'] if c['min_set']} == {'true', 'false'}
    return names


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=REPO / 'target/debug')
    parser.add_argument('--work-dir', type=Path, help='New directory for copies, logs and results')
    parser.add_argument('--analysis-only', action='store_true')
    args = parser.parse_args()
    work = args.work_dir.resolve() if args.work_dir else Path(tempfile.mkdtemp(prefix='palm-minimal-')).resolve()
    if args.work_dir:
        work.mkdir(parents=True, exist_ok=False)
    print(f'Validation artifacts: {work}', flush=True)
    env = os.environ.copy()
    env.pop('CARGO_TARGET_DIR', None)
    for name in ['PALM_CONFIG', 'PALM_API_BASE', 'PALM_API_KEY', 'PALM_MODEL']:
        env.pop(name, None)
    env['PATH'] = str(args.bin_dir.resolve()) + os.pathsep + env['PATH']
    # Only child commands receive this override; preserve proxies for registry downloads.
    for name in ['NO_PROXY', 'no_proxy']:
        env[name] = ','.join(filter(None, [env.get(name, ''), '127.0.0.1', 'localhost']))
    logs = work / 'logs'
    logs.mkdir()
    original = work / 'original'
    target = work / 'generated'
    for copy in [original, target]:
        shutil.copytree(REPO / 'examples/minimal', copy,
                        ignore=shutil.ignore_patterns('target', 'brinfo', 'focxt', 'utgen', 'tests.bak'))

    def run(label, command, cwd=target, expected=0):
        print(label, flush=True)
        result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True, timeout=240)
        log = logs / (label + '.log')
        log.write_text(result.stdout + result.stderr)
        assert (result.returncode == 0) == (expected == 0), f'{label}: exit {result.returncode}; see {log}\n{(result.stdout + result.stderr)[-12000:]}'
        return result

    run('original-tests', ['cargo', 'test', '--locked', '--tests'], original)
    run('preprocess', ['utgen', 'pre-process', '-p', str(target)])
    assert (target / 'tests.bak/original.rs').exists() and not (target / 'tests').exists()
    for file in ['src/lib.rs', 'src/support.rs']:
        old, new = (original / file).read_bytes(), (target / file).read_bytes()
        assert len(old) == len(new)
        assert [i for i, b in enumerate(old) if b == 10] == [i for i, b in enumerate(new) if b == 10]
    prepared = snapshot(target)
    run('preprocess-again', ['utgen', 'pre-process', '-p', str(target)])
    assert snapshot(target) == prepared
    run('prepared-check', ['cargo', 'check', '--locked'])
    run('analyze', ['utgen', 'analyze', '-p', str(target)])
    names = check_analysis(target)
    assert snapshot(target) == prepared
    run('reject-stale-analysis', ['utgen', 'analyze', '-p', str(target)], expected=1)
    if args.analysis_only:
        print('Analysis contract passed for all five functions.', flush=True)
        return

    # Invalid analysis must fail before requests or manifest/source mutations.
    env.update(PALM_API_BASE='http://127.0.0.1:9/v1', PALM_API_KEY='unused', PALM_MODEL='unused')
    context = target / f"focxt/{names['palm_fixture::classify']}.rs"
    saved = context.read_bytes()
    context.unlink()
    try:
        run('missing-context', ['utgen', 'gen', '-p', str(target)], expected=1)
        assert snapshot(target) == prepared
        assert not (target / 'utgen').exists()
    finally:
        context.write_bytes(saved)
    artifact = target / f"brinfo/brdata/{names['palm_fixture::classify']}.json"
    saved = artifact.read_bytes()
    data = json.loads(saved)
    data['loc'] = 'src/lib.rs:not-a-number:1:2:1'
    artifact.write_text(json.dumps(data))
    try:
        failure = run('malformed-location', ['utgen', 'gen', '-p', str(target)], expected=1)
        assert 'panicked' not in failure.stderr
        assert snapshot(target) == prepared
    finally:
        artifact.write_bytes(saved)

    version = run('coverage-version', ['cargo', 'llvm-cov', '--version'])
    assert version.stdout.strip() == 'cargo-llvm-cov 0.6.16', version.stdout
    run('original-coverage', ['cargo', 'llvm-cov', '--tests', '--branch', '--json', '--output-path', 'original-coverage.json'], original)
    original_coverage = json.loads((original / 'original-coverage.json').read_text())
    original_file = next(file for data in original_coverage['data'] for file in data['files']
                         if Path(file['filename']) == original / 'src/lib.rs')
    condition_line = (original / 'src/lib.rs').read_text().splitlines().index('    if support::positive(value) {') + 1
    # Library and unit-test binaries may emit separate records for one location.
    records = [branch for branch in original_file['branches'] if branch[0] == condition_line]
    assert any(branch[4] > 0 for branch in records) and any(branch[5] > 0 for branch in records)
    state = {'generation': 0, 'repair': 0, 'errors': []}

    class Model(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            try:
                request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                assert self.path == '/v1/chat/completions'
                assert self.headers['Authorization'] == 'Bearer local-fixture-key'
                user = request['messages'][-1]['content']
                if 'The function to be tested is presented as follows:' in user:
                    focal = user.split('The function to be tested is presented as follows:', 1)[1]
                    function = re.search(r'\bfn (\w+)\(', focal).group(1)
                    positive = '// constraint: support::positive(value) is true' in user
                    body = {
                        'classify': f'assert_eq!(classify({2 if positive else -1}), {1 if positive else 0});',
                        'label': 'assert_eq!(Gauge { value: 2 }.label(), 1);',
                        'positive': 'assert!(positive(2));',
                        'threshold': 'assert!(threshold(2));',
                        'double': 'assert_eq!(double("bad"), 4);',
                    }[function]
                    answer = '#[test]\nfn generated() {\n    ' + body + '\n}\n'
                    state['generation'] += 1
                else:
                    file = re.search(r'You can only modify lines \d+ to \d+ in file (.+?)\. For your answer', user).group(1)
                    match = re.search(r'^\[(\d+)\](.*assert_eq!\(double\("bad"\), 4\);.*)$', user, re.M)
                    assert match, 'repair prompt lacks numbered failing statement'
                    number, old = match.groups()
                    new = old.replace('double("bad")', 'double(2)')
                    answer = f'ChangeLog:1@{file}\nFixDescription: Use an integer argument.\nOriginalCode@{number}-{number}:\n[{number}]{old}\nFixedCode@{number}-{number}:\n[{number}]{new}\n'
                    state['repair'] += 1
                response = {'id': 'fixture', 'object': 'chat.completion', 'created': 0, 'model': 'fixture',
                            'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': answer}, 'finish_reason': 'stop'}],
                            'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}}
                code = 200
            except Exception as error:
                state['errors'].append(str(error))
                response = {'error': {'message': str(error), 'type': 'fixture_error'}}
                code = 400
            body = json.dumps(response).encode()
            self.send_response(code)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Model)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    env.update(PALM_API_BASE=f'http://127.0.0.1:{server.server_port}/v1', PALM_API_KEY='local-fixture-key', PALM_MODEL='fixture')
    try:
        run('generate', ['utgen', 'gen', '-p', str(target), '--requirement', '--context'])
        assert snapshot(target) == prepared
        run('repair', ['utgen', 'fix', '-p', str(target)])
        assert snapshot(target) == prepared
        assert not state['errors'], state
        assert state['generation'] == 6 and state['repair'] >= 1, state
        for directory in ['result', 'fixed_result']:
            results = [json.loads(p.read_text()) for p in (target / 'utgen' / directory).glob('*.json')]
            assert len(results) == 5, (directory, results)
            if directory == 'fixed_result':
                assert all(r['tests_compiled'] == r['tests'] and r['tests_passed'] == r['tests_run'] and r['tests_run'] > 0 for r in results), results
            classify = next(r for r in results if r['function_name'] == 'palm_fixture::classify')
            assert classify['branches_covered'] == 2 and classify['branches'] == 2, classify
        double = json.loads((target / f"utgen/result/{names['palm_fixture::nested::double']}.json").read_text())
        assert double['tests_compiled'] == 0 and double['tests'] == 1, double
        assert not list((target / 'src').rglob('*.bak'))
        (work / 'summary.json').write_text(json.dumps(state, indent=2) + '\n')
        print('Minimal analysis/generation/repair/coverage checks passed.', flush=True)
    finally:
        server.shutdown()
        server.server_close()
        (work / 'model-requests.json').write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
