import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";
import { test } from "node:test";

let key = "test-key";
let checks = [];
let requests = [];
let replies = [];

function reply(status, body) {
    return { ok: status >= 200 && status < 300, status, json: () => body };
}

globalThis.__chargeAfterHost = {
    secrets: { get: () => key },
    check: (capability, context) => { checks.push({ capability, context }); },
    get: (url, headers) => { requests.push({ method: "GET", url, headers }); return replies.shift(); },
    post: (url, body, headers) => { requests.push({ method: "POST", url, body, headers }); return replies.shift(); },
    patch: (url, body, headers) => { requests.push({ method: "PATCH", url, body, headers }); return replies.shift(); },
};

const root = new URL("../src/", import.meta.url);
const files = ["errors.ts", "request.ts", "response.ts", "lib.ts"];
let source = "const baseUrl = \"https://api-sandbox.ca-dev.co\";\nconst { get, post, patch, secrets, check } = globalThis.__chargeAfterHost;\n";
for (const file of files) {
    source += (await readFile(new URL(file, root), "utf8"))
        .replace(/^import .*;\n/gm, "")
        .replace(/^export \{ ChargeAfterError \} from .*;\n/gm, "");
}
const chargeafter = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(source)).toString("base64")}`);

function reset(...responses) {
    key = "test-key";
    checks = [];
    requests = [];
    replies = responses;
}
const transaction = { id: "transaction-1", chargeId: "charge-1", amount: 400, merchantOrderId: "order-1", status: "Settled", createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z", lenderId: "lender-1", merchantId: "merchant-1", type: "SETTLE" };
const lookup = { firstName: "Ada", lastName: "Lovelace", email: "ada@example.test", shippingAddress: { state: "CA", city: "San Francisco", line1: "1 Main", zipCode: "94105" }, billingAddress: { state: "CA", city: "San Francisco", line1: "1 Main", zipCode: "94105" }, netMonthlyIncome: 5000, dateOfBirth: "1990-01-01", phone: "+15550000001" };

test("getCharge sends the configured endpoint, bearer key, and checked id", () => {
    reset(reply(200, { id: "charge-1", state: "Authorized", totalAmount: 400, settledAmount: 0, refundedAmount: 0, merchantOrderId: "order-1" }));
    const charge = chargeafter.getCharge(" charge-1 ");
    assert.equal(charge.authorizedAmount, 400);
    assert.deepEqual(checks, [{ capability: "chargeafter.com/charges.get", context: { chargeId: "charge-1" } }]);
    assert.equal(requests[0].url, "https://api-sandbox.ca-dev.co/v2/post-sale/charges/charge-1");
    assert.equal(requests[0].headers.get("Authorization"), "Bearer test-key");
});

test("a settlement reads the authoritative charge before its capability check and write", () => {
    reset(
        reply(200, { id: "charge-1", state: "Authorized", totalAmount: 400, settledAmount: 0, refundedAmount: 0, merchantOrderId: "order-1" }),
        reply(201, transaction),
    );
    chargeafter.settleCharge("charge-1", 400, "merchant-tx");
    assert.equal(requests.length, 2);
    assert.deepEqual(checks.at(-1), { capability: "chargeafter.com/charges.settle", context: { chargeId: "charge-1", amount: 400, authorizedAmount: 400, merchantOrderId: "order-1" } });
    assert.equal(requests[1].url, "https://api-sandbox.ca-dev.co/v2/post-sale/charges/charge-1/settles");
    assert.deepEqual(requests[1].body, { amount: 400, merchantTransactionId: "merchant-tx" });
});

test("a limit failure happens before the write check", () => {
    reset(reply(200, { id: "charge-1", state: "Authorized", totalAmount: 400, settledAmount: 0, refundedAmount: 0, merchantOrderId: "order-1" }));
    assert.throws(() => chargeafter.settleCharge("charge-1", 401), /authorized amount/);
    assert.equal(requests.length, 1);
    assert.deepEqual(checks, []);
});

test("ChargeAfter API errors preserve documented codes", () => {
    reset(reply(400, { code: 2006, message: "Operation is not permitted" }));
    assert.throws(() => chargeafter.getCharge("charge-1"), error => error.code === "2006" && error.status === 400);
    reset(reply(401, {}));
    assert.throws(() => chargeafter.getLink("link-1"), error => error.code === "unauthorized" && error.status === 401);
    reset(reply(422, { errors: [{ code: 2004, description: "The request field is invalid" }] }));
    assert.throws(() => chargeafter.getConsumerAccounts("consumer-1"), error => error.code === "2004" && error.message === "The request field is invalid" && error.status === 422);
});

test("list reads do not turn API failures into empty successes", () => {
    reset(reply(401, {}));
    assert.throws(() => chargeafter.listFundings({ fromDate: "2026-01-01", toDate: "2026-01-31" }), error => error.code === "unauthorized" && error.status === 401);
    reset(reply(403, { code: 10201, message: "Consumer is not permitted" }));
    assert.throws(() => chargeafter.getConsumerAccounts("consumer-1"), error => error.code === "10201" && error.status === 403);
});

test("every remaining operation uses its documented method, path, and check", () => {
    const charge = { id: "c", state: "Authorized", totalAmount: 100, settledAmount: 60, refundedAmount: 10, merchantOrderId: "o" };
    const generated = { linkId: "link", url: "https://example.test/link", qrCode: null };
    const action = { ...transaction, chargeId: "c", merchantOrderId: "o" };
    const account = { id: "account", lenderId: "lender", lenderName: "Lender", status: "Approved", offers: [{ amount: 50, code: "PROMO" }] };
    const funding = { chargeId: "c", transactionId: "t", transactionAmount: 10 };
    const consumer = { applicationId: "app", accountsInfo: [{ account: { id: "account" } }] };
    reset(reply(200, { linkStatus: "New", linkType: "Apply" }), reply(200, generated), reply(200, generated),
        reply(200, { sessionStatus: "Active", flowType: "Checkout", applicationStatus: "Approved" }), reply(200, { accountsInfo: [account] }),
        reply(200, charge), reply(201, action), reply(200, charge), reply(201, action), reply(200, charge), reply(200, { records: [funding] }),
        reply(200, consumer), reply(200, { accountsInfo: [{ account: { id: "account" } }] }));
    const input = { consumer: { firstName: "Ada", lastName: "Lovelace", email: "ada@example.test" }, deliveryMethod: "Email", storeId: "store", channel: "E_COMMERCE" };
    chargeafter.getLink("link");
    chargeafter.sendApplyLink(input);
    chargeafter.sendCheckoutLink({ ...input, cart: { items: [{ sku: "sku", name: "part", quantity: 1, unitPrice: 100 }], totalAmount: 100 }, charge: true });
    chargeafter.getApplicationStatus("app");
    chargeafter.listApplicationOffers("app");
    chargeafter.voidCharge("c", 10, "tx");
    chargeafter.refundCharge("c", 10, undefined, "tx");
    chargeafter.setChargeOrderId("c", "new-order");
    chargeafter.listFundings({ fromDate: "2026-01-01", toDate: "2026-01-31", offset: 2, limit: 3, showMissing: true });
    chargeafter.lookupConsumer(lookup);
    chargeafter.getConsumerAccounts("consumer");
    assert.deepEqual(requests.map(request => request.method + " " + request.url.replace("https://api-sandbox.ca-dev.co", "")), [
        "GET /v2/omni-link/consumer/link", "POST /v2/omni-link/consumer/application", "POST /v2/omni-link/consumer/checkout",
        "GET /v2/checkout/applications/app/status", "POST /v2/checkout/applications/app/accounts", "GET /v2/post-sale/charges/c", "POST /v2/post-sale/charges/c/voids", "GET /v2/post-sale/charges/c", "POST /v2/post-sale/charges/c/refunds", "PATCH /v2/post-sale/charges/c", "GET /v2/post-sale/fundings/report?fromDate=2026-01-01&toDate=2026-01-31&offset=2&limit=3&showMissing=true", "POST /v2/consumers/consumer-details", "GET /v2/consumers/consumer/accounts",
    ]);
    assert.deepEqual(requests[1].body.merchantDetails.storeId, "store");
    assert.deepEqual(requests[1].body.applicationDetails, { channel: "E_COMMERCE" });
    assert.deepEqual(requests[2].body.cart.totalAmount, 100);
    assert.equal(requests[2].body.cart.items[0].price, 100);
    assert.equal(requests[2].body.checkoutOptions.charge, true);
    assert.deepEqual(requests[6].body, { amount: 10, merchantTransactionId: "tx" });
    assert.deepEqual(requests[8].body, { amount: 10, merchantTransactionId: "tx" });
    assert.deepEqual(requests[9].body, { merchantOrderId: "new-order" });
    assert.equal(replies.length, 0, "operations do not retry");
});

test("missing records, error codes, and refund limits preserve their boundaries", () => {
    reset(reply(400, { code: 2001 }));
    assert.equal(chargeafter.getCharge("missing"), null);
    for (const [status, code] of [[400, "2002"], [400, "2004"], [400, "2006"], [403, "10101"], [403, "10201"], [422, "field_error"]]) {
        reset(reply(status, { code, message: "API error" }));
        assert.throws(() => chargeafter.getLink("link"), error => error.code === code && error.status === status);
    }
    reset(reply(200, { id: "c", state: "Settled", totalAmount: 100, settledAmount: 60, refundedAmount: 10, merchantOrderId: "o" }));
    assert.throws(() => chargeafter.refundCharge("c", 51), /remaining refundable/);
    assert.equal(requests.length, 1, "refund cap permits only the authoritative read");
    assert.equal(checks.length, 0, "refund cap fails before the write check");
});

test("link checks and requests use one snapshot of caller getters", () => {
    let storeReads = 0;
    let deliveryReads = 0;
    let totalReads = 0;
    const input = {
        consumer: { firstName: "Ada", lastName: "Lovelace", email: "ada@example.test" },
        get storeId() { storeReads += 1; return storeReads === 1 ? "store-first" : "store-later"; },
        channel: "E_COMMERCE",
        get deliveryMethod() { deliveryReads += 1; return deliveryReads === 1 ? "Email" : "Sms"; },
        get cart() {
            return {
                items: [{ sku: "sku", name: "part", quantity: 1, unitPrice: 100 }],
                get totalAmount() { totalReads += 1; return totalReads === 1 ? 100 : 999; },
            };
        },
        charge: true,
    };
    reset(reply(200, { linkId: "link", url: null, qrCode: null }));
    chargeafter.sendCheckoutLink(input);
    assert.deepEqual(checks, [{ capability: "chargeafter.com/links.sendCheckout", context: { storeId: "store-first", deliveryMethod: "Email", totalAmount: 100, autoCharge: true } }]);
    assert.equal(requests[0].body.merchantDetails.storeId, "store-first");
    assert.equal(requests[0].body.linkOptions.deliveryMethod, "Email");
    assert.equal(requests[0].body.cart.totalAmount, 100);
    assert.equal(storeReads, 1);
    assert.equal(deliveryReads, 1);
    assert.equal(totalReads, 1);
});
