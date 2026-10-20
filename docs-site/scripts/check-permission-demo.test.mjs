import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtemp, writeFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';

for (const failure of ['missing', 'not-executable']) {
  test(`permission demo removes scratch files when server is ${failure}`, async () => {
    const root = await mkdtemp(join(tmpdir(), 'permission-demo-failure-'));
    try {
      const server = join(root, 'server');
      if (failure === 'not-executable') await writeFile(server, '#!/bin/sh\nexit 1\n', { mode: 0o600 });
      await assert.rejects(promisify(execFile)(process.execPath,
        [fileURLToPath(new URL('./check-permission-demo.mjs', import.meta.url))], {
          env: { ...process.env, TMPDIR: root, SUBMILLI_SERVER: server }, timeout: 30000,
        }), error => {
          assert.match(error.stderr, failure === 'missing' ? /ENOENT/ : /EACCES/);
          return true;
        });
      assert.deepEqual(await readdir(root), failure === 'missing' ? [] : ['server']);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
}
