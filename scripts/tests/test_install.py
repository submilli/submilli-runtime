"""Exercise the actual installer against local release fixtures, without network access."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

INSTALLER = Path(__file__).resolve().parents[2] / 'install.sh'
BINARIES = ['submilli', 'submilli-server']


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.destination = self.root / 'install with spaces'
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
                        FIXTURES=str(self.root), TEST_OS='Linux', TEST_ARCH='x86_64')
        self.command('uname', '#!/bin/sh\nif [ "$1" = -s ]; then echo "$TEST_OS"; else echo "$TEST_ARCH"; fi\n')
        self.command('curl', '''#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(a for a in args if a.startswith('https://'))
if '-w' in args:
    print('https://github.com/submilli/submilli-runtime/releases/tag/v9.8.7', end='')
else:
    shutil.copyfile(pathlib.Path(os.environ['FIXTURES']) / url.rsplit('/', 1)[1], args[args.index('-o') + 1])
''')
        self.seed('x86_64-unknown-linux-musl')

    def command(self, name, content):
        p = self.bin / name
        p.write_text(content)
        p.chmod(0o755)

    def seed(self, target):
        sums = ''
        for binary in BINARIES:
            asset = binary + '-' + target
            content = ('#!/bin/sh\necho "' + binary + ' test fixture"\n').encode()
            (self.root / asset).write_bytes(content)
            sums += hashlib.sha256(content).hexdigest() + '  ' + asset + '\n'
        (self.root / 'SHA256SUMS').write_text(sums)

    def run_installer(self, *args):
        return subprocess.run(['sh', str(INSTALLER), '--install-dir', str(self.destination), *args],
                              env=self.env, text=True, capture_output=True)

    def test_latest_and_explicit_version_install_and_replace(self):
        for args in [(), ('--version', 'v9.8.7')]:
            result = self.run_installer(*args)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('v9.8.7', result.stdout)
            for binary in BINARIES:
                self.assertTrue(os.access(self.destination / binary, os.X_OK))
                self.assertIn(binary + ' test fixture', (self.destination / binary).read_text())
            self.assertEqual(sorted(p.name for p in self.destination.iterdir()), BINARIES)

    def test_mac_architectures(self):
        for arch, target in [('x86_64', 'x86_64-apple-darwin'), ('arm64', 'aarch64-apple-darwin')]:
            self.env.update(TEST_OS='Darwin', TEST_ARCH=arch)
            self.seed(target)
            result = self.run_installer()
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_bad_checksum_of_either_executable_preserves_the_existing_install(self):
        self.destination.mkdir()
        for binary in BINARIES:
            (self.destination / binary).write_text('existing installation')
        for corrupted in BINARIES:
            self.seed('x86_64-unknown-linux-musl')
            (self.root / (corrupted + '-x86_64-unknown-linux-musl')).write_text('corrupted')
            result = self.run_installer()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Checksum mismatch', result.stderr)
            for binary in BINARIES:
                self.assertEqual((self.destination / binary).read_text(), 'existing installation')

    def test_missing_or_duplicate_checksum_refused(self):
        p = self.root / 'SHA256SUMS'
        original = p.read_text()
        server_only = ''.join(line + '\n' for line in original.splitlines() if 'submilli-server-' in line)
        for content in ['', original * 2, server_only]:
            p.write_text(content)
            result = self.run_installer()
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.destination.exists())

    def test_failed_download_does_not_install(self):
        (self.root / 'submilli-server-x86_64-unknown-linux-musl').unlink()
        self.assertNotEqual(self.run_installer().returncode, 0)
        self.assertFalse(self.destination.exists())

    def test_unsupported_architecture_and_invalid_arguments(self):
        self.env['TEST_ARCH'] = 'aarch64'
        self.assertNotEqual(self.run_installer().returncode, 0)
        self.env['TEST_ARCH'] = 'x86_64'
        for args in [('--version',), ('--version', '../main'), ('--unknown',)]:
            self.assertNotEqual(self.run_installer(*args).returncode, 0)


if __name__ == '__main__':
    unittest.main()
