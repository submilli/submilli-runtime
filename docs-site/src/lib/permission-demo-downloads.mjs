import { readFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

export const permissionDemoPath = '/docs/examples/permission-demo/';

// Reuse the tested Quickstart Package. Readers compile with their runtime.
export async function permissionDemoDownloads() {
  const scratch = await mkdtemp(join(tmpdir(), 'submilli-demo-download-'));
  try {
    const archive = join(scratch, 'billing-source.zip');
    await promisify(execFile)('zip', ['-q', archive,
      'submilli.toml', 'package/src/lib.ts', 'package/docs/readme.md'],
      { cwd: fileURLToPath(new URL('../../../examples/quickstart/', import.meta.url)) });
    return new Map([
      [permissionDemoPath + 'billing-source.zip', await readFile(archive)],
      [permissionDemoPath + 'basic-blueprint.yaml', await readFile(new URL('../../../examples/harnesses/permission-demo/basic-blueprint.yaml', import.meta.url))],
      [permissionDemoPath + 'blueprint.yaml', await readFile(new URL('../../../examples/harnesses/permission-demo/blueprint.yaml', import.meta.url))],
    ]);
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}
