// MOCK HARNESS. Exercises the actual package operations against a mocked Submilli
// host and a mocked Wix API, without network access or credentials. Passing here
// means the requests match this file's reading of Wix's documentation. It says
// nothing about the live API. Native build tests separately verify Submilli
// compatibility; this harness checks the requests each operation sends and what
// `check()` sees.
//
//   node --test packages/wix-ecommerce/scripts/contract.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
import { test } from 'node:test';

const SITE = '11111111-2222-4333-8444-555555555555';
const ORDER = 'aaaaaaaa-0000-4000-8000-000000000001';
const PAYMENT = 'bbbbbbbb-0000-4000-8000-000000000001';
const LINE = 'cccccccc-0000-4000-8000-000000000001';
const FULFILLMENT = 'dddddddd-0000-4000-8000-000000000001';
const PRODUCT = 'eeeeeeee-0000-4000-8000-000000000001';
const VARIANT = 'ffffffff-0000-4000-8000-000000000001';
const ITEM = '99999999-0000-4000-8000-000000000001';
const API = 'https://www.wixapis.com/';

let events = [];
let requests = [];
let checks = [];
let boundSecrets = {};
let denied = new Set();
let respond = () => reply(500, { message: 'no handler' });

function reset(handler, secrets = { WIX_API_KEY: 'wix-test-key', WIX_SITE_ID: SITE }) {
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
        statusText: { 200: 'OK', 404: 'Not Found', 429: 'Too Many Requests', 500: 'Internal Server Error' }[status] ?? '',
        headers: new Map(Object.entries(headers)),
        body: text,
        url: '',
        json: () => JSON.parse(text),
    };
}

function send(method, url, body, headers) {
    const request = { method, url, body, headers, path: url.replace(API, '') };
    requests.push(request);
    events.push({ kind: 'request', method, path: request.path });
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

globalThis.__wixHost = {
    get: (url, headers) => send('GET', url, undefined, headers),
    post: (url, body, headers) => send('POST', url, body, headers),
    patch: (url, body, headers) => send('PATCH', url, body, headers),
    encodeComponent: encodeURIComponent,
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
const bundle = 'const { get, post, patch, secrets, check, encodeComponent } = globalThis.__wixHost;\n' +
    (await source('errors.ts')) + (await source('request.ts')) + (await source('response.ts')) + (await source('lib.ts'));
const wix = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(bundle)).toString('base64')}`);

const rawOrder = {
    id: ORDER,
    number: '10042',
    status: 'APPROVED',
    paymentStatus: 'PAID',
    fulfillmentStatus: 'NOT_FULFILLED',
    currency: 'USD',
    priceSummary: { total: { amount: '150.50' } },
    buyerInfo: { email: 'buyer@example.com', contactId: VARIANT },
    createdDate: '2026-10-01T10:00:00Z',
    lineItems: [{ id: LINE, productName: { original: 'Mug' }, quantity: 2, price: { amount: '75.25' }, catalogReference: { catalogItemId: PRODUCT }, physicalProperties: { sku: 'MUG-1' } }],
};
const rawPayments = {
    orderTransactions: {
        payments: [{ id: PAYMENT, amount: { amount: '150.50' }, createdDate: '2026-10-01T10:00:00Z' }],
        refunds: [
            { id: 'r1', transactions: [{ paymentId: PAYMENT, amount: { amount: '50.25' }, refundStatus: 'SUCCEEDED' }] },
            { id: 'r2', transactions: [{ paymentId: PAYMENT, amount: { amount: '100' }, refundStatus: 'FAILED' }] },
        ],
    },
};

function orderAndPayments(extra) {
    return (request) => {
        if (request.method === 'GET' && request.path === `ecom/v1/orders/${ORDER}`) return reply(200, { order: rawOrder });
        if (request.method === 'GET' && request.path === `ecom/v1/payments/orders/${ORDER}`) return reply(200, rawPayments);
        return extra(request);
    };
}

function catalog(version, extra) {
    return (request) => (request.path === 'stores/v3/provision/version' ? reply(200, { catalogVersion: version }) : extra(request));
}

function kinds() {
    return events.map((event) => (event.kind === 'check' ? 'check' : `${event.method} ${event.path}`));
}

test('every request carries the API key as it is, and the site id', () => {
    reset(() => reply(200, { orders: [] }));
    wix.searchOrders();
    assert.equal(requests[0].headers.get('Authorization'), 'wix-test-key');
    assert.equal(requests[0].headers.get('wix-site-id'), SITE);
    assert.equal(requests[0].url, API + 'ecom/v1/orders/search');
});

test('searchOrders builds the filter and reads the page', () => {
    reset(() => reply(200, { orders: [rawOrder], metadata: { hasNext: true, cursors: { next: 'n1' } } }));
    const page = wix.searchOrders(
        { status: 'approved', buyerEmail: ' Buyer@Example.com ', createdAfter: '2026-10-01T00:00:00Z', number: 10042, sort: [{ field: 'number', order: 'DESC' }] },
        { limit: 25 },
    );
    assert.deepEqual(requests[0].body, {
        search: {
            cursorPaging: { limit: 25 },
            filter: { status: 'APPROVED', 'buyerInfo.email': 'buyer@example.com', createdDate: { $gte: '2026-10-01T00:00:00Z' }, number: 10042 },
            sort: [{ fieldName: 'number', order: 'DESC' }],
        },
    });
    assert.deepEqual(checks, [{ capability: 'wix.com/orders.search', context: { siteId: SITE } }]);
    assert.equal(page.nextCursor, 'n1');
    assert.equal(page.items[0].number, '10042');
    assert.equal(page.items[0].total, 150.5);
    assert.equal(page.items[0].lineItems[0].productId, PRODUCT);
});

test('a cursor request sends only the cursor, which carries the filter', () => {
    reset(() => reply(200, { orders: [], metadata: { hasNext: false, cursors: { next: 'n2' } } }));
    const page = wix.searchOrders({ status: 'APPROVED' }, { limit: 10, cursor: 'c1' });
    assert.deepEqual(requests[0].body, { search: { cursorPaging: { limit: 10, cursor: 'c1' } } });
    assert.equal(page.nextCursor, null);
});

test('order number as a JSON number, as the schema describes it, reads the same', () => {
    reset(() => reply(200, { order: { ...rawOrder, number: 10042 } }));
    assert.equal(wix.getOrder(ORDER).number, '10042');
});

test('getOrder normalizes the id, and a 404 is null', () => {
    reset(() => reply(404, { message: 'nope' }));
    assert.equal(wix.getOrder(ORDER.toUpperCase()), null);
    assert.deepEqual(checks[0].context, { siteId: SITE, orderId: ORDER });
});

test('addOrderNote posts a merchant comment', () => {
    reset(() => reply(200, { order: rawOrder }));
    wix.addOrderNote(ORDER, 'Call first');
    assert.equal(requests[0].path, `ecom/v1/orders/${ORDER}/activities/add`);
    assert.deepEqual(requests[0].body, { orderActivities: [{ activityType: 'MERCHANT_COMMENT', merchantComment: { message: 'Call first' } }] });
});

test('cancelOrder fetches the order, checks its real total, then posts', () => {
    reset(orderAndPayments(() => reply(200, { order: { ...rawOrder, status: 'CANCELED' } })));
    const order = wix.cancelOrder(ORDER, { restock: true, notifyCustomer: true, message: 'Sorry' });
    assert.deepEqual(kinds(), [`GET ecom/v1/orders/${ORDER}`, 'check', `POST ecom/v1/orders/${ORDER}/cancel`]);
    assert.deepEqual(checks[0].context, { siteId: SITE, orderId: ORDER, totalAmount: 150.5, currency: 'USD', paymentStatus: 'PAID', restock: true });
    assert.deepEqual(requests[1].body, { restockAllItems: true, sendOrderCanceledEmail: true, customMessage: 'Sorry' });
    assert.equal(order.status, 'CANCELED');
});

test('a denied cancel sends nothing', () => {
    reset(orderAndPayments(() => reply(200, {})));
    denied.add('wix.com/orders.cancel');
    assert.throws(() => wix.cancelOrder(ORDER), { name: 'PermissionDeniedError' });
    assert.equal(requests.filter((r) => r.method === 'POST').length, 0);
});

test('an order with no readable total stops before the check: a missing total never reads as zero', () => {
    reset((request) => reply(200, { order: { ...rawOrder, priceSummary: {} } }));
    assert.throws(() => wix.cancelOrder(ORDER), { name: 'WixError', code: 'unexpected_response' });
    assert.equal(checks.length, 0);
});

test('a missing order stops a cancel with not_found', () => {
    reset(() => reply(404, {}));
    assert.throws(() => wix.cancelOrder(ORDER), { code: 'not_found' });
    assert.equal(checks.length, 0);
});

test('createFulfillment sends the chosen lines and the tracking', () => {
    reset(() => reply(200, { orderWithFulfillments: { fulfillments: [{ id: FULFILLMENT, trackingInfo: { trackingNumber: '1Z', shippingProvider: 'ups' }, completed: true }] } }));
    const items = wix.createFulfillment(ORDER, { tracking: { number: '1Z', provider: 'ups', link: 'https://ups.example/1Z' }, lineItems: [{ lineItemId: LINE, quantity: 1 }] });
    assert.equal(requests[0].path, `ecom/v1/fulfillments/orders/${ORDER}/create-fulfillment`);
    assert.deepEqual(requests[0].body, {
        fulfillment: { lineItems: [{ id: LINE, quantity: 1 }], trackingInfo: { trackingNumber: '1Z', shippingProvider: 'ups', trackingLink: 'https://ups.example/1Z' } },
    });
    assert.deepEqual(checks[0].context, { siteId: SITE, orderId: ORDER, shippingProvider: 'ups' });
    assert.equal(items[0].tracking.number, '1Z');
});

test('createFulfillment without lines resolves every line of the order, after the check', () => {
    reset(orderAndPayments(() => reply(200, { orderWithFulfillments: { fulfillments: [] } })));
    wix.createFulfillment(ORDER, { tracking: { number: '1Z', provider: 'ups' } });
    assert.deepEqual(kinds(), ['check', `GET ecom/v1/orders/${ORDER}`, `POST ecom/v1/fulfillments/orders/${ORDER}/create-fulfillment`]);
    assert.deepEqual(requests[1].body.fulfillment.lineItems, [{ id: LINE }]);
});

test('updateTracking patches only the fields set', () => {
    reset(() => reply(200, { orderWithFulfillments: { fulfillments: [] } }));
    wix.updateTracking(ORDER, FULFILLMENT, { number: '2Z' });
    assert.equal(requests[0].method, 'PATCH');
    assert.equal(requests[0].path, `ecom/v1/fulfillments/${FULFILLMENT}/orders/${ORDER}`);
    assert.deepEqual(requests[0].body, { fulfillment: { trackingInfo: { trackingNumber: '2Z' } } });
    assert.throws(() => wix.updateTracking(ORDER, FULFILLMENT, {}), { code: 'empty_update' });
});

test('listPayments works out what is left, and a failed refund took nothing', () => {
    reset(orderAndPayments(() => reply(500, {})));
    const [payment] = wix.listPayments(ORDER);
    assert.equal(payment.refunded, 50.25);
    assert.equal(payment.refundable, 100.25);
});

test('cancelOrder refuses an order with no currency, before its check', () => {
    reset((request) => (request.path === `ecom/v1/orders/${ORDER}` ? reply(200, { order: { ...rawOrder, currency: undefined } }) : reply(500, {})));
    assert.throws(() => wix.cancelOrder(ORDER), { code: 'unexpected_response' });
    assert.deepEqual(checks, []);
});

test('refundOrder resolves the currency and the payment, checks, then posts one refund', () => {
    reset(orderAndPayments(() => reply(200, { refund: { id: 'r3', transactions: [{ refundStatus: 'SUCCEEDED' }] } })));
    const refund = wix.refundOrder(ORDER, {
        paymentId: PAYMENT, amount: 25.5, lineItems: [{ lineItemId: LINE, quantity: 1 }], restock: true, notifyCustomer: true, message: 'Sorry', reason: 'Damaged',
    });
    assert.deepEqual(kinds(), [`GET ecom/v1/orders/${ORDER}`, `GET ecom/v1/payments/orders/${ORDER}`, 'check', 'POST ecom/v1/order-billing/refund-payments']);
    assert.deepEqual(checks[0].context, { siteId: SITE, orderId: ORDER, amount: 25.5, currency: 'USD', restock: true, externalRefund: false });
    assert.deepEqual(requests[2].body, {
        orderId: ORDER,
        paymentRefunds: [{ paymentId: PAYMENT, amount: { amount: '25.5' } }],
        sideEffects: { restock: { lineItems: [{ lineItemId: LINE, quantity: 1 }] }, notifications: { sendCustomerEmail: true, customMessage: 'Sorry' } },
        refundItems: { lineItems: [{ lineItemId: LINE, quantity: 1 }] },
        customerReason: 'Damaged',
    });
    assert.deepEqual(refund, { id: 'r3', status: 'SUCCEEDED' });
});

test('a refund above what is left on the payment is refused with no check and no refund', () => {
    reset(orderAndPayments(() => reply(500, {})));
    assert.throws(() => wix.refundOrder(ORDER, { paymentId: PAYMENT, amount: 100.26 }), { name: 'WixError', code: 'refund_too_large' });
    assert.equal(checks.length, 0);
    assert.equal(requests.filter((r) => r.method === 'POST').length, 0);
});

test('the full remainder of a payment can be refunded', () => {
    reset(orderAndPayments(() => reply(200, { refund: { id: 'r4', transactions: [] } })));
    wix.refundOrder(ORDER, { paymentId: PAYMENT, amount: 100.25 });
    assert.equal(checks.length, 1);
});

test('a payment that is not on the order, or that Wix will not refund, is refused', () => {
    reset(orderAndPayments(() => reply(500, {})));
    assert.throws(() => wix.refundOrder(ORDER, { paymentId: ITEM, amount: 1 }), { code: 'payment_not_found' });
    reset((request) => reply(200, request.path.includes('payments')
        ? { orderTransactions: { payments: [{ id: PAYMENT, amount: { amount: '10' }, refundDisabled: true }] } }
        : { order: rawOrder }));
    assert.throws(() => wix.refundOrder(ORDER, { paymentId: PAYMENT, amount: 1 }), { code: 'refund_disabled' });
});

test('restock needs line items, and an external refund is marked', () => {
    reset(orderAndPayments(() => reply(200, { refund: { id: 'r5' } })));
    assert.throws(() => wix.refundOrder(ORDER, { paymentId: PAYMENT, amount: 1, restock: true }), { code: 'restock_needs_line_items' });
    assert.equal(requests.length, 0);
    wix.refundOrder(ORDER, { paymentId: PAYMENT, amount: 1, externalRefund: true });
    assert.equal(checks[0].context.externalRefund, true);
    assert.equal(requests.at(-1).body.paymentRefunds[0].externalRefund, true);
});

test('a getter cannot change a refund amount after the check', () => {
    reset(orderAndPayments(() => reply(200, { refund: { id: 'r6' } })));
    let reads = 0;
    const input = { paymentId: PAYMENT, get amount() { reads += 1; return reads === 1 ? 1 : 9999; } };
    wix.refundOrder(ORDER, input);
    assert.equal(reads, 1);
    assert.equal(requests.at(-1).body.paymentRefunds[0].amount.amount, '1');
});

test('abandoned checkouts list newest first, and one comes back with its link', () => {
    reset(() => reply(200, { abandonedCheckouts: [{ id: ITEM, status: 'ABANDONED', buyerInfo: { email: 'b@example.com' }, totalPrice: { amount: '12.00' }, currency: 'USD' }], metadata: { hasNext: true, cursors: { next: 'a1' } } }));
    const page = wix.listAbandonedCheckouts({ status: 'abandoned' }, { limit: 5 });
    assert.deepEqual(requests[0].body, { query: { cursorPaging: { limit: 5 }, filter: { status: 'ABANDONED' }, sort: [{ fieldName: 'createdDate', order: 'DESC' }] } });
    assert.equal(page.items[0].total, 12);
    assert.equal(page.nextCursor, 'a1');
    reset(() => reply(200, { abandonedCheckout: { id: ITEM, status: 'ABANDONED', checkoutUrl: 'https://shop.example/recover' } }));
    assert.equal(wix.getAbandonedCheckout(ITEM).checkoutUrl, 'https://shop.example/recover');
    assert.deepEqual(checks[0].context, { siteId: SITE, abandonedCheckoutId: ITEM });
});

test('createCart sends Stores catalog references and checks the unit count', () => {
    reset(() => reply(200, { cart: { id: ITEM, lineItems: [{}, {}] } }));
    const cart = wix.createCart([{ productId: PRODUCT, quantity: 2 }, { productId: PRODUCT, quantity: 3, variantId: VARIANT }], { email: 'B@example.com', firstName: 'Bo' });
    assert.deepEqual(checks[0].context, { siteId: SITE, itemCount: 5 });
    assert.equal(requests[0].path, 'ecom/v2/carts');
    assert.deepEqual(requests[0].body, {
        catalogItems: [
            { catalogReference: { catalogItemId: PRODUCT, appId: '215238eb-22a5-4c36-9e7b-e7c08025e04e' }, quantity: 2 },
            { catalogReference: { catalogItemId: PRODUCT, appId: '215238eb-22a5-4c36-9e7b-e7c08025e04e', options: { variantId: VARIANT } }, quantity: 3 },
        ],
        cart: { customerInfo: { email: 'b@example.com', firstName: 'Bo' } },
    });
    assert.deepEqual(cart, { id: ITEM, lineCount: 2 });
});

test('getCheckoutUrl returns the link', () => {
    reset(() => reply(200, { checkoutUrl: 'https://shop.example/checkout' }));
    assert.equal(wix.getCheckoutUrl(ITEM), 'https://shop.example/checkout');
    assert.equal(requests[0].path, `ecom/v2/carts/${ITEM}/get-checkout-url`);
});

test('getCatalogVersion returns the raw version, including no Stores', () => {
    reset(() => reply(200, { catalogVersion: 'STORES_NOT_INSTALLED' }));
    assert.equal(wix.getCatalogVersion(), 'STORES_NOT_INSTALLED');
    assert.throws(() => wix.getProduct(PRODUCT), { code: 'stores_not_installed' });
});

test('searchProducts on catalog V1 uses a JSON string filter and offset cursors', () => {
    reset(catalog('V1_CATALOG', () => reply(200, { products: [{ id: PRODUCT, name: 'Mug', slug: 'mug', visible: true, priceData: { price: 12.5, currency: 'USD' } }], totalResults: 3 })));
    const page = wix.searchProducts({ text: 'mug', includeHidden: true }, { limit: 1, cursor: '1' });
    assert.deepEqual(checks[0].context, { siteId: SITE, includeHidden: true });
    assert.equal(requests[1].path, 'stores/v1/products/query');
    assert.deepEqual(requests[1].body, { query: { paging: { limit: 1, offset: 1 }, filter: '{"name":{"$contains":"mug"}}' }, includeHiddenProducts: true });
    assert.equal(page.items[0].price, 12.5);
    assert.equal(page.nextCursor, '2');
});

test('searchProducts on catalog V1 stops at the last page, and refuses a cursor from V3', () => {
    reset(catalog('V1_CATALOG', () => reply(200, { products: [{ id: PRODUCT }], totalResults: 1 })));
    assert.equal(wix.searchProducts().nextCursor, null);
    assert.throws(() => wix.searchProducts({}, { cursor: 'abc' }), { code: 'invalid_cursor' });
});

test('searchProducts on catalog V3 uses cursor search and hides hidden products', () => {
    reset(catalog('V3_CATALOG', () => reply(200, { products: [{ id: PRODUCT, name: 'Mug', visible: true, currency: 'USD', plainDescription: '<p>Nice</p>', actualPriceRange: { minValue: { amount: '9.50' } } }], pagingMetadata: { hasNext: true, cursors: { next: 'p2' } } })));
    const page = wix.searchProducts({ text: 'mug' }, { limit: 2 });
    assert.equal(requests[1].path, 'stores/v3/products/search');
    assert.deepEqual(requests[1].body, { search: { cursorPaging: { limit: 2 }, filter: { visible: true }, search: { expression: 'mug' } }, fields: ['CURRENCY', 'PLAIN_DESCRIPTION'] });
    assert.equal(page.items[0].price, 9.5);
    assert.equal(page.items[0].catalog, 'V3_CATALOG');
    assert.equal(page.nextCursor, 'p2');
});

test('getProduct follows the catalog version', () => {
    reset(catalog('V1_CATALOG', () => reply(200, { product: { id: PRODUCT, name: 'Mug' } })));
    assert.equal(wix.getProduct(PRODUCT).catalog, 'V1_CATALOG');
    assert.equal(requests[1].path, `stores/v1/products/${PRODUCT}`);
    reset(catalog('V3_CATALOG', () => reply(404, {})));
    assert.equal(wix.getProduct(PRODUCT), null);
    assert.equal(requests[1].path, `stores/v3/products/${PRODUCT}?fields=CURRENCY&fields=PLAIN_DESCRIPTION`);
});

test('updateProduct on catalog V1 patches the fields and the check lists their sorted names', () => {
    reset(catalog('V1_CATALOG', () => reply(200, { product: { id: PRODUCT, name: 'New' } })));
    wix.updateProduct(PRODUCT, { visible: false, name: 'New', price: 12.5 });
    assert.deepEqual(checks[0].context, { siteId: SITE, productId: PRODUCT, fields: ['name', 'price', 'visible'] });
    assert.equal(requests[1].method, 'PATCH');
    assert.deepEqual(requests[1].body, { product: { id: PRODUCT, name: 'New', visible: false, priceData: { price: 12.5 } } });
});

test('updateProduct on catalog V3 reads the revision and sends it back', () => {
    reset(catalog('V3_CATALOG', (request) => (request.method === 'GET'
        ? reply(200, { product: { id: PRODUCT, revision: '7' } })
        : reply(200, { product: { id: PRODUCT, name: 'New', revision: '8' } }))));
    wix.updateProduct(PRODUCT, { name: 'New', description: '<p>x</p>' });
    assert.equal(requests[2].path, `stores/v3/products/${PRODUCT}`);
    assert.deepEqual(requests[2].body, { product: { id: PRODUCT, revision: '7', name: 'New', plainDescription: '<p>x</p>' } });
});

test('updateProduct refuses price and ribbon on catalog V3, and an empty update', () => {
    reset(catalog('V3_CATALOG', () => reply(500, {})));
    assert.throws(() => wix.updateProduct(PRODUCT, { price: 1 }), { code: 'unsupported_on_catalog' });
    assert.equal(requests.filter((r) => r.method === 'PATCH').length, 0);
    assert.throws(() => wix.updateProduct(PRODUCT, {}), { code: 'empty_update' });
});

test('getInventory on catalog V1 and V3', () => {
    reset(catalog('V1_CATALOG', () => reply(200, { inventoryItems: [{ id: ITEM, variants: [{ variantId: VARIANT, quantity: 4, inStock: true }] }] })));
    assert.deepEqual(wix.getInventory(PRODUCT), [{ variantId: VARIANT, inventoryItemId: ITEM, locationId: null, quantity: 4, inStock: true }]);
    assert.equal(requests[1].path, 'stores/v2/inventoryItems/query');
    assert.deepEqual(requests[1].body, { query: { filter: `{"productId":"${PRODUCT}"}` } });
    reset(catalog('V3_CATALOG', () => reply(200, { inventoryItems: [{ id: ITEM, variantId: VARIANT, locationId: PAYMENT, quantity: 2 }] })));
    assert.deepEqual(wix.getInventory(PRODUCT), [{ variantId: VARIANT, inventoryItemId: ITEM, locationId: PAYMENT, quantity: 2, inStock: true }]);
    assert.equal(requests[1].path, 'stores/v3/inventory-items/query');
    assert.deepEqual(requests[1].body, { query: { filter: { productId: PRODUCT }, cursorPaging: { limit: 100 } } });
});

test('adjustInventory on catalog V1 increments and decrements', () => {
    reset(catalog('V1_CATALOG', () => reply(200, {})));
    wix.adjustInventory(PRODUCT, VARIANT, 5);
    assert.equal(requests[1].path, 'stores/v2/inventoryItems/increment');
    assert.deepEqual(requests[1].body, { incrementData: [{ productId: PRODUCT, variantId: VARIANT, incrementBy: 5 }] });
    assert.deepEqual(checks[0].context, { siteId: SITE, productId: PRODUCT, variantId: VARIANT, delta: 5 });
    reset(catalog('V1_CATALOG', () => reply(200, {})));
    wix.adjustInventory(PRODUCT, VARIANT, -3);
    assert.equal(requests[1].path, 'stores/v2/inventoryItems/decrement');
    assert.deepEqual(requests[1].body, { decrementData: [{ productId: PRODUCT, variantId: VARIANT, decrementBy: 3 }] });
});

test('adjustInventory on catalog V3 resolves the inventory item', () => {
    const levels = [{ id: ITEM, variantId: VARIANT, quantity: 2 }];
    reset(catalog('V3_CATALOG', (request) => (request.path.endsWith('query') ? reply(200, { inventoryItems: levels }) : reply(200, {}))));
    wix.adjustInventory(PRODUCT, VARIANT, -1);
    assert.equal(requests.at(-1).path, 'stores/v3/bulk/inventory-items/decrement');
    assert.deepEqual(requests.at(-1).body, { decrementData: [{ inventoryItemId: ITEM, decrementBy: 1 }], reason: 'MANUAL' });
});

test('adjustInventory on catalog V3 refuses a variant in several locations, and an unknown one', () => {
    reset(catalog('V3_CATALOG', () => reply(200, { inventoryItems: [{ id: ITEM, variantId: VARIANT }, { id: PAYMENT, variantId: VARIANT }] })));
    assert.throws(() => wix.adjustInventory(PRODUCT, VARIANT, 1), { code: 'ambiguous_inventory_item' });
    assert.equal(requests.filter((r) => r.path.includes('bulk')).length, 0);
    reset(catalog('V3_CATALOG', () => reply(200, { inventoryItems: [] })));
    assert.throws(() => wix.adjustInventory(PRODUCT, VARIANT, 1), { code: 'not_found' });
});

test('findContactByEmail filters on the email, and two matches are an error', () => {
    reset(() => reply(200, { contacts: [{ id: ITEM, primaryInfo: { email: 'b@example.com', phone: '+1555' }, info: { name: { first: 'Bo', last: 'Lee' }, company: 'Acme' }, createdDate: '2026-01-01T00:00:00Z' }] }));
    const contact = wix.findContactByEmail(' B@Example.com ');
    assert.deepEqual(requests[0].body, { query: { filter: { 'info.emails.email': 'b@example.com' }, paging: { limit: 2 }, fieldsets: ['FULL'] } });
    assert.deepEqual(checks[0].context, { siteId: SITE, email: 'b@example.com' });
    assert.equal(contact.firstName, 'Bo');
    assert.equal(contact.company, 'Acme');
    reset(() => reply(200, { contacts: [{ id: ITEM }, { id: PAYMENT }] }));
    assert.throws(() => wix.findContactByEmail('b@example.com'), { code: 'multiple_matches' });
    reset(() => reply(200, { contacts: [] }));
    assert.equal(wix.findContactByEmail('b@example.com'), null);
});

test('getContact asks for every field', () => {
    reset(() => reply(200, { contact: { id: ITEM, primaryInfo: { email: 'b@example.com' } } }));
    assert.equal(wix.getContact(ITEM).email, 'b@example.com');
    assert.equal(requests[0].path, `contacts/v4/contacts/${ITEM}?fieldsets=FULL`);
});

test('errors: Wix application code, 429 with a wait, 429 without, and 500', () => {
    reset(() => reply(400, { message: 'Bad filter', details: { applicationError: { code: 'INVALID_FILTER' } } }));
    assert.throws(() => wix.searchOrders(), { name: 'WixError', code: 'INVALID_FILTER', status: 400, message: 'Wix 400 INVALID_FILTER: Bad filter', retryable: false });
    reset(() => reply(429, 'slow down', { 'retry-after': '30' }));
    assert.throws(() => wix.searchOrders(), { code: 'rate_limited', status: 429, retryable: true, retryAfterSeconds: 30 });
    reset(() => reply(429, ''));
    assert.throws(() => wix.searchOrders(), { code: 'rate_limited', retryAfterSeconds: null });
    reset(() => reply(500, ''));
    assert.throws(() => wix.searchOrders(), { code: 'server_error', status: 500, retryable: false, message: 'Wix request failed: HTTP 500 Internal Server Error' });
});

test('a missing or malformed site id stops before the check, and a missing key before the request', () => {
    reset(() => reply(200, {}), { WIX_API_KEY: 'k' });
    assert.throws(() => wix.searchOrders(), { code: 'missing_site_id' });
    assert.equal(checks.length, 0);
    reset(() => reply(200, {}), { WIX_API_KEY: 'k', WIX_SITE_ID: 'not-a-guid' });
    assert.throws(() => wix.searchOrders(), { code: 'invalid_site_id' });
    reset(() => reply(200, {}), { WIX_SITE_ID: SITE });
    assert.throws(() => wix.searchOrders(), { code: 'missing_api_key' });
    assert.equal(requests.length, 0);
});

test('invalid input stops before the site id, the check and any request', () => {
    reset(() => reply(200, {}), {});
    for (const [code, action] of [
        ['invalid_id', () => wix.getOrder('1')],
        ['invalid_limit', () => wix.searchOrders({}, { limit: 101 })],
        ['invalid_limit', () => wix.searchOrders({}, { limit: 0 })],
        ['invalid_sort', () => wix.searchOrders({ sort: [{ field: 'secret', order: 'ASC' }] })],
        ['invalid_sort', () => wix.searchOrders({ sort: [1, 2, 3, 4, 5].map(() => ({ field: 'number', order: 'ASC' })) })],
        ['invalid_date', () => wix.searchOrders({ createdAfter: 'yesterday' })],
        ['invalid_email', () => wix.findContactByEmail('nobody')],
        ['invalid_amount', () => wix.refundOrder(ORDER, { paymentId: PAYMENT, amount: 0 })],
        ['invalid_amount', () => wix.refundOrder(ORDER, { paymentId: PAYMENT, amount: 0.1 + 0.2 })],
        ['message_without_notification', () => wix.cancelOrder(ORDER, { message: 'x' })],
        ['invalid_delta', () => wix.adjustInventory(PRODUCT, VARIANT, 0)],
        ['invalid_delta', () => wix.adjustInventory(PRODUCT, VARIANT, 1.5)],
        ['invalid_line_items', () => wix.createCart([])],
        ['invalid_quantity', () => wix.createCart([{ productId: PRODUCT, quantity: 0 }])],
        ['invalid_tracking_link', () => wix.createFulfillment(ORDER, { tracking: { number: '1', provider: 'x', link: 'http://x.example' } })],
    ]) {
        assert.throws(action, { name: 'WixError', code });
    }
    assert.equal(checks.length, 0);
    assert.equal(requests.length, 0);
});
