// MOCK HARNESS. Exercises the actual package operations against a mocked Submilli
// host and a mocked Shopify API, without network access or credentials. Passing
// here means the requests match this file's reading of Shopify's documentation. It
// says nothing about the live API. Native build tests separately verify Submilli
// compatibility; this harness checks the requests each operation sends and what
// `check()` sees.
//
//   node --test packages/shopify-admin/scripts/contract.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';

const SHOP = 'acme-store';
const ENDPOINT = 'https://acme-store.myshopify.com/admin/api/2026-10/graphql.json';
const KEY = '3f5b6ebf-143c-4da5-8d0f-fb8553bfd85d';
const ORDER = 'gid://shopify/Order/1001';
const LINE = 'gid://shopify/LineItem/7';
const FO_A = 'gid://shopify/FulfillmentOrder/1';
const FO_B = 'gid://shopify/FulfillmentOrder/2';
const LOCATION = 'gid://shopify/Location/1';
const ITEM = 'gid://shopify/InventoryItem/1';
const PRODUCT = 'gid://shopify/Product/1';
const VARIANT = 'gid://shopify/ProductVariant/1';

let events = [];
let requests = [];
let checks = [];
let boundSecrets = {};
let denied = new Set();
let respond = () => reply(500, {});

function reset(handler, secrets = { SHOPIFY_ACCESS_TOKEN: 'shpat_test', SHOPIFY_SHOP: SHOP }) {
    events = [];
    requests = [];
    checks = [];
    boundSecrets = secrets;
    denied = new Set();
    respond = handler;
}

function reply(status, body, headers = {}) {
    const text = typeof body === 'string' ? body : JSON.stringify(body);
    return {
        ok: status >= 200 && status < 300,
        status,
        statusText: { 200: 'OK', 401: 'Unauthorized', 429: 'Too Many Requests', 500: 'Internal Server Error' }[status] ?? '',
        headers: new Map(Object.entries(headers)),
        body: text,
        url: '',
        json: () => JSON.parse(text),
    };
}

// `data` answers by the operation name in the query.
function by(answers) {
    return (request) => {
        const name = request.body.query.split(/[ ({]/)[1];
        if (!(name in answers)) throw new Error(`no mock answer for ${name}`);
        const answer = answers[name];
        return reply(200, { data: typeof answer === 'function' ? answer(request) : answer });
    };
}

function send(url, body, headers) {
    const request = { url, body, headers, name: body.query.split(/[ ({]/)[1] };
    requests.push(request);
    events.push({ kind: 'request', name: request.name });
    return respond(request);
}

globalThis.Temporal ??= {
    Instant: {
        from: (text) => {
            const at = new Date(text);
            if (Number.isNaN(at.getTime()) || !text.endsWith('Z')) throw new RangeError('bad instant');
            return { toString: () => at.toISOString().replace('.000Z', 'Z') };
        },
    },
};

globalThis.__shopifyHost = {
    post: send,
    secrets: { get: (name) => boundSecrets[name] },
    check: (capability, context) => {
        checks.push({ capability, context: JSON.parse(JSON.stringify(context)) });
        events.push({ kind: 'check', capability });
        if (denied.has(capability)) {
            const error = new Error('Capability denied: ' + capability);
            error.name = 'PermissionDeniedError';
            throw error;
        }
    },
};

async function source(name) {
    return (await readFile(new URL(`../src/${name}`, import.meta.url), 'utf8'))
        .replace(/^import [\s\S]*?from "[^"]+";\n/gm, '')
        .replace(/^export \{[\s\S]*?\} from "[^"]+";\n/gm, '');
}
const bundle = 'const { post, secrets, check } = globalThis.__shopifyHost;\n' +
    (await source('errors.ts')) + (await source('queries.ts')) + (await source('request.ts')) + (await source('response.ts')) + (await source('lib.ts'));
const shopify = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(bundle)).toString('base64')}`);

function kinds() {
    return events.map((event) => (event.kind === 'check' ? 'check' : event.name));
}

function money(amount, currencyCode = 'USD') {
    return { presentmentMoney: { amount, currencyCode } };
}

const rawOrder = {
    id: ORDER,
    name: '#1001',
    createdAt: '2026-10-01T10:00:00Z',
    cancelledAt: null,
    displayFinancialStatus: 'PAID',
    displayFulfillmentStatus: 'UNFULFILLED',
    presentmentCurrencyCode: 'USD',
    email: 'buyer@example.com',
    currentTotalPriceSet: money('150.50'),
    note: 'Fragile',
    tags: ['VIP'],
};
const orderFacts = { order: { id: ORDER, cancelledAt: null, note: 'Fragile', tags: ['VIP'], currentTotalPriceSet: money('150.50') } };
const suggestion = {
    order: {
        id: ORDER,
        suggestedRefund: {
            amountSet: money('25.50'),
            maximumRefundableSet: money('150.50'),
            suggestedTransactions: [{ gateway: 'shopify_payments', amountSet: money('25.50'), parentTransaction: { id: 'gid://shopify/OrderTransaction/5' } }],
        },
    },
};
const refundDone = { refundCreate: { refund: { id: 'gid://shopify/Refund/1', totalRefundedSet: money('25.50') }, userErrors: [] } };
const openOrders = {
    order: {
        id: ORDER,
        fulfillmentOrders: {
            nodes: [
                { id: FO_A, status: 'OPEN', assignedLocation: { location: { id: LOCATION } } },
                { id: FO_B, status: 'OPEN', assignedLocation: { location: { id: 'gid://shopify/Location/2' } } },
            ],
        },
    },
};
const fulfilled = { fulfillment: { id: 'gid://shopify/Fulfillment/1', status: 'SUCCESS', trackingInfo: [{ company: 'UPS', number: '1Z', url: null }] }, userErrors: [] };

test('every request goes to the store\'s endpoint with the token and the query', () => {
    reset(by({ GetShop: { shop: { name: 'Acme', email: 'a@example.com', currencyCode: 'USD', ianaTimezone: 'UTC', myshopifyDomain: 'acme-store.myshopify.com', plan: { publicDisplayName: 'Basic' } } } }));
    const shop = shopify.getShop();
    assert.equal(requests[0].url, ENDPOINT);
    assert.equal(requests[0].headers.get('X-Shopify-Access-Token'), 'shpat_test');
    assert.deepEqual(checks, [{ capability: 'shopify.com/shop.get', context: { shop: SHOP } }]);
    assert.equal(shop.plan, 'Basic');
    assert.equal(shop.domain, 'acme-store.myshopify.com');
});

test('a store that is not a single label is refused before any check or request', () => {
    for (const bad of ['evil.example/#', 'acme.myshopify.com', 'acme.evil.example', 'a b', '-acme', '']) {
        reset(() => assert.fail('no request'), { SHOPIFY_ACCESS_TOKEN: 'shpat_test', SHOPIFY_SHOP: bad });
        assert.throws(() => shopify.getShop(), { name: 'ShopifyError', code: 'invalid_shop' });
        assert.deepEqual(events, []);
    }
});

test('a missing store stops before the check, and a missing token before the request', () => {
    reset(() => assert.fail('no request'), {});
    assert.throws(() => shopify.getShop(), { code: 'missing_shop' });
    assert.deepEqual(events, []);
    reset(() => assert.fail('no request'), { SHOPIFY_SHOP: SHOP });
    assert.throws(() => shopify.getShop(), { code: 'missing_access_token' });
    assert.deepEqual(kinds(), ['check']);
});

test('a denied check stops every operation before its write', () => {
    reset(by({ OrderFacts: orderFacts, SuggestRefund: suggestion }));
    denied.add('shopify.com/refunds.create');
    assert.throws(() => shopify.refundOrder(ORDER, { lineItems: [{ lineItemId: LINE, quantity: 1 }], idempotencyKey: KEY }), { name: 'PermissionDeniedError' });
    assert.deepEqual(kinds(), ['SuggestRefund', 'check']);
    reset(by({ OrderFacts: orderFacts }));
    denied.add('shopify.com/orders.cancel');
    assert.throws(() => shopify.cancelOrder(ORDER, { reason: 'CUSTOMER', restock: false, refund: false }), { name: 'PermissionDeniedError' });
    assert.deepEqual(kinds(), ['OrderFacts', 'check']);
});

test('searchOrders sends the search and reads the page', () => {
    reset(by({ SearchOrders: { orders: { nodes: [rawOrder], pageInfo: { hasNextPage: true, endCursor: 'n1' } } } }));
    const page = shopify.searchOrders({ query: 'financial_status:paid', first: 25 });
    assert.deepEqual(requests[0].body.variables, { first: 25, query: 'financial_status:paid', sortKey: 'CREATED_AT', reverse: true });
    assert.equal(page.nextCursor, 'n1');
    assert.equal(page.items[0].total, 150.5);
    assert.equal(page.items[0].financialStatus, 'PAID');
    assert.deepEqual(checks[0].context, { shop: SHOP });
});

test('searchOrders passes a cursor, an explicit sort and direction', () => {
    reset(by({ SearchOrders: { orders: { nodes: [], pageInfo: { hasNextPage: false } } } }));
    const page = shopify.searchOrders({ after: 'c', sortKey: 'order_number', reverse: false });
    assert.deepEqual(requests[0].body.variables, { first: 50, after: 'c', sortKey: 'ORDER_NUMBER', reverse: false });
    assert.equal(page.nextCursor, null);
});

test('getOrder takes a number and returns null for a missing order', () => {
    reset(by({ GetOrder: { order: { ...rawOrder, lineItems: { nodes: [{ id: LINE, title: 'Mug', quantity: 2, refundableQuantity: 2, unfulfilledQuantity: 2, variant: { id: VARIANT }, discountedUnitPriceSet: money('75.25') }], pageInfo: { hasNextPage: true } } } } }));
    const order = shopify.getOrder('1001');
    assert.equal(requests[0].body.variables.id, ORDER);
    assert.deepEqual(checks[0].context, { shop: SHOP, orderId: ORDER });
    assert.equal(order.lineItems[0].unitPrice, 75.25);
    assert.equal(order.moreLineItems, true);
    assert.deepEqual(order.tags, ['VIP']);
    reset(by({ GetOrder: { order: null } }));
    assert.equal(shopify.getOrder('1001'), null);
});

test('addOrderNote keeps the note already there', () => {
    reset(by({ OrderFacts: orderFacts, OrderUpdate: (r) => ({ orderUpdate: { order: { id: ORDER, note: r.body.variables.input.note }, userErrors: [] } }) }));
    assert.equal(shopify.addOrderNote(ORDER, 'Call first'), 'Fragile\nCall first');
    assert.deepEqual(kinds(), ['check', 'OrderFacts', 'OrderUpdate']);
    assert.deepEqual(checks[0].context, { shop: SHOP, orderId: ORDER, fields: ['note'] });
    assert.deepEqual(requests[1].body.variables.input, { id: ORDER, note: 'Fragile\nCall first' });
});

test('addOrderTags merges with the tags already there, because orderUpdate overwrites', () => {
    reset(by({ OrderFacts: orderFacts, OrderUpdate: (r) => ({ orderUpdate: { order: { id: ORDER, tags: r.body.variables.input.tags }, userErrors: [] } }) }));
    assert.deepEqual(shopify.addOrderTags(ORDER, ['vip', 'gift']), ['VIP', 'gift']);
    assert.deepEqual(requests[1].body.variables.input, { id: ORDER, tags: ['VIP', 'gift'] });
    assert.deepEqual(checks[0].context.fields, ['tags']);
});

test('cancelOrder resolves the total, checks it, and uses refundMethod, not the deprecated refund argument', () => {
    reset(by({ OrderFacts: orderFacts, OrderCancel: { orderCancel: { job: { id: 'gid://shopify/Job/1', done: false }, orderCancelUserErrors: [] } } }));
    const job = shopify.cancelOrder(ORDER, { reason: 'customer', restock: true, refund: true, notifyCustomer: true, staffNote: 'Asked by phone' });
    assert.deepEqual(kinds(), ['OrderFacts', 'check', 'OrderCancel']);
    assert.deepEqual(checks[0].context, { shop: SHOP, orderId: ORDER, totalAmount: 150.5, currency: 'USD', restock: true, refund: true });
    assert.deepEqual(requests[1].body.variables, {
        orderId: ORDER, reason: 'CUSTOMER', restock: true, refundMethod: { originalPaymentMethodsRefund: true }, notifyCustomer: true, staffNote: 'Asked by phone',
    });
    assert.deepEqual(job, { jobId: 'gid://shopify/Job/1', done: false });
});

test('cancelOrder refuses a canceled order, and a total it cannot read', () => {
    reset(by({ OrderFacts: { order: { ...orderFacts.order, cancelledAt: '2026-10-02T00:00:00Z' } } }));
    assert.throws(() => shopify.cancelOrder(ORDER, { reason: 'STAFF', restock: false, refund: false }), { code: 'already_cancelled' });
    assert.deepEqual(checks, []);
    reset(by({ OrderFacts: { order: { id: ORDER, cancelledAt: null } } }));
    assert.throws(() => shopify.cancelOrder(ORDER, { reason: 'STAFF', restock: false, refund: false }), { code: 'unexpected_response' });
    assert.deepEqual(checks, []);
});

test('cancelOrder surfaces orderCancelUserErrors', () => {
    reset(by({ OrderFacts: orderFacts, OrderCancel: { orderCancel: { orderCancelUserErrors: [{ code: 'INVALID', field: ['orderId'], message: 'no' }] } } }));
    assert.throws(() => shopify.cancelOrder(ORDER, { reason: 'STAFF', restock: false, refund: false }), { code: 'INVALID', field: 'orderId' });
});

test('suggestRefund sends no restock and writes nothing', () => {
    reset(by({ SuggestRefund: suggestion }));
    const result = shopify.suggestRefund(ORDER, { lineItems: [{ lineItemId: '7', quantity: 2 }] });
    assert.deepEqual(requests[0].body.variables, { id: ORDER, refundShipping: false, refundLineItems: [{ lineItemId: LINE, quantity: 2, restockType: 'NO_RESTOCK' }] });
    assert.equal(result.amount, 25.5);
    assert.equal(result.maximumRefundable, 150.5);
    assert.deepEqual(kinds(), ['check', 'SuggestRefund']);
    reset(by({ SuggestRefund: { order: null } }));
    assert.equal(shopify.suggestRefund(ORDER, { shipping: true }), null);
});

test('refundOrder checks the amount Shopify worked out and sends exactly those transactions, with the idempotency key', () => {
    reset(by({ SuggestRefund: suggestion, RefundCreate: refundDone }));
    const refund = shopify.refundOrder('1001', {
        lineItems: [{ lineItemId: '7', quantity: 1 }], shipping: true, restockType: 'RETURN', locationId: '1', note: 'Damaged', notify: true, idempotencyKey: ` ${KEY} `,
    });
    assert.deepEqual(kinds(), ['SuggestRefund', 'check', 'RefundCreate']);
    assert.deepEqual(checks[0].context, { shop: SHOP, orderId: ORDER, amount: 25.5, currency: 'USD', restock: true });
    const line = { lineItemId: LINE, quantity: 1, restockType: 'RETURN', locationId: LOCATION };
    assert.deepEqual(requests[0].body.variables.refundLineItems, [line]);
    assert.equal(requests[0].body.variables.refundShipping, true);
    assert.deepEqual(requests[1].body.variables, {
        idempotencyKey: KEY,
        input: {
            orderId: ORDER,
            currency: 'USD',
            notify: true,
            transactions: [{ orderId: ORDER, kind: 'REFUND', gateway: 'shopify_payments', amount: '25.50', parentId: 'gid://shopify/OrderTransaction/5' }],
            refundLineItems: [line],
            shipping: { fullRefund: true },
            note: 'Damaged',
        },
    });
    assert.ok(requests[1].body.query.includes('refundCreate(input: $input) @idempotent(key: $idempotencyKey)'));
    assert.deepEqual(refund, { id: 'gid://shopify/Refund/1', amount: 25.5, currency: 'USD' });
});

test('refundOrder refuses a missing order and a refund of nothing, before its check', () => {
    reset(by({ SuggestRefund: { order: null } }));
    assert.throws(() => shopify.refundOrder(ORDER, { shipping: true, idempotencyKey: KEY }), { code: 'not_found' });
    reset(by({ SuggestRefund: { order: { id: ORDER, suggestedRefund: { amountSet: money('0.00'), suggestedTransactions: [] } } } }));
    assert.throws(() => shopify.refundOrder(ORDER, { shipping: true, idempotencyKey: KEY }), { code: 'nothing_to_refund' });
    assert.deepEqual(checks, []);
});

test('refundOrder surfaces a userError as a typed error', () => {
    reset(by({ SuggestRefund: suggestion, RefundCreate: { refundCreate: { userErrors: [{ field: ['input', 'transactions'], message: 'too much' }] } } }));
    assert.throws(() => shopify.refundOrder(ORDER, { shipping: true, idempotencyKey: KEY }), { name: 'ShopifyError', code: 'user_error', field: 'input.transactions', status: 200 });
});

test('listFulfillmentOrders maps holds and lines, and null for a missing order', () => {
    reset(by({ ListFulfillmentOrders: { order: { id: ORDER, fulfillmentOrders: { nodes: [{ id: FO_A, status: 'ON_HOLD', assignedLocation: { name: 'Depot', location: { id: LOCATION } }, fulfillmentHolds: [{ id: 'gid://shopify/FulfillmentHold/1', reason: 'OTHER' }], lineItems: { nodes: [{ id: 'gid://shopify/FulfillmentOrderLineItem/1', productTitle: 'Mug', remainingQuantity: 1, totalQuantity: 2 }], pageInfo: { hasNextPage: false } } }] } } } }));
    const [order] = shopify.listFulfillmentOrders(ORDER);
    assert.equal(order.locationName, 'Depot');
    assert.equal(order.holds[0].reason, 'OTHER');
    assert.equal(order.lines[0].remainingQuantity, 1);
    reset(by({ ListFulfillmentOrders: { order: null } }));
    assert.equal(shopify.listFulfillmentOrders(ORDER), null);
});

test('createFulfillment ships a named fulfillment order of this order', () => {
    reset(by({ OpenFulfillmentOrders: openOrders, FulfillmentCreate: { fulfillmentCreate: fulfilled } }));
    const result = shopify.createFulfillment(ORDER, { fulfillmentOrderIds: ['1'], tracking: { company: 'UPS', number: '1Z' }, notifyCustomer: true });
    assert.deepEqual(kinds(), ['OpenFulfillmentOrders', 'check', 'FulfillmentCreate']);
    assert.deepEqual(checks[0].context, { shop: SHOP, orderId: ORDER, notifyCustomer: true });
    assert.deepEqual(requests[1].body.variables.fulfillment, {
        lineItemsByFulfillmentOrder: [{ fulfillmentOrderId: FO_A }], notifyCustomer: true, trackingInfo: { company: 'UPS', number: '1Z' },
    });
    assert.equal(result.tracking[0].number, '1Z');
});

test('createFulfillment refuses a fulfillment order of another order, before its check', () => {
    reset(by({ OpenFulfillmentOrders: openOrders }));
    assert.throws(() => shopify.createFulfillment(ORDER, { fulfillmentOrderIds: ['999'] }), { code: 'fulfillment_order_not_open' });
    assert.deepEqual(checks, []);
});

test('createFulfillment with none named refuses two locations, and a missing order', () => {
    reset(by({ OpenFulfillmentOrders: openOrders }));
    assert.throws(() => shopify.createFulfillment(ORDER), { code: 'several_locations' });
    reset(by({ OpenFulfillmentOrders: { order: null } }));
    assert.throws(() => shopify.createFulfillment(ORDER), { code: 'not_found' });
});

test('updateTracking resolves the order from the fulfillment and checks that one', () => {
    reset(by({ FulfillmentOwner: { fulfillment: { id: 'gid://shopify/Fulfillment/1', order: { id: ORDER } } }, FulfillmentTrackingUpdate: { fulfillmentTrackingInfoUpdate: fulfilled } }));
    shopify.updateTracking('1', { number: '1Z' }, true);
    assert.deepEqual(kinds(), ['FulfillmentOwner', 'check', 'FulfillmentTrackingUpdate']);
    assert.deepEqual(checks[0].context, { shop: SHOP, orderId: ORDER, fulfillmentId: 'gid://shopify/Fulfillment/1' });
    assert.deepEqual(requests[1].body.variables, { fulfillmentId: 'gid://shopify/Fulfillment/1', trackingInfoInput: { number: '1Z' }, notifyCustomer: true });
    reset(by({ FulfillmentOwner: { fulfillment: null } }));
    assert.throws(() => shopify.updateTracking('1', { number: '1Z' }), { code: 'not_found' });
});

test('hold and release send the reason and the hold ids', () => {
    reset(by({ FulfillmentOrderHold: { fulfillmentOrderHold: { fulfillmentHold: { id: 'gid://shopify/FulfillmentHold/1' }, fulfillmentOrder: { id: FO_A, status: 'ON_HOLD' }, userErrors: [] } } }));
    const held = shopify.holdFulfillmentOrder('1', 'incorrect_address', 'Check the unit number');
    assert.deepEqual(requests[0].body.variables, { id: FO_A, fulfillmentHold: { reason: 'INCORRECT_ADDRESS', reasonNotes: 'Check the unit number' } });
    assert.deepEqual(checks[0].context, { shop: SHOP, fulfillmentOrderId: FO_A, reason: 'INCORRECT_ADDRESS' });
    assert.equal(held.holdId, 'gid://shopify/FulfillmentHold/1');
    reset(by({ FulfillmentOrderRelease: { fulfillmentOrderReleaseHold: { fulfillmentOrder: { id: FO_A, status: 'OPEN' }, userErrors: [] } } }));
    assert.equal(shopify.releaseFulfillmentOrderHold(FO_A, ['1']).status, 'OPEN');
    assert.deepEqual(requests[0].body.variables, { id: FO_A, holdIds: ['gid://shopify/FulfillmentHold/1'] });
    reset(by({ FulfillmentOrderRelease: { fulfillmentOrderReleaseHold: { fulfillmentOrder: { id: FO_A, status: 'OPEN' }, userErrors: [] } } }));
    shopify.releaseFulfillmentOrderHold(FO_A);
    assert.deepEqual(requests[0].body.variables, { id: FO_A });
});

test('products: search, get and an update of only the fields set', () => {
    const product = { id: PRODUCT, title: 'Mug', handle: 'mug', status: 'ACTIVE', vendor: 'Acme', productType: 'Cup', tags: [], totalInventory: 5, priceRangeV2: { minVariantPrice: { amount: '9.50', currencyCode: 'USD' } } };
    reset(by({ SearchProducts: { products: { nodes: [product], pageInfo: { hasNextPage: false } } } }));
    assert.equal(shopify.searchProducts({ query: 'mug', first: 10 }).items[0].minPrice, 9.5);
    assert.deepEqual(requests[0].body.variables, { first: 10, query: 'mug' });
    reset(by({ GetProduct: { product: { ...product, descriptionHtml: '<p>Hi</p>', variants: { nodes: [{ id: VARIANT, title: 'Default', price: '9.50', compareAtPrice: null, inventoryQuantity: 5 }], pageInfo: { hasNextPage: false } } } } }));
    assert.equal(shopify.getProduct(PRODUCT).variants[0].price, 9.5);
    reset(by({ ProductUpdate: { productUpdate: { product, userErrors: [] } } }));
    shopify.updateProduct('1', { title: 'New', tags: [], status: 'draft' });
    assert.deepEqual(requests[0].body.variables.product, { id: PRODUCT, title: 'New', status: 'DRAFT', tags: [] });
    assert.deepEqual(checks[0].context.fields, ['status', 'tags', 'title']);
});

test('updateVariantPrices checks the highest price and sends decimal strings', () => {
    reset(by({ VariantPricesUpdate: { productVariantsBulkUpdate: { productVariants: [{ id: VARIANT, price: '12.50' }], userErrors: [] } } }));
    const items = shopify.updateVariantPrices('1', [{ variantId: '1', price: 12.5, compareAtPrice: 20 }, { variantId: '2', price: 3 }]);
    assert.deepEqual(checks[0].context, { shop: SHOP, productId: PRODUCT, variantIds: [VARIANT, 'gid://shopify/ProductVariant/2'], maxPrice: 12.5 });
    assert.deepEqual(requests[0].body.variables.variants, [{ id: VARIANT, price: '12.5', compareAtPrice: '20' }, { id: 'gid://shopify/ProductVariant/2', price: '3' }]);
    assert.equal(items[0].price, 12.5);
});

test('inventory reads: locations and levels by variant', () => {
    reset(by({ ListLocations: { locations: { nodes: [{ id: LOCATION, name: 'Depot', isActive: true, fulfillsOnlineOrders: true }] } } }));
    assert.equal(shopify.listLocations()[0].name, 'Depot');
    reset(by({ InventoryLevels: { product: { id: PRODUCT, variants: { nodes: [{ id: VARIANT, title: 'Default', inventoryItem: { id: ITEM, inventoryLevels: { nodes: [{ location: { id: LOCATION, name: 'Depot' }, quantities: [{ name: 'available', quantity: 3 }, { name: 'committed', quantity: 1 }, { name: 'on_hand', quantity: 4 }] }] } } }], pageInfo: { hasNextPage: true, endCursor: 'v1' } } } } }));
    const page = shopify.getInventoryLevels('1');
    assert.deepEqual(requests[0].body.variables, { id: PRODUCT, first: 15 });
    assert.deepEqual(page.items[0].levels[0], { locationId: LOCATION, locationName: 'Depot', available: 3, committed: 1, onHand: 4 });
    assert.equal(page.items[0].inventoryItemId, ITEM);
    assert.equal(page.nextCursor, 'v1');
    reset(by({ InventoryLevels: { product: null } }));
    assert.equal(shopify.getInventoryLevels('1'), null);
});

test('setInventory reads the level, checks the real change, and sends changeFromQuantity with the idempotency key', () => {
    reset(by({ AvailableAt: { inventoryItem: { id: ITEM, inventoryLevel: { quantities: [{ name: 'available', quantity: 10 }] } } }, InventorySet: { inventorySetQuantities: { inventoryAdjustmentGroup: { id: 'g' }, userErrors: [] } } }));
    shopify.setInventory('1', '1', 7, 'correction', KEY);
    assert.deepEqual(kinds(), ['AvailableAt', 'check', 'InventorySet']);
    assert.deepEqual(checks[0].context, { shop: SHOP, inventoryItemId: ITEM, locationId: LOCATION, quantity: 7, delta: -3 });
    assert.deepEqual(requests[1].body.variables, {
        idempotencyKey: KEY,
        input: { name: 'available', reason: 'correction', quantities: [{ inventoryItemId: ITEM, locationId: LOCATION, quantity: 7, changeFromQuantity: 10 }] },
    });
    assert.ok(requests[1].body.query.includes('inventorySetQuantities(input: $input) @idempotent(key: $idempotencyKey)'));
});

test('setInventory refuses an item not stocked at the location, and surfaces a stale write', () => {
    reset(by({ AvailableAt: { inventoryItem: { id: ITEM, inventoryLevel: null } } }));
    assert.throws(() => shopify.setInventory('1', '1', 7, 'correction', KEY), { code: 'not_stocked' });
    assert.deepEqual(checks, []);
    reset(by({ AvailableAt: { inventoryItem: { id: ITEM, inventoryLevel: { quantities: [{ name: 'available', quantity: 10 }] } } }, InventorySet: { inventorySetQuantities: { userErrors: [{ code: 'CHANGE_FROM_QUANTITY_STALE', field: ['input'], message: 'stale' }] } } }));
    assert.throws(() => shopify.setInventory('1', '1', 7, 'correction', KEY), { code: 'CHANGE_FROM_QUANTITY_STALE' });
});

test('adjustInventory sends an explicit null changeFromQuantity and the idempotency key', () => {
    reset(by({ InventoryAdjust: { inventoryAdjustQuantities: { inventoryAdjustmentGroup: { id: 'g' }, userErrors: [] } } }));
    shopify.adjustInventory('1', '1', -2, 'damaged', KEY);
    assert.deepEqual(checks[0].context, { shop: SHOP, inventoryItemId: ITEM, locationId: LOCATION, delta: -2 });
    assert.deepEqual(requests[0].body.variables, {
        idempotencyKey: KEY,
        input: { name: 'available', reason: 'damaged', changes: [{ inventoryItemId: ITEM, locationId: LOCATION, delta: -2, changeFromQuantity: null }] },
    });
    assert.ok(requests[0].body.query.includes('inventoryAdjustQuantities(input: $input) @idempotent(key: $idempotencyKey)'));
});

test('customers use the default email and phone fields', () => {
    const customer = { id: 'gid://shopify/Customer/1', firstName: 'Ann', lastName: 'Lee', defaultEmailAddress: { emailAddress: 'ann@example.com' }, defaultPhoneNumber: { phoneNumber: '+15550100' }, numberOfOrders: '3' };
    reset(by({ SearchCustomers: { customers: { nodes: [customer], pageInfo: { hasNextPage: false } } } }));
    assert.equal(shopify.searchCustomers({ query: 'ann' }).items[0].email, 'ann@example.com');
    reset(by({ GetCustomer: { customer: { ...customer, amountSpent: { amount: '99.00', currencyCode: 'USD' }, orders: { nodes: [rawOrder] } } } }));
    const full = shopify.getCustomer('1');
    assert.equal(full.numberOfOrders, 3);
    assert.equal(full.amountSpent, 99);
    assert.equal(full.orders[0].name, '#1001');
    reset(by({ GetCustomer: { customer: null } }));
    assert.equal(shopify.getCustomer('1'), null);
});

test('listAbandonedCheckouts maps the recovery link', () => {
    reset(by({ ListAbandonedCheckouts: { abandonedCheckouts: { nodes: [{ id: 'gid://shopify/AbandonedCheckout/1', name: '#1', createdAt: 't', abandonedCheckoutUrl: 'https://acme.example/recover', totalPriceSet: money('12.00') }], pageInfo: { hasNextPage: false } } } }));
    const [checkout] = shopify.listAbandonedCheckouts({ query: 'recovery_state:not_recovered' }).items;
    assert.equal(checkout.url, 'https://acme.example/recover');
    assert.equal(checkout.total, 12);
    assert.equal(checkout.completedAt, null);
});

test('createDiscountCode: the check sees percent, not a fraction, and an unused kind and no limit read as 0', () => {
    reset(by({ DiscountCreate: { discountCodeBasicCreate: { codeDiscountNode: { id: 'gid://shopify/DiscountCodeNode/1' }, userErrors: [] } } }));
    shopify.createDiscountCode({ title: 'Welcome', code: ' welcome20 ', percentage: 20, startsAt: '2026-10-01T00:00:00Z', oncePerCustomer: true });
    assert.deepEqual(checks[0].context, { shop: SHOP, code: 'WELCOME20', percentage: 20, amount: 0, usageLimit: 0 });
    assert.deepEqual(requests[0].body.variables.basicCodeDiscount, {
        title: 'Welcome',
        code: 'WELCOME20',
        startsAt: '2026-10-01T00:00:00Z',
        appliesOncePerCustomer: true,
        context: { all: 'ALL' },
        customerGets: { value: { percentage: 0.2 }, items: { all: true } },
    });
});

test('createDiscountCode with a fixed amount, a limit and a minimum', () => {
    reset(by({ DiscountCreate: { discountCodeBasicCreate: { codeDiscountNode: { id: 'gid://shopify/DiscountCodeNode/1' }, userErrors: [] } } }));
    shopify.createDiscountCode({ title: 'T', code: 'C', amount: 10, usageLimit: 5, minimumSubtotal: 50, startsAt: '2026-10-01T00:00:00Z', endsAt: '2026-11-01T00:00:00Z' });
    assert.deepEqual(checks[0].context, { shop: SHOP, code: 'C', percentage: 0, amount: 10, usageLimit: 5 });
    const discount = requests[0].body.variables.basicCodeDiscount;
    assert.deepEqual(discount.customerGets.value, { discountAmount: { amount: '10', appliesOnEachItem: false } });
    assert.equal(discount.usageLimit, 5);
    assert.equal(discount.endsAt, '2026-11-01T00:00:00Z');
    assert.deepEqual(discount.minimumRequirement, { subtotal: { greaterThanOrEqualToSubtotal: '50' } });
});

test('deactivateDiscountCode', () => {
    reset(by({ DiscountDeactivate: { discountCodeDeactivate: { codeDiscountNode: { id: 'gid://shopify/DiscountCodeNode/1' }, userErrors: [] } } }));
    assert.equal(shopify.deactivateDiscountCode('1').id, 'gid://shopify/DiscountCodeNode/1');
    assert.deepEqual(checks[0].context, { shop: SHOP, discountId: 'gid://shopify/DiscountCodeNode/1' });
});

test('createDraftOrder puts total units and the discount in the check', () => {
    reset(by({ DraftOrderCreate: { draftOrderCreate: { draftOrder: { id: 'gid://shopify/DraftOrder/1', name: '#D1', status: 'OPEN', invoiceUrl: 'https://acme.example/pay', totalPriceSet: money('9.00') }, userErrors: [] } } }));
    const draft = shopify.createDraftOrder({ lineItems: [{ variantId: '1', quantity: 2 }, { variantId: '2', quantity: 3 }], email: 'Buyer@Example.com', note: 'Phone order', discount: { percentage: 10, title: 'Thanks' } });
    assert.deepEqual(checks[0].context, { shop: SHOP, itemCount: 5, email: 'buyer@example.com', discountPercentage: 10 });
    assert.deepEqual(requests[0].body.variables.input, {
        lineItems: [{ variantId: VARIANT, quantity: 2 }, { variantId: 'gid://shopify/ProductVariant/2', quantity: 3 }],
        email: 'buyer@example.com',
        note: 'Phone order',
        appliedDiscount: { value: 10, valueType: 'PERCENTAGE', title: 'Thanks' },
    });
    assert.equal(draft.invoiceUrl, 'https://acme.example/pay');
});

test('a draft order without a discount or an email reads as 0 and empty in the check', () => {
    reset(by({ DraftOrderCreate: { draftOrderCreate: { draftOrder: { id: 'gid://shopify/DraftOrder/1' }, userErrors: [] } } }));
    shopify.createDraftOrder({ lineItems: [{ variantId: '1', quantity: 1 }] });
    assert.deepEqual(checks[0].context, { shop: SHOP, itemCount: 1, email: '', discountPercentage: 0 });
    assert.deepEqual(Object.keys(requests[0].body.variables.input), ['lineItems']);
});

test('sendDraftOrderInvoice checks the recipient', () => {
    reset(by({ DraftOrderSendInvoice: { draftOrderInvoiceSend: { draftOrder: { id: 'gid://shopify/DraftOrder/1' }, userErrors: [] } } }));
    shopify.sendDraftOrderInvoice('1', ' Buyer@Example.com ', 'Your order');
    assert.deepEqual(checks[0].context, { shop: SHOP, draftOrderId: 'gid://shopify/DraftOrder/1', to: 'buyer@example.com' });
    assert.deepEqual(requests[0].body.variables, { id: 'gid://shopify/DraftOrder/1', email: { to: 'buyer@example.com', subject: 'Your order' } });
});

test('errors: a GraphQL error, a throttle with a wait, a 429, a 401 with a string body, and a 500', () => {
    reset(() => reply(200, { errors: [{ message: 'Access denied', extensions: { code: 'ACCESS_DENIED' } }] }));
    assert.throws(() => shopify.getShop(), { name: 'ShopifyError', code: 'ACCESS_DENIED', status: 200, retryable: false });
    reset(() => reply(200, { errors: [{ message: 'Throttled', extensions: { code: 'THROTTLED' } }], extensions: { cost: { requestedQueryCost: 300, throttleStatus: { currentlyAvailable: 50, restoreRate: 100 } } } }));
    assert.throws(() => shopify.getShop(), { code: 'throttled', retryable: true, retryAfterSeconds: 3 });
    reset(() => reply(429, '', { 'retry-after': '2.0' }));
    assert.throws(() => shopify.getShop(), { code: 'throttled', status: 429, retryable: true, retryAfterSeconds: 2 });
    reset(() => reply(401, { errors: '[API] Invalid API key or access token' }));
    assert.throws(() => shopify.getShop(), (error) => error.code === 'unauthorized' && error.message.includes('Invalid API key'));
    reset(() => reply(500, '<html>oops</html>'));
    assert.throws(() => shopify.getShop(), { code: 'server_error', status: 500, retryable: false });
});

test('invalid input stops before the store, the check and any request', () => {
    reset(() => assert.fail('no request'), {});
    assert.throws(() => shopify.getOrder('abc'), { code: 'invalid_id' });
    assert.throws(() => shopify.refundOrder(ORDER, { idempotencyKey: KEY }), { code: 'nothing_to_refund' });
    assert.throws(() => shopify.adjustInventory(ITEM, LOCATION, 0, 'correction', KEY), { code: 'invalid_delta' });
    assert.deepEqual(events, []);
});
