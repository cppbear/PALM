#!/usr/bin/env python3
"""Validate mixed lib/bin ownership with small crates and local model responses."""
import argparse
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=REPO / 'target/debug')
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='palm-mixed-')).resolve()
    root = work / 'crate'
    shutil.copytree(REPO / 'examples/mixed-targets', root, ignore=shutil.ignore_patterns('target', 'Cargo.lock'))
    print(f'Validation artifacts: {work}', flush=True)
    env = os.environ.copy()
    for key in ['CARGO_TARGET_DIR', 'PALM_CONFIG', 'PALM_API_BASE', 'PALM_API_KEY', 'PALM_MODEL']:
        env.pop(key, None)
    env['PATH'] = str(args.bin_dir.resolve()) + os.pathsep + env['PATH']
    for key in ['NO_PROXY', 'no_proxy']:
        env[key] = ','.join(filter(None, [env.get(key), '127.0.0.1', 'localhost']))

    def run(label, command, expected=0, directory=root):
        print(label, flush=True)
        result = subprocess.run(command, cwd=directory, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=180)
        (work / f'{label}.log').write_text(result.stdout)
        assert (result.returncode == 0) == (expected == 0), f'{label}: exit {result.returncode}\n{result.stdout[-7000:]}'
        return result.stdout

    run('preprocess', ['utgen', 'pre-process', '-p', str(root)])
    run('analyze', ['utgen', 'analyze', '-p', str(root)])
    names = json.loads((root / 'brinfo/name_map.json').read_text())
    lib = 'lib:mixed_fixture::mixed_fixture::'
    binary = 'bin:mixed_fixture::mixed_fixture::'
    assert set(names) == {lib + 'helper', lib + 'lib_only', lib + 'shared::classify',
                          binary + 'helper', binary + 'bin_only', binary + 'library_type', binary + 'main'}, names
    assert names[lib + 'helper'] != names[binary + 'helper']
    contexts = {key: (root / f'focxt/{encoded}.rs').read_text() for key, encoded in names.items()}
    assert 'value + 10' in contexts[lib + 'shared::classify']
    assert 'value - 10' not in contexts[lib + 'shared::classify']
    assert 'value - 10' in contexts[binary + 'helper'] and 'value + 10' not in contexts[binary + 'helper']
    local, dependency = contexts[binary + 'bin_only'].split('// Dependency context from library crate mixed_fixture')
    assert 'fn classify' in local and 'value + 10' not in local
    assert 'value + 10' in dependency
    assert 'pub struct Subject' in contexts[binary + 'library_type']
    raw = root / 'brinfo/targets/bin/mixed_fixture/focxt'
    raw_infos = json.loads((raw / 'impl_informations.json').read_text())
    shared = next(info for info in raw_infos if info['full_name'] == 'mixed_fixture::shared::classify')
    assert 'value - 10' in (raw / f"{shared['encoded_name']}.rs").read_text()

    selection = work / 'functions.txt'
    selection.write_text('mixed_fixture::helper\n')
    env.update(PALM_API_BASE='http://127.0.0.1:9/v1', PALM_API_KEY='unused', PALM_MODEL='fixture')
    output = run('ambiguous-selection', ['utgen', 'gen', '-p', str(root), '--functions-file', str(selection)], expected=1)
    assert 'Ambiguous function' in output and lib + 'helper' in output and binary + 'helper' in output
    selection.write_text(binary + 'bin_only\n')
    output = run('reject-binary-integration', ['utgen', 'gen', '-p', str(root), '--functions-file', str(selection), '--integration'], expected=1)
    assert 'belongs to a binary' in output and not (root / 'utgen').exists()

    state = {'generation': 0, 'repair': 0, 'errors': []}

    class Model(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            try:
                request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                user = request['messages'][-1]['content']
                if 'The function to be tested is presented as follows:' in user:
                    focal = user.split('The function to be tested is presented as follows:', 1)[1]
                    function = re.search(r'\bfn (\w+)\(', focal).group(1)
                    if function == 'classify':
                        body = 'assert_eq!(classify(1), 11); assert_eq!(classify(-1), 0);'
                    elif function == 'bin_only':
                        body = 'assert_eq!(bin_only("wrong-type"), 2);'
                    elif function == 'helper':
                        expected = '11' if 'value + 10' in focal else '-9'
                        body = f'assert_eq!(helper(1), {expected});'
                    else:
                        raise AssertionError(function)
                    answer = '```rust\n#[test]\nfn generated() {\n' + body + '\n}\n```'
                    state['generation'] += 1
                else:
                    file = re.search(r'You can only modify lines \d+ to \d+ in file (.+?)\. For your answer', user).group(1)
                    changes = [(number, old, old.replace('"wrong-type"', '1'))
                               for number, old in re.findall(r'^\[(\d+)\](.*)$', user, re.M)
                               if '"wrong-type"' in old]
                    assert changes, user
                    answer = f'ChangeLog:1@{file}\nFixDescription: Use an integer argument.\n'
                    for number, old, new in changes:
                        answer += f'OriginalCode@{number}-{number}:\n[{number}]{old}\nFixedCode@{number}-{number}:\n[{number}]{new}\n'
                    state['repair'] += 1
                (work / f"model-{state['generation']}-{state['repair']}.json").write_text(json.dumps({'prompt': user, 'answer': answer}, indent=2))
                response = {'id': 'fixture', 'object': 'chat.completion', 'created': 0, 'model': 'fixture',
                            'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': answer}, 'finish_reason': 'stop'}],
                            'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}}
                status = 200
            except Exception as error:
                state['errors'].append(str(error))
                response, status = {'error': {'message': str(error)}}, 400
            data = json.dumps(response).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Model)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env.update(PALM_API_BASE=f'http://127.0.0.1:{server.server_port}/v1', PALM_API_KEY='local-fixture-key')
    selected = [lib + 'shared::classify', lib + 'helper', binary + 'helper', binary + 'bin_only']
    selection.write_text('\n'.join(selected) + '\n')
    sources = {p: p.read_bytes() for p in (root / 'src').rglob('*.rs')}
    try:
        run('generate', ['utgen', 'gen', '-p', str(root), '--functions-file', str(selection), '--context'])
        assert state['generation'] == 4 and state['repair'] == 0 and not state['errors'], state
        assert sources == {p: p.read_bytes() for p in sources}
        def result(name, directory='result'):
            return json.loads((root / f'utgen/{directory}/{names[name]}.json').read_text())
        for name in selected[:-1]:
            row = result(name)
            assert row['tests_run'] == row['tests_passed'] == 1 and row['coverage_available'], row
            assert row['target']['kind'] == ('lib' if name.startswith('lib:') else 'bin'), row
        row = result(selected[-1])
        assert row['tests_compiled'] == row['tests_run'] == 0 and row['coverage_available'], row
        assert row['codes_lines_covered'] == [], row
        run('repair', ['utgen', 'fix', '-p', str(root), '--functions-file', str(selection)])
        assert state['repair'] >= 1 and not state['errors'], state
        for name in selected:
            row = result(name, 'fixed_result')
            assert row['tests_run'] == row['tests_passed'] == 1, row
        before = state.copy()
        run('cached-repair', ['utgen', 'fix', '-p', str(root), '--functions-file', str(selection)])
        assert state == before
        assert sources == {p: p.read_bytes() for p in sources}
        assert not list((root / 'src').rglob('*.bak'))
        # A broken unselected binary must not enter a library unit-test build.
        main_file = root / 'src/main.rs'
        main_file.write_bytes(sources[main_file] + b'\ncompile_error!("unselected binary");\n')
        selection.write_text(lib + 'shared::classify\n')
        run('library-with-broken-bin', ['utgen', 'gen', '-p', str(root), '--functions-file', str(selection)])
        assert result(lib + 'shared::classify')['tests_passed'] == 1
        main_file.write_bytes(sources[main_file])
        # Automatic integration mode uses all library representatives, and
        # leaves binary caches alone. Replay fixed candidates without requests.
        template = json.loads((root / f"utgen/generation/pre_fix/{names[lib + 'helper']}.json").read_text())
        for name in [lib + 'helper', lib + 'shared::classify', lib + 'lib_only']:
            data = json.loads(json.dumps(template))
            branch = json.loads((root / f'brinfo/brdata/{names[name]}.json').read_text())
            for field in ['id', 'name', 'name_with_impl', 'mod_info', 'visible', 'loc', 'target']:
                data[field] = branch[field]
            data['integration'] = True
            function = branch['name'].split('::')[-1]
            test = data['fn_tests'][0]['answers'][0]['chain_tests'][0]
            test['codes'] = [['{', f'assert_eq!({function}(1), 11);', '}']]
            (root / f'utgen/generation/pre_fix/{names[name]}.json').write_text(json.dumps(data))
        shutil.rmtree(root / 'utgen/generation/llm_fix')
        requests = (state['generation'], state['repair'])
        output = run('integration-auto', ['utgen', 'gen', '-p', str(root), '--integration'])
        assert 'omits 4 binary focal functions' in output
        run('integration-repair', ['utgen', 'fix', '-p', str(root), '--integration'])
        assert (state['generation'], state['repair']) == requests, state
        for name in [lib + 'helper', lib + 'shared::classify', lib + 'lib_only']:
            for directory in ['result', 'fixed_result']:
                row = result(name, directory)
                assert row['target']['kind'] == 'lib' and row['tests_run'] == row['tests_passed'] == 1, row
        assert sources == {p: p.read_bytes() for p in sources}
        # Missing ownership in a legacy mixed cache is an error, not a guess.
        cached = root / f"utgen/generation/pre_fix/{names[lib + 'helper']}.json"
        saved = cached.read_bytes()
        data = json.loads(saved)
        data.pop('target')
        cached.write_text(json.dumps(data))
        output = run('reject-legacy-mixed-cache', ['utgen', 'gen', '-p', str(root), '--integration'], expected=1)
        assert 'lack target ownership' in output
        assert (state['generation'], state['repair']) == requests
        cached.write_bytes(saved)
    finally:
        server.shutdown()
        server.server_close()
    # Real Cargo names and entry files can differ from the package and defaults.
    custom = work / 'custom'
    shutil.copytree(REPO / 'examples/mixed-targets', custom, ignore=shutil.ignore_patterns('target', 'Cargo.lock'))
    (custom / 'src/lib.rs').rename(custom / 'src/core.rs')
    (custom / 'tools').mkdir()
    original_main = (custom / 'src/main.rs').read_text()
    (custom / 'src/main.rs').unlink()
    (custom / 'tools/cli.rs').write_text(original_main.replace('mod shared;', '#[path = "../src/shared.rs"] mod shared;').replace('mixed_fixture::', 'mixed_core::') + '\n#[test] fn existing() { panic!("must be preprocessed"); }\n')
    with (custom / 'Cargo.toml').open('a') as manifest:
        manifest.write('\n[lib]\nname="mixed_core"\npath="src/core.rs"\n[[bin]]\nname="mixed_tool"\npath="tools/cli.rs"\n')
    with (custom / 'src/shared.rs').open('a') as source:
        source.write('\npub struct Generic<T>(pub T);\nimpl<T: Copy> Generic<T> { pub fn get(&self) -> T { self.0 } }\nimpl From<i32> for Generic<i32> { fn from(value: i32) -> Self { Self(value) } }\n')
    with (custom / 'tools/cli.rs').open('a') as source:
        source.write('\nfn library_method(subject: &mixed_core::shared::Generic<i32>) -> i32 { subject.get() }\n')
    run('custom-preprocess', ['utgen', 'pre-process', '-p', str(custom)], directory=custom)
    run('custom-analyze', ['utgen', 'analyze', '-p', str(custom)], directory=custom)
    custom_names = json.loads((custom / 'brinfo/name_map.json').read_text())
    assert 'lib:mixed_core::mixed_core::shared::classify' in custom_names
    assert 'bin:mixed_tool::mixed_tool::shared::classify' not in custom_names
    custom_infos = json.loads((custom / 'focxt/impl_informations.json').read_text())
    for method in ['get', 'from']:
        matches = [info for info in custom_infos if info['fn_name'] == method]
        assert len(matches) == 1 and matches[0]['target']['kind'] == 'lib', matches
    method_context = (custom / f"focxt/{custom_names['bin:mixed_tool::mixed_tool::library_method']}.rs").read_text()
    assert 'fn get' in method_context and 'self.0' in method_context, method_context
    custom_bin = 'bin:mixed_tool::mixed_tool::bin_only'
    data = json.loads((custom / f"brinfo/brdata/{custom_names[custom_bin]}.json").read_text())
    assert data['target']['src_path'] == 'tools/cli.rs', data
    assert 'Dependency context from library crate mixed_core' in (custom / f"focxt/{custom_names[custom_bin]}.rs").read_text()

    custom_candidate = json.loads(json.dumps(template))
    for field in ['id', 'name', 'name_with_impl', 'mod_info', 'visible', 'loc', 'target']:
        custom_candidate[field] = data[field]
    custom_candidate['integration'] = False
    test = custom_candidate['fn_tests'][0]['answers'][0]['chain_tests'][0]
    test['codes'] = [['{', 'assert_eq!(bin_only(1), 2);', '}']]
    cache = custom / f"utgen/generation/pre_fix/{custom_names[custom_bin]}.json"
    cache.parent.mkdir(parents=True)
    cache.write_text(json.dumps(custom_candidate))
    selection.write_text(custom_bin + '\n')
    run('custom-unit-statistics', ['utgen', 'gen', '-p', str(custom), '--functions-file', str(selection)], directory=custom)
    row = json.loads((custom / f"utgen/result/{custom_names[custom_bin]}.json").read_text())
    assert row['target']['src_path'] == 'tools/cli.rs' and row['tests_run'] == row['tests_passed'] == 1, row

    # With no library representative, retain the definition in each binary.
    bins = work / 'bins'
    (bins / 'src').mkdir(parents=True)
    (bins / 'Cargo.toml').write_text('[package]\nname="bin_fixture"\nversion="0.1.0"\nedition="2021"\n[workspace]\n[[bin]]\nname="one"\npath="src/one.rs"\n[[bin]]\nname="two"\npath="src/two.rs"\n')
    (bins / 'rust-toolchain.toml').write_bytes((REPO / 'rust-toolchain.toml').read_bytes())
    (bins / 'src/shared.rs').write_bytes((root / 'src/shared.rs').read_bytes())
    for name, value in [('one', 1), ('two', 2)]:
        (bins / f'src/{name}.rs').write_text(f'mod shared;\nfn helper(value: i32) -> i32 {{ value + {value} }}\nfn main() {{}}\n')
    run('multiple-bins-analyze', ['utgen', 'analyze', '-p', str(bins)], directory=bins)
    bin_names = json.loads((bins / 'brinfo/name_map.json').read_text())
    for name, value in [('one', 1), ('two', 2)]:
        key = f'bin:{name}::{name}::shared::classify'
        assert key in bin_names, bin_names
        assert f'value + {value}' in (bins / f'focxt/{bin_names[key]}.rs').read_text()
    # Cargo metadata can include sibling workspace members. Preprocessing
    # a selected package must not change a sibling's declared entry file.
    workspace = work / 'workspace'
    workspace.mkdir()
    (workspace / 'Cargo.toml').write_text('[workspace]\nmembers=["first", "second"]\nresolver="2"\n')
    for name in ['first', 'second']:
        package = workspace / name
        (package / 'src').mkdir(parents=True)
        (package / 'Cargo.toml').write_text(f'[package]\nname="{name}"\nversion="0.1.0"\n[lib]\npath="entry.rs"\n')
        (package / 'entry.rs').write_text('pub fn production() {}\n#[test] fn original() {}\n')
    sibling = (workspace / 'second/entry.rs').read_bytes()
    run('preprocess-selected-package', ['utgen', 'pre-process', '-p', str(workspace / 'first')])
    assert b'fn original' not in (workspace / 'first/entry.rs').read_bytes()
    assert (workspace / 'second/entry.rs').read_bytes() == sibling
    (work / 'summary.json').write_text(json.dumps(state, indent=2))
    print('Mixed-target analysis, generation, repair and coverage checks passed.', flush=True)


if __name__ == '__main__':
    main()
