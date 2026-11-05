// Runs the policy fixtures under the real policy engine, through `submilli run --blueprint`,
// with an isolated SUBMILLI_HOME so nothing touches the machine's store or secrets. Every
// secret here is a fake, and tests/fake-host talks to a MOCK Shopify on 127.0.0.1.
//
//   SUBMILLI_BIN=submilli node --test packages/shopify-admin/scripts/policy.test.mjs
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
const SHOP = 'policy-test-shop';
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

// MOCK Shopify. Order 1 is 300 USD with a refund of 150. Order 2 is 900 USD with a refund of 800.
// Order 3 is in EUR. Order 401 answers as a bad token does.
const totals = { 1: ['300.00', 'USD', '150.00'], 2: ['900.00', 'USD', '800.00'], 3: ['300.00', 'EUR', '150.00'] };

function money(amount, currencyCode) {
    return { presentmentMoney: { amount, currencyCode } };
}

function answer(query, variables) {
    const number = String(variables?.id ?? variables?.orderId ?? '').split('/').at(-1);
    const [total, currency, refund] = totals[number] ?? ['1.00', 'USD', '1.00'];
    if (query.startsWith('query OrderFacts')) return { order: { id: variables.id, cancelledAt: null, currentTotalPriceSet: money(total, currency) } };
    if (query.startsWith('query SuggestRefund')) {
        return {
            order: {
                id: variables.id,
                suggestedRefund: {
                    amountSet: money(refund, currency),
                    maximumRefundableSet: money(total, currency),
                    suggestedTransactions: [{ gateway: 'manual', amountSet: money(refund, currency), parentTransaction: { id: 'gid://shopify/OrderTransaction/5' } }],
                },
            },
        };
    }
    if (query.startsWith('query FulfillmentOwner')) return { fulfillment: { id: variables.id, order: { id: `gid://shopify/Order/${number}` } } };
    if (query.startsWith('query AvailableAt')) return { inventoryItem: { id: variables.id, inventoryLevel: { quantities: [{ name: 'available', quantity: 10 }] } } };
    if (query.startsWith('mutation RefundCreate')) return { refundCreate: { refund: { id: 'gid://shopify/Refund/1' }, userErrors: [] } };
    if (query.startsWith('mutation OrderCancel')) return { orderCancel: { job: { id: 'gid://shopify/Job/1', done: false }, orderCancelUserErrors: [] } };
    if (query.startsWith('mutation FulfillmentTrackingUpdate')) return { fulfillmentTrackingInfoUpdate: { fulfillment: { id: variables.fulfillmentId, status: 'SUCCESS' }, userErrors: [] } };
    if (query.startsWith('mutation InventorySet')) return { inventorySetQuantities: { inventoryAdjustmentGroup: { id: 'gid://shopify/InventoryAdjustmentGroup/1' }, userErrors: [] } };
    if (query.startsWith('mutation InventoryAdjust')) return { inventoryAdjustQuantities: { inventoryAdjustmentGroup: { id: 'gid://shopify/InventoryAdjustmentGroup/2' }, userErrors: [] } };
    throw new Error(`no fake answer for ${query.slice(0, 40)}`);
}

function fakeShopify(request, response) {
    const chunks = [];
    request.on('data', (chunk) => chunks.push(chunk));
    request.on('end', () => {
        const { query, variables } = JSON.parse(Buffer.concat(chunks).toString());
        if (variables?.id === 'gid://shopify/Order/401') {
            response.writeHead(401, { 'Content-Type': 'application/json' });
            response.end(JSON.stringify({ errors: '[API] Invalid API key or access token' }));
            return;
        }
        if (query.startsWith('mutation')) writes.push({ query, variables });
        response.writeHead(200, { 'Content-Type': 'application/json' });
        response.end(JSON.stringify({ data: answer(query, variables) }));
    });
}

before(async () => {
    work = mkdtempSync(join(tmpdir(), 'shopify-policy-'));
    home = join(work, 'home');
    server = createServer(fakeShopify);
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const port = server.address().port;

    // The real package, then a copy that talks to the fake, each from its own manifest.
    for (const [name, endpoint] of [['real', null], ['fake', `http://127.0.0.1:${port}/graphql.json`]]) {
        cpSync(join(PACKAGE, 'src'), join(work, name, 'packages/shopify-admin/src'), { recursive: true });
        cpSync(join(PACKAGE, 'docs'), join(work, name, 'packages/shopify-admin/docs'), { recursive: true });
        if (endpoint) {
            const lib = join(work, name, 'packages/shopify-admin/src/lib.ts');
            const source = readFileSync(lib, 'utf8');
            const url = '"https://" + shop + GRAPHQL_PATH';
            assert.ok(source.includes(url), 'the endpoint expression moved');
            writeFileSync(lib, source.replace(url, `"${endpoint}"`));
        }
        writeFileSync(join(work, name, 'submilli.toml'), [
            '[[package]]', 'name = "@submilli/shopify-admin"', 'version = "0.1.0"', 'description = "Fixture copy for policy tests."',
            'keywords = ["test"]', 'path = "packages/shopify-admin"', '',
        ].join('\n'));
    }
    submilli(['secret', 'put', 'policy-shop'], work, SHOP);
    submilli(['secret', 'put', 'policy-access-token'], work, 'fake-token-for-the-mock');
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

test('the shop and argument rules hold under the real engine, with no token and no network', async () => {
    submilli(['build', 'publish-local', '-p', '@submilli/shopify-admin'], join(work, 'real'));
    assert.match(await runFixture('policy/shop-scope'), /ok/);
});

test('control: rules pinned to another shop deny every call', async () => {
    assert.match(await runFixture('policy/wrong-shop'), /ok/);
});

test('MOCK Shopify: caps hold on values resolved from the service, and denied writes send nothing', async () => {
    submilli(['build', 'publish-local', '-p', '@submilli/shopify-admin'], join(work, 'fake'));
    assert.match(await runFixture('fake-host/money'), /ok/);
    const names = writes.map((write) => write.query.split(/[ (]/)[1]);
    assert.deepEqual(names, ['RefundCreate', 'OrderCancel', 'FulfillmentTrackingUpdate', 'InventorySet', 'InventoryAdjust']);
    assert.equal(writes[0].variables.input.transactions[0].amount, '150.00');
    assert.equal(writes[0].variables.idempotencyKey, '3f5b6ebf-143c-4da5-8d0f-fb8553bfd85d');
    assert.equal(writes[3].variables.input.quantities[0].changeFromQuantity, 10);
    // The real runtime keeps an explicit null in a request body. Shopify refuses a missing field.
    assert.ok(Object.hasOwn(writes[4].variables.input.changes[0], 'changeFromQuantity'));
    assert.equal(writes[4].variables.input.changes[0].changeFromQuantity, null);
});
