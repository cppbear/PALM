#!/usr/bin/env python3
"""Validate the bundled bytes analysis in fresh copies, without a model service."""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import tempfile
import time

from check_minimal import snapshot, source_fragment

REPO = Path(__file__).resolve().parents[1]


def check_artifacts(root, expected):
    names = json.loads((root / 'brinfo/name_map.json').read_text())
    infos = json.loads((root / 'focxt/impl_informations.json').read_text())
    contexts = {info['full_name']: info for info in infos}
    assert len(names) == expected, (len(names), expected)
    assert set(names) == set(contexts) and len(infos) == expected
    encoded = set(names.values())
    for directory in ['brinfo/brdata', 'focxt/callsandtypes', 'focxt/new_callsandtypes']:
        assert {p.stem for p in (root / directory).glob('*.json')} == encoded, directory
    assert {p.stem for p in (root / 'focxt').glob('*.rs')} == encoded
    for name, key in names.items():
        info = contexts[name]
        assert info['encoded_name'] == key, name
        data = json.loads((root / f'brinfo/brdata/{key}.json').read_text())
        assert data['name'] == name and data['loc'] == info['loc'], name
        actual = source_fragment(root, data['loc']).splitlines()
        assert [line.lstrip() for line in actual] == [line.lstrip() for line in data['code']], name
        assert [line.lstrip() for line in actual] == [line.lstrip() for line in info['code'].splitlines()], name
        context = (root / f'focxt/{key}.rs').read_text()
        assert context.strip() and re.search(r'\bfn\s+' + re.escape(info['fn_name']) + r'\b', context), name
    return names, contexts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=REPO / 'target/release')
    parser.add_argument('--runs', type=int, default=2, choices=[1, 2])
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='palm-bytes-check-')).resolve()
    print(f'Analysis artifacts: {work}', flush=True)
    env = os.environ.copy()
    env.pop('CARGO_TARGET_DIR', None)
    for name in ['PALM_CONFIG', 'PALM_API_BASE', 'PALM_API_KEY', 'PALM_MODEL']:
        env.pop(name, None)
    env['PATH'] = str(args.bin_dir.resolve()) + os.pathsep + env['PATH']
    summary = {'platform': platform.platform(), 'bin_dir': str(args.bin_dir.resolve()), 'runs': []}
    summary['rustc'] = subprocess.check_output(['rustc', '--version'], cwd=REPO, env=env, text=True).strip()

    def run(root, label, command):
        log = root.parent / (root.name + '-' + label + '.log')
        start = time.monotonic()
        with log.open('w') as output:
            process = subprocess.Popen(command, cwd=root, env=env, stdout=output, stderr=subprocess.STDOUT,
                                       start_new_session=True)
            try:
                status = process.wait(timeout=300)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                raise AssertionError(f'{label}: external 300-second check limit; see {log}')
        elapsed = round(time.monotonic() - start, 3)
        print(f'{root.name} {label}: exit={status}, {elapsed}s', flush=True)
        assert status == 0, f'{label}: see {log}\n{log.read_text()[-3000:]}'
        return elapsed, log.read_text()

    fixture = work / 'identities'
    (fixture / 'src').mkdir(parents=True)
    (fixture / 'Cargo.toml').write_text('[package]\nname="context_fixture"\nversion="0.1.0"\nedition="2021"\n[workspace]\n')
    shutil.copyfile(REPO / 'rust-toolchain.toml', fixture / 'rust-toolchain.toml')
    (fixture / 'src/lib.rs').write_text('''pub struct Subject;
pub trait Probe<T> {
    fn probe(&self, input: T) -> u16;
    fn tag(&self) -> u16 { 303 }
}
fn helper() -> u16 { 101 }
impl Probe<u8> for Subject { #[cfg(any())] fn probe(&self, _: u8) -> u16 { 999 } #[cfg(not(any()))] fn probe(&self, _: u8) -> u16 { helper() } } impl Probe<u16> for Subject { fn probe(&self, _: u16) -> u16 { 202 } }
macro_rules! forward { () => { fn probe(&self, _: u8) -> u16 { 404 } }; }
impl Probe<u8> for &Subject { forward!(); }
''')
    before = snapshot(fixture)
    run(fixture, 'analyze', ['utgen', 'analyze', '-p', str(fixture)])
    assert snapshot(fixture) == before
    _, infos = check_artifacts(fixture, 5)
    seen = set()
    for info in infos.values():
        context = (fixture / f"focxt/{info['encoded_name']}.rs").read_text()
        if info['impl_loc']:
            header = source_fragment(fixture, info['impl_loc']).strip()
            assert header in context, header
            if header == 'impl Probe<u8> for Subject':
                assert 'helper()' in context and '101' in context and '999' not in context
            elif header == 'impl Probe<u16> for Subject':
                assert '202' in context
            else:
                assert header == 'impl Probe<u8> for &Subject'
                assert re.search(r'impl Probe<u8> for &Subject\s*\{\s*fn probe\([^}]+404', context)
            seen.add(header)
        elif info['fn_name'] == 'tag':
            assert re.search(r'fn tag\([^}]+303', context)
            seen.add('default method')
    assert len(seen) == 4, seen

    original = snapshot(REPO / 'examples/bytes')
    for number in range(1, args.runs + 1):
        root = work / f'bytes-{number}'
        shutil.copytree(REPO / 'examples/bytes', root,
                        ignore=shutil.ignore_patterns('target', 'brinfo', 'focxt', 'utgen', 'tests.bak'))
        preprocess, _ = run(root, 'preprocess', ['utgen', 'pre-process', '-p', str(root)])
        prepared = snapshot(root)
        elapsed, output = run(root, 'analyze', ['utgen', 'analyze', '-p', str(root)])
        assert snapshot(root) == prepared, 'analysis modified prepared sources'
        names, infos = check_artifacts(root, 663)
        # Keep the future model-smoke lists aligned with real compiler identities.
        for count, chains in [(2, 2), (8, 16)]:
            selected = (REPO / f'examples/bytes-smoke-{count}.txt').read_text().splitlines()
            assert len(selected) == len(set(selected)) == count
            assert set(selected) <= names.keys()
            actual_chains = sum(json.loads((root / f'brinfo/brdata/{names[name]}.json').read_text())
                                ['size']['min_set'] for name in selected)
            assert actual_chains == chains, (count, actual_chains)
        # Verify representative default, generic-impl, and macro-generated methods.
        for name, info in infos.items():
            context = (root / f"focxt/{info['encoded_name']}.rs").read_text()
            if name == 'bytes::buf::buf_impl::Buf::has_remaining':
                assert 'self.remaining() > 0' in context
            if info['fn_name'] == 'eq' and info['mod_name'] == 'bytes::bytes':
                assert 'PartialEq' in context
            if info['fn_name'] == 'get_u8' and info['impl_loc'] and info['mod_name'] == 'bytes::buf::buf_impl':
                assert '(**self).get_u8()' in context
        stages = {name: float(seconds) for name, seconds in re.findall(r'Analysis step (\w+): ([\d.]+)s', output)}
        assert set(stages) == {'clean', 'brinfo', 'focxt'}, stages
        summary['runs'].append({'preprocess_seconds': preprocess, 'analyze_seconds': elapsed,
                                'stages_seconds': stages, 'functions': len(names), 'contexts': len(infos)})
        (work / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    assert snapshot(REPO / 'examples/bytes') == original
    print('All bytes analysis checks passed.', flush=True)


if __name__ == '__main__':
    main()
