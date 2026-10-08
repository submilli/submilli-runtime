#!/usr/bin/env python3
"""Validate evaluation ground truth through real packages and restricted policies."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent


def invoke(binary, cwd, home, *args):
    result = subprocess.run([str(binary), *map(str, args)], cwd=cwd,
                            env=dict(os.environ, SUBMILLI_HOME=str(home), SUBMILLI_TELEMETRY='0'),
                            text=True, capture_output=True, timeout=120)
    if result.returncode:
        raise RuntimeError(f'{args}:\n{result.stdout}\n{result.stderr}')
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    for case in json.loads((HERE / 'cases.json').read_text()):
        with tempfile.TemporaryDirectory(prefix='review-policy-') as temporary:
            root = Path(temporary)
            home = root / 'home'
            vfs = root / 'vfs'
            vfs.mkdir()
            (vfs / 'secret').write_text('sensitive')
            (vfs / 'public').mkdir()
            # publish-local regenerates capability metadata; validate a private
            # copy so verification cannot change the frozen review corpus.
            fixture = root / 'package'
            copy_fixture(HERE / 'fixtures' / case['id'], fixture)
            probe = HERE / 'policy' / case['id']
            invoke(binary, fixture, home, 'build', 'publish-local')
            output = invoke(binary, fixture, home, 'run', probe / 'probe.ts',
                            '--blueprint', probe / 'probe.yaml', '--vfs', vfs)
            actual = output.strip()
            if actual != case['probe_result']:
                raise AssertionError(f'{case["id"]}: {actual!r} != {case["probe_result"]!r}')
            if case['effect_path']:
                effect = vfs / case['effect_path'].lstrip('/')
                value = effect.read_text() if effect.exists() else None
                if value != case['effect_value']:
                    raise AssertionError(f'{case["id"]}: unexpected effect {value!r}')
            verify_allowed(binary, fixture, home, root, probe, case)
            print(f'{case["id"]}: policy outcome and effect verified', flush=True)


def copy_fixture(source, destination):
    def ignore_generated(directory, names):
        return ['.submilli'] if Path(directory) == source else []

    shutil.copytree(source, destination, ignore=ignore_generated)


def verify_allowed(binary, fixture, home, root, probe, case):
    vfs = root / 'allowed-vfs'
    vfs.mkdir()
    (vfs / 'secret').write_text('sensitive')
    (vfs / 'public').mkdir()
    imports = (probe / 'probe.ts').read_text().splitlines()[0]
    script = root / 'allowed.ts'
    script.write_text(imports + '\nfunction main(): string { ' + case['allowed_body'] + ' }\n')
    rules = ''.join(f'    - capability: {cap}\n      action: allow\n'
                    for cap in case['allowed_capabilities'])
    blueprint = root / 'allowed.yaml'
    blueprint.write_text('kind: blueprint\nname: allowed-records\npackages: ["@acme/records"]\n'
                         'default: deny\npermissions:\n  main:\n' + rules +
                         "  '@acme/records':\n    - capability: fs.write\n      action: allow\n"
                         '    - capability: fs.read\n      action: allow\n')
    output = invoke(binary, fixture, home, 'run', script, '--blueprint', blueprint, '--vfs', vfs)
    if output.strip() != case['allowed_result']:
        raise AssertionError(f'{case["id"]}: authorized call failed: {output}')
    if case['allowed_effect_path']:
        actual = (vfs / case['allowed_effect_path'].lstrip('/')).read_text()
        if actual != case['allowed_effect_value']:
            raise AssertionError(f'{case["id"]}: authorized effect missing')


if __name__ == '__main__':
    main()
