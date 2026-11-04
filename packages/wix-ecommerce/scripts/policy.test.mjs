// Runs the policy fixtures under the real policy engine, through `submilli run --blueprint`,
// with an isolated SUBMILLI_HOME so nothing touches the machine's store or secrets. Every
// secret here is a fake, and tests/fake-host talks to a MOCK Wix on 127.0.0.1.
//
//   SUBMILLI_BIN=submilli node --test packages/wix-ecommerce/scripts/policy.test.mjs
import assert from 'node:assert/strict';
import { execFile, spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const SUBMILLI = process.env.SUBMILLI_BIN ?? 'submilli';
const PACKAGE = fileURLToPath(new URL('..', import.meta.url));
const SITE = '11111111-2222-4333-8444-555555555555';
const SMALL = 'aaaaaaaa-0000-4000-8000-000000000001';
const LARGE = 'aaaaaaaa-0000-4000-8000-000000000002';
const PAYMENT = 'bbbbbbbb-0000-4000-8000-000000000001';
const run = promisify(execFile);

let work;
let home;
let server;
const writes = [];

function submilli(args, cwd, input) {
    const result = spawnSync(SUBMILLI, args, { cwd, input, encoding: 'utf8', env: { ...process.env, SUBMILLI_HOME: home } });
    assert.equal(result.status, 0, `${args.join(' ')}\n${result.stdout}\n${result.stderr}`);
    return result.stdout;
}

// MOCK Wix: one order of 300 and one of 900, each with one payment of its full total.
function fakeWix(request, response) {
    const chunks = [];
    request.on('data', (chunk) => chunks.push(chunk));
    request.on('end', () => {
        const orderTotal = (id) => (id === SMALL ? '300.00' : '900.00');
        let body = {};
        const path = request.url;
        if (request.method === 'GET' && path.startsWith('/ecom/v1/orders/')) {
            const id = path.split('/').at(-1);
            body = { order: { id, status: 'APPROVED', paymentStatus: 'PAID', currency: 'USD', priceSummary: { total: { amount: orderTotal(id) } } } };
        } else if (request.method === 'GET' && path.startsWith('/ecom/v1/payments/orders/')) {
            const id = path.split('/').at(-1);
            body = { orderTransactions: { payments: [{ id: PAYMENT, amount: { amount: orderTotal(id) } }] } };
        } else {
            writes.push({ path, body: JSON.parse(Buffer.concat(chunks).toString() || '{}') });
            body = path.endsWith('refund-payments') ? { refund: { id: 'fake', transactions: [{ refundStatus: 'SUCCEEDED' }] } } : { order: { id: SMALL } };
        }
        response.writeHead(200, { 'Content-Type': 'application/json' });
        response.end(JSON.stringify(body));
    });
}

before(async () => {
    work = mkdtempSync(join(tmpdir(), 'wix-policy-'));
    home = join(work, 'home');
    server = createServer(fakeWix);
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const port = server.address().port;

    // The real package, then a copy that talks to the fake, each from its own manifest.
    for (const [name, base] of [['real', 'https://www.wixapis.com/'], ['fake', `http://127.0.0.1:${port}/`]]) {
        cpSync(join(PACKAGE, 'src'), join(work, name, 'packages/wix-ecommerce/src'), { recursive: true });
        cpSync(join(PACKAGE, 'docs'), join(work, name, 'packages/wix-ecommerce/docs'), { recursive: true });
        const lib = join(work, name, 'packages/wix-ecommerce/src/lib.ts');
        writeFileSync(lib, readFileSync(lib, 'utf8').replace('const API = "https://www.wixapis.com/";', `const API = "${base}";`));
        writeFileSync(join(work, name, 'submilli.toml'), [
            '[[package]]', 'name = "@submilli/wix-ecommerce"', 'version = "0.1.0"', 'description = "Fixture copy for policy tests."',
            'keywords = ["test"]', 'path = "packages/wix-ecommerce"', '',
        ].join('\n'));
    }
    submilli(['secret', 'put', 'policy-site-id'], work, SITE);
    submilli(['secret', 'put', 'policy-api-key'], work, 'fake-key-for-the-mock');
});

after(() => {
    server?.close();
    if (work) rmSync(work, { recursive: true, force: true });
});

async function runFixture(fixture) {
    const script = join(PACKAGE, 'tests', `${fixture}.ts`);
    const blueprint = join(PACKAGE, 'tests', `${fixture}.yaml`);
    const { stdout } = await run(SUBMILLI, ['run', script, '--blueprint', blueprint], { cwd: work, env: { ...process.env, SUBMILLI_HOME: home } });
    return stdout;
}

test('the site and argument rules hold under the real engine, with no key and no network', async () => {
    submilli(['build', 'publish-local', '-p', '@submilli/wix-ecommerce'], join(work, 'real'));
    assert.match(await runFixture('policy/site-scope'), /ok/);
});

test('control: rules pinned to another site deny every call', async () => {
    assert.match(await runFixture('policy/wrong-site'), /ok/);
});

test('MOCK Wix: refund and cancel caps hold on values resolved from the service, and denied writes send nothing', async () => {
    submilli(['build', 'publish-local', '-p', '@submilli/wix-ecommerce'], join(work, 'fake'));
    assert.match(await runFixture('fake-host/money'), /ok/);
    assert.deepEqual(writes.map((write) => write.path), ['/ecom/v1/order-billing/refund-payments', `/ecom/v1/orders/${SMALL}/cancel`]);
    assert.equal(writes[0].body.paymentRefunds[0].amount.amount, '200');
});
