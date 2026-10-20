import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomBytes } from 'node:crypto';
import { createServer } from 'node:net';
import { permissionDemoDownloads, permissionDemoPath } from '../src/lib/permission-demo-downloads.mjs';

const cli = process.env.SUBMILLI || 'submilli';
const serverBin = process.env.SUBMILLI_SERVER || 'submilli-server';
const work = await mkdtemp(join(tmpdir(), 'submilli-permission-demo-'));
const reservation = createServer();
await new Promise(resolve => reservation.listen(0, '127.0.0.1', resolve));
const port = reservation.address().port;
await new Promise(resolve => reservation.close(resolve));
const base = `http://127.0.0.1:${port}`;
const env = { ...process.env, SUBMILLI_TELEMETRY: '0', SUBMILLI_HOME: join(work, 'home'), SUBMILLI_SERVER_URL: base,
  SUBMILLI_SERVER_TOKEN: randomBytes(32).toString('hex') };
for (const name of ['SUBMILLI_CONFIG', 'SUBMILLI_ALLOW_UNAUTHENTICATED', 'SUBMILLI_SERVER_TOKEN_FILE']) delete env[name];
const run = (bin, args) => promisify(execFile)(bin, args, { cwd: work, env });
let server;
try {
  const downloads = await permissionDemoDownloads();
  if (process.env.SUBMILLI_DEMO_DOWNLOAD_ORIGIN) {
    for (const path of downloads.keys()) {
      const response = await fetch(new URL(path, process.env.SUBMILLI_DEMO_DOWNLOAD_ORIGIN), { signal: AbortSignal.timeout(10000) });
      assert.equal(response.status, 200);
      assert.match(response.headers.get('content-type') || '', path.endsWith('.yaml') ? /yaml/ : /application\/zip/);
      assert.equal(response.headers.get('content-encoding'), null);
      downloads.set(path, Buffer.from(await response.arrayBuffer()));
    }
  }
  for (const [path, bytes] of downloads) await writeFile(join(work, path.slice(permissionDemoPath.length)), bytes);
  await run('unzip', ['-q', 'billing-source.zip']);
  assert.equal(await readFile(join(work, 'package/src/lib.ts'), 'utf8'),
    await readFile(new URL('../../examples/quickstart/package/src/lib.ts', import.meta.url), 'utf8'));
  await run(cli, ['build', 'publish-local', '--deny-warnings']);
  await run(cli, ['blueprint', 'lint', '--deny-warnings', 'blueprint.yaml']);
  server = spawn(serverBin, ['--bind', '127.0.0.1', '--port', String(port)], { cwd: work, env, stdio: 'ignore' });
  let serverError;
  server.on('error', error => { serverError = error; });
  let ready = false;
  for (let i = 0; i < 100; i++) {
    if (serverError) throw serverError;
    if (server.exitCode !== null) throw new Error(`Server exited: ${server.exitCode}`);
    try { if ((await fetch(`${base}/healthz`)).ok) { ready = true; break; } } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  if (serverError) throw serverError;
  assert.ok(ready, 'server became ready');
  await run(cli, ['server', 'blueprint', 'apply', 'blueprint.yaml']);
  async function execute(customerId, blueprint = 'permission-demo') {
    const response = await fetch(`${base}/v1/execute`, {
      method: 'POST', headers: { authorization: `Bearer ${env.SUBMILLI_SERVER_TOKEN}`, 'content-type': 'application/json' },
      body: JSON.stringify({ blueprint, code: `import { listCharges, Charge } from "@acme/billing"; function main(): Charge[] { return listCharges("${customerId}"); }` }),
    });
    assert.equal(response.status, 200);
    return response.json();
  }
  const allowed = await execute('cus_northwind');
  assert.ok(allowed.error == null);
  assert.deepEqual(JSON.parse(allowed.result), [{ customerId: 'cus_northwind', id: 'ch_a1', amount: 4900 }, { customerId: 'cus_northwind', id: 'ch_a2', amount: 1250 }]);
  console.log('ALLOW: Northwind charges ch_a1=4900 and ch_a2=1250.');
  const denied = await execute('cus_initech');
  assert.equal(denied.error.kind, 'permission_denied');
  assert.equal(denied.error.caller, 'main');
  assert.equal(denied.error.capability, 'acme.com/charges.list');
  assert.equal(denied.error.source, 'policy');
  assert.match(denied.error.message, /acme.com\/charges.list/);
  assert.ok(denied.result == null);
  console.log('REJECT: permission_denied for main / acme.com/charges.list / cus_initech.');
  // Prove the fixture contains this customer and only the filter blocks access.
  const policy = await readFile(join(work, 'blueprint.yaml'), 'utf8');
  await writeFile(join(work, 'control.yaml'), policy.replace('name: permission-demo', 'name: permission-demo-control').replace('    filter: customerId == "cus_northwind"\n', ''));
  await run(cli, ['server', 'blueprint', 'apply', 'control.yaml']);
  const control = await execute('cus_initech', 'permission-demo-control');
  assert.ok(control.error == null);
  assert.deepEqual(JSON.parse(control.result), [{ customerId: 'cus_initech', id: 'ch_a3', amount: 39900 }]);
  console.log('PASS: downloaded Package and Blueprint enforce the customer filter without a model or external API.');
} finally {
  if (server?.pid && server.exitCode === null && server.signalCode === null) {
    server.kill('SIGTERM');
    await new Promise(resolve => server.once('exit', resolve));
  }
  await rm(work, { recursive: true, force: true });
}
