#!/usr/bin/env python3
"""Compile coverage compatibility cases in temporary standalone crates."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import xml.etree.ElementTree as ET

REPO = Path(__file__).resolve().parents[1]
PRODUCTION = 'pub fn production() -> u8 { 42 }\n'
TEST = '#[test] fn checks() { assert_eq!(production(), 42); }\n'
NIGHTLY_GATE = '#![cfg_attr(coverage_nightly, feature(coverage_attribute))]\n'
CONDITIONAL = {
    'src/lib.rs': (
        '#![cfg_attr(all(coverage_nightly, feature="existing"), feature(coverage_attribute))]\n'
        + PRODUCTION
        + '#[cfg_attr(coverage_nightly, cfg_attr(feature="existing", coverage(off)))]\n'
        + TEST
    ),
}
CASES = {
    'unconditional': {
        'src/lib.rs': '#![feature(coverage_attribute)]\n' + PRODUCTION + '#[coverage(off)]\n' + TEST,
    },
    'conditional_off': {
        'src/lib.rs': NIGHTLY_GATE + PRODUCTION + '#[cfg_attr(coverage_nightly, coverage(off))]\n' + TEST,
    },
    'test_gate': {
        'src/lib.rs': '#![cfg_attr(test, feature(coverage_attribute))]\n' + PRODUCTION + TEST,
    },
    'all_gate': {
        'src/lib.rs': '#![cfg_attr(all(coverage_nightly), feature(coverage_attribute))]\n' + PRODUCTION + TEST,
    },
    'feature_disabled': CONDITIONAL,
    'feature_enabled': CONDITIONAL,
    'method_cfg': {
        'src/lib.rs': PRODUCTION + 'struct S;\nimpl S { #[cfg(test)] fn method_helper() -> u8 { production() } }\n'
                      '#[test] fn checks() { assert_eq!(S::method_helper(), 42); }\n',
    },
    'impl_cfg': {
        'src/lib.rs': PRODUCTION + 'struct S;\n#[cfg(test)] impl S { fn impl_helper() -> u8 { production() } }\n'
                      '#[test] fn checks() { assert_eq!(S::impl_helper(), 42); }\n',
    },
    'inner_cfg': {
        'src/lib.rs': PRODUCTION + 'mod support;\n',
        'src/support.rs': '#![cfg(test)]\nfn inner_helper() -> u8 { super::production() }\n'
                          '#[test] fn checks() { assert_eq!(inner_helper(), 42); }\n',
    },
    'outer_cfg': {
        'src/lib.rs': PRODUCTION + '#[cfg(test)] #[cfg_attr(coverage_nightly, coverage(off))] mod support;\n',
        'src/support.rs': 'fn outer_helper() -> u8 { super::production() }\n'
                          '#[test] fn checks() { assert_eq!(outer_helper(), 42); }\n',
    },
    'expression_fragment': {
        'src/lib.rs': 'pub fn production() -> u8 { include!("value.rs") }\n' + TEST,
        'src/value.rs': '42\n',
    },
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=REPO / 'target/debug')
    args = parser.parse_args()
    utgen = str(args.bin_dir.resolve() / 'utgen')
    work = Path(tempfile.mkdtemp(prefix='palm-coverage-check-')).resolve()
    print(f'Coverage compatibility artifacts: {work}', flush=True)
    env = os.environ.copy()
    env.pop('CARGO_TARGET_DIR', None)
    for name in ['PALM_CONFIG', 'PALM_API_BASE', 'PALM_API_KEY', 'PALM_MODEL']:
        env.pop(name, None)

    def run(root, label, command, success=True):
        result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True, timeout=180)
        log = root / (label + '.log')
        log.write_text(result.stdout + result.stderr)
        assert (result.returncode == 0) == success, f'{label}: see {log}\n{(result.stdout + result.stderr)[-6000:]}'
        return result.stdout

    for name, sources in CASES.items():
        print(name, flush=True)
        root = work / name
        root.mkdir()
        default = '["existing"]' if name == 'feature_enabled' else '[]'
        (root / 'Cargo.toml').write_text(
            '[package]\nname="coverage_fixture"\nversion="0.1.0"\nedition="2021"\n'
            f'[features]\nexisting=[]\ndefault={default}\n'
            '[lints.rust]\nunexpected_cfgs={level="deny", check-cfg=["cfg(coverage_nightly)"]}\n'
        )
        (root / 'rust-toolchain.toml').write_text('[toolchain]\nchannel="nightly-2025-03-19"\n')
        # An integration test also compiles the library without cfg(test), so
        # conditional feature gates must work in both compilation modes.
        sources = sources | {'tests/integration.rs':
                             '#[test] fn integration() { assert_eq!(coverage_fixture::production(), 42); }\n'}
        for relative, content in sources.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        baseline = {relative: (root / relative).read_bytes() for relative in sources}
        original = run(root, 'ordinary-tests', ['cargo', 'test', '--offline', '--tests'])
        covered = run(root, 'coverage', [utgen, 'coverage', '-p', str(root)])
        outcomes = r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;'
        assert re.findall(outcomes, original) == re.findall(outcomes, covered)
        assert all((root / relative).read_bytes() == contents for relative, contents in baseline.items())
        report = json.loads((root / 'coverage.json').read_text())
        functions = [f for data in report['data'] for f in data['functions']]
        assert functions and all('production' in f['name'] for f in functions), functions
        assert all(f['count'] > 0 for f in functions), functions
        xml = ET.parse(root / 'coverage.xml')
        assert all('helper' not in m.get('name', '') and 'checks' not in m.get('name', '')
                   for m in xml.iter('method'))

        run(root, 'preprocess', [utgen, 'pre-process', '-p', str(root)])
        for relative, original_bytes in baseline.items():
            if not relative.startswith('src/'):
                continue
            prepared = (root / relative).read_bytes()
            assert len(prepared) == len(original_bytes)
            assert [i for i, b in enumerate(prepared) if b == 10] == [i for i, b in enumerate(original_bytes) if b == 10]
        if name == 'expression_fragment':
            assert (root / 'src/value.rs').read_bytes() == baseline['src/value.rs']
        prepared_output = run(root, 'prepared-tests', ['cargo', 'test', '--offline', '--tests'])
        assert re.findall(r'running (\d+) tests?', prepared_output) == ['0'], prepared_output

    # A custom Cargo entry path is still an entry, not an include! fragment.
    root = work / 'invalid_entry'
    (root / 'src').mkdir(parents=True)
    (root / 'Cargo.toml').write_text(
        '[package]\nname="invalid_entry"\nversion="0.1.0"\nedition="2021"\n'
        '[lib]\npath="src/entry.rs"\n'
    )
    (root / 'rust-toolchain.toml').write_text('[toolchain]\nchannel="nightly-2025-03-19"\n')
    (root / 'src/entry.rs').write_text('42\n')
    for command in ['coverage', 'pre-process']:
        run(root, command, [utgen, command, '-p', str(root)], success=False)
        assert (root / 'src/entry.rs').read_text() == '42\n'
    print(f'All {len(CASES)} coverage/preprocessing cases passed.', flush=True)


if __name__ == '__main__':
    main()
