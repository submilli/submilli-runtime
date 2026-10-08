#!/usr/bin/env python3
"""Exercise denial contracts through the CLI without changing fixture sources."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
PACKAGE = '@acme/denial-validation'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    binary = parser.parse_args().binary.resolve()
    unsafe_source = (HERE / 'fixtures/unsafe/src/lib.ts').read_bytes()
    if unsafe_source != (HERE / 'fixtures/optional/src/lib.ts').read_bytes():
        raise AssertionError('The contract counterexample must have identical source')
    count = 0
    schemas = {}
    for fixture in sorted((HERE / 'fixtures').iterdir()):
        with tempfile.TemporaryDirectory(prefix='denial-validation-') as temporary:
            root = Path(temporary)
            package = root / 'package'
            shutil.copytree(fixture, package, ignore=shutil.ignore_patterns('.submilli'))
            invoke(binary, package, root, 'build', 'publish-local')
            schemas[fixture.name] = (package / 'capabilities.yaml').read_bytes()
            for label, caller_caps, host_write, outcome, effects in scenarios(fixture.name):
                verify(binary, package, root, label, caller_caps, host_write, outcome, effects)
                count += 1
                print(f'{fixture.name}/{label}: verified', flush=True)
    if schemas['unsafe'] != schemas['optional']:
        raise AssertionError('The contract counterexample must have identical capability schemas')
    print(f'{count} policy executions passed')


def scenarios(name):
    denied = {
        'unsafe': ('handled', {'record': 'record'}),
        'optional': ('handled', {'record': 'record'}),
        'rethrow': ('denied:acme.write', {}),
        'return': ('handled', {}),
        'fallback': ('denied:acme.fallback', {}),
        'logging': ('denied:acme.write', {'audit': 'denied'}),
    }
    outcome, effects = denied[name]
    yield 'denied', [], True, outcome, effects
    yield 'allowed', ['acme.write'], True, 'allowed', {'record': 'record'}
    yield 'host-denied', ['acme.write'], False, 'denied:fs.write', {}
    if name in ('unsafe', 'optional', 'logging'):
        yield 'both-denied', [], False, 'denied:fs.write', {}
    if name == 'fallback':
        yield 'fallback-allowed', ['acme.fallback'], True, 'handled', {'record': 'record'}
        yield 'fallback-host-denied', ['acme.fallback'], False, 'denied:fs.write', {}


def verify(binary, package, root, label, caller_caps, host_write, outcome, effects):
    run = root / label
    run.mkdir()
    vfs = run / 'vfs'
    vfs.mkdir()
    probe = run / 'probe.ts'
    probe.write_text('import { run } from "' + PACKAGE + '";\n'
                     'function main(): string {\n'
                     '    try { return run(); }\n'
                     '    catch (error: PermissionDeniedError) { return "denied:" + error.capability; }\n'
                     '}\n')
    caller = ''.join(f'    - capability: {cap}\n      action: allow\n' for cap in caller_caps)
    policy = run / 'policy.yaml'
    policy.write_text('kind: blueprint\nname: denial-validation\n'
                      f'packages: ["{PACKAGE}"]\ndefault: deny\npermissions:\n'
                      + ('  main:\n' + caller if caller else '  main: []\n')
                      + (f"  '{PACKAGE}':\n    - capability: fs.write\n      action: allow\n"
                         if host_write else f"  '{PACKAGE}': []\n"))
    actual = invoke(binary, package, root, 'run', probe, '--blueprint', policy, '--vfs', vfs).strip()
    if actual != outcome:
        raise AssertionError(f'{package}/{label}: expected {outcome!r}, got {actual!r}')
    actual_effects = {str(p.relative_to(vfs)): p.read_text() for p in vfs.rglob('*') if p.is_file()}
    if actual_effects != effects:
        raise AssertionError(f'{package}/{label}: expected effects {effects!r}, got {actual_effects!r}')


def invoke(binary, package, root, *args):
    result = subprocess.run([str(binary), *map(str, args)], cwd=package,
                            env=dict(os.environ, SUBMILLI_HOME=str(root / 'home'), SUBMILLI_TELEMETRY='0'),
                            text=True, capture_output=True, timeout=120)
    if result.returncode:
        raise RuntimeError(f'{args}:\n{result.stdout}\n{result.stderr}')
    return result.stdout


if __name__ == '__main__':
    main()
