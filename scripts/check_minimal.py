#!/usr/bin/env python3
"""Exercise PALM on an isolated, deterministic crate; never use a real model."""
import argparse
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import threading
import xml.etree.ElementTree as ET

REPO = Path(__file__).resolve().parents[1]


def snapshot(root):
    return {str(p.relative_to(root)): p.read_bytes()
            for directory in ['src', 'tests', 'tests.bak']
            for p in sorted((root / directory).rglob('*.rs'))} | {
                'Cargo.toml': (root / 'Cargo.toml').read_bytes()}


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

    def run(label, command, cwd=target, expected=0, timeout=240):
        print(label, flush=True)
        log = logs / (label + '.log')
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate()
            log.write_text(stdout + stderr)
            raise AssertionError(f'{label}: timeout; see {log}')
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
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
    baseline = snapshot(original)
    # A test side effect must occur once even though both formats are exported.
    counter = work / 'coverage-runs.txt'
    env['PALM_FIXTURE_RUN_LOG'] = str(counter)
    coverage_run = run('original-coverage', ['utgen', 'coverage', '-p', str(original)], original)
    assert counter.read_text() == 'x', 'coverage ran the test body more than once'
    env.pop('PALM_FIXTURE_RUN_LOG')
    assert snapshot(original) == baseline
    assert '2 passed; 0 failed' in coverage_run.stdout and '1 passed; 0 failed' in coverage_run.stdout
    original_coverage = json.loads((original / 'coverage.json').read_text())
    (original / 'coverage.json').rename(original / 'original-coverage.json')
    (original / 'coverage.xml').rename(original / 'original-coverage.xml')
    covered_functions = [function['name'] for data in original_coverage['data'] for function in data['functions']]
    assert not any(marker in name for name in covered_functions
                   for marker in ['existing_unit_test', 'before_helper', 'test_only_helper', 'record_run', 'existing_integration_test']), covered_functions
    assert any('classify' in name for name in covered_functions), covered_functions
    xml = ET.parse(original / 'original-coverage.xml')
    assert not any(marker in method.get('name', '') for method in xml.iter('method')
                   for marker in ['existing_unit_test', 'before_helper', 'test_only_helper', 'record_run'])
    # Test-only lines must disappear from both the numerator and denominator.
    lib_lines = (original / 'src/lib.rs').read_text().splitlines()
    test_line = next(i for i, line in enumerate(lib_lines, 1) if 'fn existing_unit_test' in line)
    for file in (file for data in original_coverage['data'] for file in data['files']):
        if Path(file['filename']) == original / 'src/lib.rs':
            assert not any(segment[0] == test_line and segment[3] for segment in file['segments'])
    for cls in xml.iter('class'):
        if cls.get('filename', '').endswith('src/lib.rs'):
            assert not any(int(line.get('number')) == test_line for line in cls.findall('./lines/line'))
    original_file = next(file for data in original_coverage['data'] for file in data['files']
                         if Path(file['filename']) == original / 'src/lib.rs')
    condition_line = (original / 'src/lib.rs').read_text().splitlines().index('    if support::positive(value) {') + 1
    # Library and unit-test binaries may emit separate records for one location.
    records = [branch for branch in original_file['branches'] if branch[0] == condition_line]
    assert any(branch[4] > 0 for branch in records) and any(branch[5] > 0 for branch in records)
    # Preserve assertion failures as test outcomes, but stop on build errors;
    # temporary coverage attributes must be removed in either case.
    lib = original / 'src/lib.rs'
    saved_lib = lib.read_bytes()
    try:
        lib.write_bytes(saved_lib.replace(b'assert_eq!(super::classify(2), 1)', b'assert_eq!(super::classify(2), 99)'))
        failed_test_source = snapshot(original)
        failed_test = run('coverage-failed-test', ['utgen', 'coverage', '-p', str(original)], original)
        assert '1 passed; 1 failed' in failed_test.stdout
        assert snapshot(original) == failed_test_source
        lib.write_bytes(saved_lib + b'\ncompile_error!("coverage build failure fixture");\n')
        failed_build_source = snapshot(original)
        failure = run('coverage-failed-build', ['utgen', 'coverage', '-p', str(original)], original, expected=1)
        assert 'coverage build failure fixture' in failure.stderr and 'panicked' not in failure.stderr
        assert snapshot(original) == failed_build_source
        lib.write_bytes(saved_lib)
        # Block only the JSON export, after the tests and XML export succeed.
        (original / 'coverage.json').unlink()
        (original / 'coverage.json').mkdir()
        try:
            report_failure = run('coverage-failed-report', ['utgen', 'coverage', '-p', str(original)], original, expected=1)
            assert 'report --json' in report_failure.stderr and 'panicked' not in report_failure.stderr
            assert snapshot(original) == baseline
        finally:
            (original / 'coverage.json').rmdir()
    finally:
        lib.write_bytes(saved_lib)
    assert snapshot(original) == baseline
    state = {'generation': 0, 'repair': 0, 'errors': [], 'integration': False, 'reject_repair': False}

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
                    answer = 'fn generated_helper() -> i32 { 1 }\n#[test]\nfn generated() {\n    assert_eq!(generated_helper(), 1);\n    ' + body + '\n}\n'
                    if state['integration']:
                        answer = {
                            'classify': '''use std::cmp::max as target;
use std::fmt::Write;
fn helper() -> i32 { 1 }
#[test]
fn alias_case() {
    let mut buffer = Vec::new();
    buffer.write_all(b"x").unwrap();
    assert_eq!(target(2), helper());
}
#[test]
fn sibling_case() {
    assert_eq!(target(2, 1), 2);
    assert_eq!(palm_fixture::classify(-1), 0);
}
''',
                            'double': '''use palm_fixture::nested::double as twice;
fn helper() -> i32 { "bad-helper" }
#[test]
fn body_case() {
    assert_eq!(twice("bad-body") + helper(), 5);
}
''',
                        }[function]
                    state['generation'] += 1
                else:
                    file = re.search(r'You can only modify lines \d+ to \d+ in file (.+?)\. For your answer', user).group(1)
                    replacements = {
                        'double("bad")': 'double(2)',
                        'use std::cmp::max as target;': 'use palm_fixture::classify as target;',
                        'use std::fmt::Write;': 'use std::io::Write;',
                        '"bad-helper"': '1',
                        '"bad-body"': '2',
                    }
                    changes = {}
                    for number, old in re.findall(r'^\[(\d+)\](.*)$', user, re.M):
                        new = old
                        for before, after in replacements.items():
                            new = new.replace(before, after)
                        if new != old:
                            changes[number] = (old, new)
                    assert changes, 'repair prompt lacks numbered failing code'
                    answer = f'ChangeLog:1@{file}\nFixDescription: Fix candidate imports and arguments.\n'
                    for number, (old, new) in changes.items():
                        answer += f'OriginalCode@{number}-{number}:\n[{number}]{old}\nFixedCode@{number}-{number}:\n[{number}]{new}\n'
                    state['repair'] += 1
                response = {'id': 'fixture', 'object': 'chat.completion', 'created': 0, 'model': 'fixture',
                            'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': answer}, 'finish_reason': 'stop'}],
                            'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}}
                code = 200
                if state['reject_repair']:
                    response = {'error': {'message': 'fixture request rejected'}}
                    code = 401
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
        # The last repaired candidate's raw data remains after per-function parsing.
        # Exporting it again must not run tests or contain llmtests/helper records.
        run('generated-coverage-report', ['cargo', 'llvm-cov', 'report', '--json', '--output-path', 'generated-coverage.json'])
        generated_coverage = json.loads((target / 'generated-coverage.json').read_text())
        generated_functions = [function['name'] for data in generated_coverage['data'] for function in data['functions']]
        assert generated_functions and not any('llmtests' in name or 'generated_helper' in name for name in generated_functions), generated_functions
        assert not state['errors'], state
        assert state['generation'] == 6 and state['repair'] >= 1, state
        for directory in ['result', 'fixed_result']:
            results = [json.loads(p.read_text()) for p in (target / 'utgen' / directory).glob('*.json')]
            assert len(results) == 5, (directory, results)
            if directory == 'fixed_result':
                assert all(r['tests_compiled'] == r['tests'] and r['tests_passed'] == r['tests_run'] and r['tests_run'] > 0 for r in results), results
            classify = next(r for r in results if r['function_name'] == 'palm_fixture::classify')
            assert classify['branches_covered'] == 2 and classify['branches'] == 2, classify
            assert all(not (branch['positive'] and branch['negative'])
                       for _, branches in classify['codes_branches_covered'] for branch in branches), classify
        double = json.loads((target / f"utgen/result/{names['palm_fixture::nested::double']}.json").read_text())
        assert double['tests_compiled'] == 0 and double['tests'] == 1, double
        assert not list((target / 'src').rglob('*.bak'))

        # Replay cached candidates on one function: ordinary and should_panic
        # loops must fail, and later passing tests must still execute.
        selected = 'palm_fixture::classify'
        selection = work / 'timeout-function.txt'
        selection.write_text(selected + '\n')
        candidate = target / f'utgen/generation/pre_fix/{names[selected]}.json'
        data = json.loads(candidate.read_text())
        data['fn_tests'] = data['fn_tests'][:1]
        answer = data['fn_tests'][0]['answers'][0]
        answer['chain_tests'] = [
            dict(attrs=attrs, prefix=[], oracles=[], codes=[['{', body, '}']],
                 can_compile=[{'Ok': None}], repaired=[False])
            for attrs, body in [
                ([], 'assert_eq!(classify(2), 1); loop { std::hint::spin_loop(); }'),
                (['#[should_panic]'], 'loop { std::hint::spin_loop(); }'),
                (['#[should_panic(expected = "expected")]'], 'panic!("expected");'),
                ([], 'assert_eq!(classify(-1), 0);'),
            ]
        ]
        candidate.write_text(json.dumps(data, indent=2) + '\n')
        # Keep original-test coverage separate when switching this fixture to
        # integration mode, which creates its own tests directory.
        original_tests = work / 'timeout-original-tests'
        shutil.move(target / 'tests.bak', original_tests)
        timeout_prepared = snapshot(target)
        requests_before = (state['generation'], state['repair'])
        for integration in [False, True]:
            data['integration'] = integration
            candidate.write_text(json.dumps(data, indent=2) + '\n')
            run(f'candidate-timeouts-{integration}',
                ['utgen', 'gen', '-p', str(target), '--functions-file', str(selection),
                 *(['--integration'] if integration else [])], timeout=90)
            result = json.loads((target / f'utgen/result/{names[selected]}.json').read_text())
            assert result['tests'] == result['tests_compiled'] == result['tests_run'] == 4, result
            assert result['tests_passed'] == 2, result
            assert result['oracles_run'] == 4 and result['oracles_passed'] == 2, result
            assert result['coverage_available'], result
            assert result['branches_covered'] == result['branches'] == 2, result
            if integration:
                assert not (target / 'tests.bak2').exists()
                shutil.rmtree(target / 'tests')
            assert snapshot(target) == timeout_prepared
            assert not list((target / 'src').rglob('*.bak'))
            assert (state['generation'], state['repair']) == requests_before, state
            report = json.loads((target / 'utgen/generation/gen-requests.json').read_text())
            assert report['attempts'] == 0, report
        shutil.move(original_tests, target / 'tests.bak')
        assert snapshot(target) == prepared

        # Multiple candidates belong to one oracle; all failed compilation is
        # still a valid result and must not attempt to export empty coverage.
        data['integration'] = True
        answer['common'] = []
        answer['chain_tests'] = [dict(
            attrs=[], prefix=[], oracles=[],
            codes=[['{', 'assert_eq!(classify(2), 1);', '}'],
                   ['{', 'assert_eq!(classify(-1), 1);', '}']],
            can_compile=[{'Ok': None}, {'Ok': None}], repaired=[False, False])]
        candidate.write_text(json.dumps(data))
        command = ['utgen', 'gen', '-p', str(target), '--functions-file', str(selection), '--integration']
        run('integration-oracle-group', command)
        result = json.loads((target / f'utgen/result/{names[selected]}.json').read_text())
        assert (result['tests_run'], result['tests_passed']) == (2, 1), result
        assert (result['oracles'], result['oracles_compiled'], result['oracles_run'], result['oracles_passed']) == (1, 1, 1, 1), result
        assert result['oracles_passed_rate'] == 100.0 and result['coverage_available'], result
        test = answer['chain_tests'][0]
        test['codes'] = [['{', 'missing();', '}']]
        test['can_compile'] = [{'Err': 'fixture compile failure'}]
        test['repaired'] = [False]
        candidate.write_text(json.dumps(data))
        run('integration-no-compiled-candidates', command)
        result = json.loads((target / f'utgen/result/{names[selected]}.json').read_text())
        assert result['tests'] == result['oracles'] == 1, result
        assert result['tests_compiled'] == result['tests_run'] == result['tests_passed'] == 0, result
        assert result['oracles_compiled'] == result['oracles_run'] == result['oracles_passed'] == 0, result
        assert not result['coverage_available'], result
        assert result['lines'] == result['branches'] == 0, result
        assert result['codes_lines_covered'] == [], result
        shutil.rmtree(target / 'tests')
        assert snapshot(target) == prepared

        # Original and generated integration tests must coexist, including on
        # a cached rerun and after original-test compilation fails.
        answer['chain_tests'] = [dict(
            attrs=[], prefix=[], oracles=[], codes=[['{', 'assert_eq!(classify(2), 1);', '}']],
            can_compile=[{'Ok': None}], repaired=[False])]
        candidate.write_text(json.dumps(data, indent=2) + '\n')
        integration_command = ['utgen', 'gen', '-p', str(target), '--functions-file', str(selection), '--integration']

        def integration_snapshot():
            # Cached generation rewrites imports from a HashSet in any order.
            return {p: re.sub(rb'(?m)(?:^use [^\n]*\n)+',
                              lambda m: b''.join(sorted(m[0].splitlines(keepends=True))), content)
                    if p.startswith('tests/') else content
                    for p, content in snapshot(target).items()}

        for label in ['integration-original-tests', 'integration-original-tests-cached']:
            run(label, integration_command)
            result = json.loads((target / f'utgen/result/{names[selected]}.json').read_text())
            assert result['tests_run'] == result['tests_passed'] == 1, result
            original_result = json.loads((target / 'utgen/original_result.json').read_text())
            assert [r['function_name'] for r in original_result] == [selected]
            assert original_result[0]['lines_covered'], original_result
            current = snapshot(target)
            assert {p: content for p, content in current.items() if not p.startswith('tests/')} == prepared
            assert any(p.startswith('tests/') for p in current)
            assert not (target / 'tests.bak2').exists()
        integration_prepared = integration_snapshot()
        original_file = target / 'tests.bak/original.rs'
        original_content = original_file.read_bytes()
        original_file.write_bytes(original_content + b'\ncompile_error!("original-test failure fixture");\n')
        failed_prepared = integration_snapshot()
        failure = run('integration-original-tests-compile-error', integration_command, expected=1)
        assert 'original-test failure fixture' in failure.stderr
        assert integration_snapshot() == failed_prepared
        assert not (target / 'tests.bak2').exists()
        original_file.write_bytes(original_content)
        assert integration_snapshot() == integration_prepared
        # An existing, even empty, staging directory must not be overwritten.
        staging = target / 'tests.bak2'
        staging.mkdir()
        failure = run('integration-existing-staging', integration_command, expected=1)
        assert 'tests.bak2 already exists' in failure.stderr
        assert staging.is_dir() and not list(staging.iterdir())
        assert integration_snapshot() == integration_prepared
        staging.rmdir()
        assert not list((target / 'src').rglob('*.bak'))
        assert (state['generation'], state['repair']) == requests_before, state
        report = json.loads((target / 'utgen/generation/gen-requests.json').read_text())
        assert report['attempts'] == 0, report
        shutil.rmtree(target / 'tests')
        assert snapshot(target) == prepared

        # Exercise the complete integration path with real Rust diagnostics.
        # One answer has two candidates needing different meanings of `target`.
        state['integration'] = True
        selected_names = [selected, 'palm_fixture::nested::double']
        integration_selection = work / 'integration-functions.txt'
        integration_selection.write_text('\n'.join(selected_names) + '\n')
        for directory in ['generation/pre_fix', 'generation/llm_fix']:
            for name in selected_names:
                (target / 'utgen' / directory / (names[name] + '.json')).unlink()
        integration_args = ['-p', str(target), '--functions-file', str(integration_selection),
                            '--integration', '--tasks', '2', '--max-requests', '8']
        run('integration-generate-imports', ['utgen', 'gen', *integration_args])
        for name, compiled in zip(selected_names, [1, 0]):
            result = json.loads((target / f'utgen/result/{names[name]}.json').read_text())
            assert result['tests_compiled'] == result['tests_run'] == compiled, result
        source_before = {p: content for p, content in snapshot(target).items() if not p.startswith('tests/')}
        assert source_before == prepared
        # Unselected targets must neither pollute diagnostics nor be executed.
        unrelated = target / 'tests/unrelated.rs'
        unrelated.write_text('compile_error!("unselected test must not compile");\n')
        temporary_file = target / 'tests/palm_candidate.rs'
        temporary_file.write_text('compile_error!("pre-existing temporary path");\n')
        before_fix = snapshot(target)
        requests_before = (state['generation'], state['repair'])
        wrong_mode = run('integration-reject-unit-fix',
                         ['utgen', 'fix', '-p', str(target), '--functions-file', str(integration_selection)], expected=1)
        assert 'Test mode mismatch' in wrong_mode.stderr
        assert snapshot(target) == before_fix
        assert (state['generation'], state['repair']) == requests_before
        wrong_mode = run('integration-reject-unit-gen',
                         ['utgen', 'gen', '-p', str(target), '--functions-file', str(integration_selection)], expected=1)
        assert 'Test mode mismatch' in wrong_mode.stderr
        assert snapshot(target) == before_fix
        assert (state['generation'], state['repair']) == requests_before
        run('integration-repair-imports', ['utgen', 'fix', *integration_args])
        assert state['repair'] > requests_before[1]
        for name, count in zip(selected_names, [2, 1]):
            result = json.loads((target / f'utgen/fixed_result/{names[name]}.json').read_text())
            assert result['tests'] == result['tests_compiled'] == result['tests_run'] == result['tests_passed'] == count, result
            assert len(result['codes_lines_covered']) == count, result
            assert result['lines_covered'] > 0 and result['coverage_available'], result
            assert result['oracles_run'] == result['oracles_passed'] == count, result
        fixed_path = target / f'utgen/generation/llm_fix/{names[selected]}.json'
        fixed = json.loads(fixed_path.read_text())
        tests = fixed['fn_tests'][0]['answers'][0]['chain_tests']
        assert 'use palm_fixture::classify as target;' in tests[0]['integration_contexts'][0]['uses']
        assert 'use std::io::Write;' in tests[0]['integration_contexts'][0]['uses']
        assert 'use std::cmp::max as target;' in tests[1]['integration_contexts'][0]['uses']
        assert {p: content for p, content in snapshot(target).items() if not p.startswith('tests/')} == prepared
        assert unrelated.read_bytes() == before_fix['tests/unrelated.rs']
        assert temporary_file.read_bytes() == before_fix['tests/palm_candidate.rs']
        assert not list((target / 'src').rglob('*.bak'))
        stable = snapshot(target)
        requests_before = (state['generation'], state['repair'])
        run('integration-repair-cached', ['utgen', 'fix', *integration_args])
        assert snapshot(target) == stable
        assert (state['generation'], state['repair']) == requests_before
        report = json.loads((target / 'utgen/generation/fix-requests.json').read_text())
        assert report['attempts'] == 0 and report['invocation']['integration'], report
        source_file = target / 'src/lib.rs'
        original_source = source_file.read_bytes()
        source_file.write_bytes(original_source + b'\ncompile_error!("production compilation failure");\n')
        broken = snapshot(target)
        failure = run('integration-repair-build-error', ['utgen', 'fix', *integration_args], expected=1)
        assert 'production compilation failure' in failure.stderr
        assert snapshot(target) == broken
        assert (state['generation'], state['repair']) == requests_before
        assert not list((target / 'src').rglob('*.bak'))
        source_file.write_bytes(original_source)
        # Request failure must restore the temporary input without touching src/.
        saved_fixed = {}
        for name in selected_names:
            path = target / f'utgen/generation/llm_fix/{names[name]}.json'
            saved_fixed[path] = path.read_bytes()
            path.unlink()
        state['reject_repair'] = True
        run('integration-repair-request-error', ['utgen', 'fix', *integration_args], expected=1)
        state['reject_repair'] = False
        assert snapshot(target) == stable
        assert not list((target / 'src').rglob('*.bak'))
        for path, content in saved_fixed.items():
            path.write_bytes(content)
        assert not state['errors'], state
        (work / 'summary.json').write_text(json.dumps(state, indent=2) + '\n')
        print('Minimal analysis/generation/repair/coverage checks passed.', flush=True)
    finally:
        server.shutdown()
        server.server_close()
        (work / 'model-requests.json').write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
