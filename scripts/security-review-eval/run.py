#!/usr/bin/env python3
"""Run paired security reviews through the private Rust evaluation entry point."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import signal
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
TEST = 'commands::build::security_review::evaluation::run_evaluation'


def schedule(case_ids, repeats, seed):
    if repeats < 1 or repeats > 20:
        raise ValueError('repeats must be between 1 and 20')
    trials = [dict(id=f'{case}-{repeat}-{arm}', fixture=case, arm=arm)
              for case in sorted(case_ids) for repeat in range(1, repeats + 1)
              for arm in ('source-only', 'source-plus-map')]
    random.Random(seed).shuffle(trials)
    return trials


def test_binary():
    command = ['cargo', 'test', '-p', 'submilli', '--bin', 'submilli',
               '--no-run', '--message-format=json']
    result = subprocess.run(command, cwd=REPO, env=check_environment(),
                            text=True, stdout=subprocess.PIPE, check=True)
    artifacts = [json.loads(line) for line in result.stdout.splitlines()]
    return next(a['executable'] for a in artifacts
                if a.get('reason') == 'compiler-artifact'
                and a.get('profile', {}).get('test') and a.get('executable'))


def check_environment():
    return dict(os.environ, SUBMILLI_SKIP_HTTP_TESTS='1', SUBMILLI_FULL_TEST='0',
                SUBMILLI_TEST_NIGHTLY_ONLY='0', SUBMILLI_TELEMETRY='0')


def fingerprint():
    # Ground truth and probes are frozen with the corpus but never enter prompts.
    paths = [HERE / 'cases.json'] + sorted((HERE / 'fixtures').rglob('*')) + sorted((HERE / 'policy').rglob('*'))
    return {str(p.relative_to(HERE)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in paths if p.is_file() and is_corpus_path(p.relative_to(HERE))}


def is_corpus_path(path):
    # publish-local may create editor stubs at the package root. Snapshot
    # collection excludes these; provenance must not require untracked caches.
    parts = Path(path).parts
    return not (len(parts) >= 3 and parts[0] == 'fixtures' and parts[2] == '.submilli')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--seed', type=int, default=1428)
    parser.add_argument('--case', action='append')
    parser.add_argument('--prepare-only', action='store_true')
    parser.add_argument('--test-binary', type=Path)
    args = parser.parse_args()
    cases = json.loads((HERE / 'cases.json').read_text())
    known = {case['id'] for case in cases}
    selected = set(args.case or known)
    if selected - known:
        parser.error('unknown case ID')
    if args.output.exists():
        parser.error('output must be a new directory; old attempts are never overwritten')
    config = dict(fixtures={case: str(HERE / 'fixtures' / case) for case in sorted(selected)},
                  trials=schedule(selected, args.repeats, args.seed),
                  output=str(args.output.resolve()), prepare_only=args.prepare_only)
    executable = str(args.test_binary.resolve()) if args.test_binary else test_binary()
    with tempfile.TemporaryDirectory(prefix='review-eval-config-') as temporary:
        path = Path(temporary) / 'config.json'
        path.write_text(json.dumps(config))
        before = fingerprint()
        command = [executable, TEST, '--ignored', '--exact', '--nocapture']
        metadata = dict(schema_version=1, corpus_format=2, seed=args.seed, repeats=args.repeats,
                        corpus=before, corpus_unchanged=True, command=command, exit_code=None,
                        revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=REPO, text=True).strip())
        args.output.mkdir()
        (args.output / 'suite.json').write_text(json.dumps(metadata, indent=2) + '\n')
        try:
            metadata['exit_code'] = execute(command, dict(check_environment(),
                                            SUBMILLI_REVIEW_EVAL_CONFIG=str(path)))
        finally:
            metadata['corpus_unchanged'] = before == fingerprint()
            (args.output / 'suite.json').write_text(json.dumps(metadata, indent=2) + '\n')
        if before != fingerprint():
            raise RuntimeError('corpus changed during evaluation; invalidate this run')
        raise SystemExit(metadata['exit_code'])


def execute(command, environment):
    def interrupt(signum, frame):
        raise KeyboardInterrupt

    previous = signal.signal(signal.SIGTERM, interrupt)
    try:
        with subprocess.Popen(command, cwd=REPO, env=environment) as process:
            try:
                return process.wait()
            except KeyboardInterrupt:
                # Let the Rust adapter terminate/reap its isolated agent group
                # and flush current-trial artifacts before stopping the suite.
                process.terminate()
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                return 130
    finally:
        signal.signal(signal.SIGTERM, previous)


if __name__ == '__main__':
    main()
