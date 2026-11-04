// Offline tests. The normalizers, builders and mappers live in the package's
// internal modules, so these tests import them from `../src/`. The operations
// section checks that each operation validates its input before any request:
// invalid input fails with a local ShopifyError (status 0). The last section stops
// at the missing store, and is skipped when SHOPIFY_SHOP is bound, so this file
// never sends a request.
import { expectException, label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    ShopifyError,
    addOrderNote,
    addOrderTags,
    adjustInventory,
    cancelOrder,
    createDiscountCode,
    createDraftOrder,
    createFulfillment,
    getCustomer,
    getInventoryLevels,
    getOrder,
    getProduct,
    getShop,
    holdFulfillmentOrder,
    listAbandonedCheckouts,
    listFulfillmentOrders,
    refundOrder,
    searchCustomers,
    searchOrders,
    searchProducts,
    sendDraftOrderInvoice,
    setInventory,
    suggestRefund,
    updateProduct,
    updateTracking,
    updateVariantPrices,
} from "@submilli/shopify-admin";
import {
    buildDiscount,
    buildInventoryAdjust,
    buildInventorySet,
    inventoryWindow,
    moneyText,
    normalizeAmount,
    normalizeCode,
    normalizeDelta,
    normalizeEmail,
    normalizeGid,
    normalizeHttpsUrl,
    normalizeInstant,
    normalizeShop,
    normalizeStockLevel,
    normalizeTag,
    pageWindow,
    productFields,
} from "../src/request";
import {
    appendNote,
    availableFrom,
    chooseFulfillmentOrders,
    decimalOrNull,
    graphQlFailure,
    httpFailure,
    mergeTags,
    nextCursorFrom,
    orderFrom,
    requireAmount,
    requireNoUserErrors,
    retryAfterFrom,
    suggestedRefundFrom,
    throttleWait,
} from "../src/response";

const KEY = "3f5b6ebf-143c-4da5-8d0f-fb8553bfd85d";
const LOCATION_A = "gid://shopify/Location/1";
const LOCATION_B = "gid://shopify/Location/2";

function expectCode(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: ShopifyError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function main(): void {
    label("a store is a single label, so the host is always <label>.myshopify.com");
    assert(normalizeShop(" Acme-Store ") === "acme-store", "label normalized");
    for (const bad of ["", "-acme", "acme-", "acme.myshopify.com", "evil.example/", "a b", "acme.evil.example", "acme:443", "acme#", "a/b", "acme@evil.example"]) {
        expectCode("invalid_shop", () => { normalizeShop(bad); });
    }

    label("ids are one spelling, for the right type");
    assert(normalizeGid(" 1001 ", "Order", "orderId") === "gid://shopify/Order/1001", "number");
    assert(normalizeGid("gid://shopify/Order/1001", "Order", "orderId") === "gid://shopify/Order/1001", "gid");
    for (const bad of ["", "abc", "gid://shopify/Product/1", "gid://shopify/Order/1?x=1", "gid://shopify/Order/", "gid://shopify/Order/007", "0", "123456789012345678901"]) {
        expectCode("invalid_id", () => { normalizeGid(bad, "Order", "orderId"); });
    }

    label("emails");
    assert(normalizeEmail("  Buyer@Example.COM ") === "buyer@example.com", "email normalized");
    for (const bad of ["", "buyer", "@example.com", "buyer@", "a@b@c", "a b@c.com"]) {
        expectCode("invalid_email", () => { normalizeEmail(bad); });
    }

    label("amounts stay exact: anything a decimal string could not carry is refused");
    assert(normalizeAmount(19.99, "price") === 19.99, "two places");
    assert(moneyText(25.5) === "25.5", "decimal text");
    for (const bad of [0, -1, 0.1 + 0.2, 1.1234, 2000000000]) {
        expectCode("invalid_amount", () => { normalizeAmount(bad, "price"); });
    }

    label("stock changes are whole; a change is not zero, a level may be");
    assert(normalizeDelta(-3) === -3, "negative allowed");
    assert(normalizeStockLevel(0) === 0, "a level may be zero");
    for (const bad of [0, 1.5, 2000000]) {
        expectCode("invalid_delta", () => { normalizeDelta(bad); });
    }
    expectCode("invalid_quantity", () => { normalizeStockLevel(-1); });

    label("dates, links, codes and tags");
    assert(normalizeInstant("2026-10-01T00:00:00Z", "at").startsWith("2026-10-01T00:00:00"), "instant kept");
    expectCode("invalid_date", () => { normalizeInstant("last week", "at"); });
    assert(normalizeHttpsUrl(" https://track.example/1 ") === "https://track.example/1", "link trimmed");
    expectCode("invalid_url", () => { normalizeHttpsUrl("javascript:alert(1)"); });
    assert(normalizeCode(" welcome10 ") === "WELCOME10", "a code has one spelling");
    expectCode("invalid_tag", () => { normalizeTag("a,b"); });

    label("page windows");
    assert(pageWindow(undefined, undefined).first === 50, "default page size");
    assert(pageWindow(100, "c").after === "c", "cursor kept");
    assert(inventoryWindow(undefined, undefined).first === 15, "the inventory default fits its smaller page");
    expectCode("invalid_limit", () => { pageWindow(101, undefined); });
    expectCode("invalid_limit", () => { pageWindow(1.5, undefined); });
    expectCode("invalid_limit", () => { inventoryWindow(16, undefined); });
    expectCode("invalid_cursor", () => { pageWindow(undefined, ""); });

    label("the field list a policy filters on is sorted and names what is set");
    const fields = productFields(true, false, true, false, false, true);
    assert(JSON.stringify(fields) === "[\"productType\",\"status\",\"title\"]", "got " + JSON.stringify(fields));
    expectCode("empty_update", () => { productFields(false, false, false, false, false, false); });

    label("inventory writes always send changeFromQuantity");
    const set = JSON.stringify(buildInventorySet("i", "l", 7, 4, "correction"));
    assert(set.includes("\"changeFromQuantity\":4"), "a set compares, got " + set);
    const adjust = JSON.stringify(buildInventoryAdjust("i", "l", -2, "damaged"));
    assert(adjust.includes("\"changeFromQuantity\":null"), "an adjustment sends an explicit null, got " + adjust);

    label("a percentage goes to Shopify as a fraction");
    const percent = JSON.stringify(buildDiscount("t", "C", 25, null, "s", null, null, false, null));
    assert(percent.includes("\"percentage\":0.25"), "got " + percent);
    const fixed = JSON.stringify(buildDiscount("t", "C", null, 10, "s", null, 5, true, 50));
    assert(fixed.includes("\"amount\":\"10\"") && fixed.includes("\"usageLimit\":5") && fixed.includes("\"greaterThanOrEqualToSubtotal\":\"50\""), "got " + fixed);

    label("response mappers");
    assert(decimalOrNull("12.50") === 12.5, "decimal read");
    assert(decimalOrNull("") === null, "empty is null");
    assert(decimalOrNull(undefined) === null, "absent is null");
    assert(requireAmount("0", "x") === 0, "a real zero is kept");
    expectCode("unexpected_response", () => { requireAmount(undefined, "total"); });
    expectCode("unexpected_response", () => { requireAmount("n/a", "total"); });
    assert(orderFrom({ id: "1" }).total === null, "an absent total is null");
    assert(nextCursorFrom(null) === null, "no paging");
    assert(nextCursorFrom({ hasNextPage: false, endCursor: "n" }) === null, "the last page has no cursor");
    assert(nextCursorFrom({ hasNextPage: true, endCursor: "n" }) === "n", "next cursor");

    label("notes and tags keep what is there");
    assert(appendNote(null, "new") === "new", "first note");
    assert(appendNote("old", "new") === "old\nnew", "a note is added to");
    assert(JSON.stringify(mergeTags(["VIP", "a"], ["vip", "b"])) === "[\"VIP\",\"a\",\"b\"]", "tags merge without repeats");

    label("a suggested refund sums its transactions");
    const suggested = suggestedRefundFrom({
        suggestedTransactions: [
            { gateway: "shopify_payments", amountSet: { presentmentMoney: { amount: "10.10", currencyCode: "EUR" } }, parentTransaction: { id: "gid://shopify/OrderTransaction/1" } },
            { gateway: "gift_card", amountSet: { presentmentMoney: { amount: "0.20", currencyCode: "EUR" } } },
        ],
    });
    assert(suggested.amount === 10.3, "summed without binary noise, got " + suggested.amount.toString());
    assert(suggested.currency === "EUR", "currency from the transactions");
    assert(suggested.transactions[0].amountText === "10.10", "the exact text is kept for the request");
    expectCode("unexpected_response", () => { suggestedRefundFrom({ suggestedTransactions: [{ gateway: "x" }] }); });
    expectCode("unexpected_response", () => { suggestedRefundFrom({ suggestedTransactions: [{ amountSet: { presentmentMoney: { amount: "1" } } }] }); });

    label("fulfillment orders must be open on this order and at one location");
    const orders = [
        { id: "a", status: "OPEN", assignedLocation: { location: { id: LOCATION_A } } },
        { id: "b", status: "IN_PROGRESS", assignedLocation: { location: { id: LOCATION_B } } },
        { id: "c", status: "CLOSED", assignedLocation: { location: { id: LOCATION_A } } },
    ];
    assert(chooseFulfillmentOrders(orders, ["a"])[0] === "a", "a chosen open one");
    expectCode("several_locations", () => { chooseFulfillmentOrders(orders, []); });
    expectCode("several_locations", () => { chooseFulfillmentOrders(orders, ["a", "b"]); });
    expectCode("fulfillment_order_not_open", () => { chooseFulfillmentOrders(orders, ["c"]); });
    expectCode("fulfillment_order_not_open", () => { chooseFulfillmentOrders(orders, ["someone-elses"]); });
    expectCode("nothing_to_fulfill", () => { chooseFulfillmentOrders([orders[2]], []); });

    label("stock must be readable to compare against");
    assert(availableFrom({ inventoryLevel: { quantities: [{ name: "available", quantity: 3 }] } }) === 3, "read");
    expectCode("not_stocked", () => { availableFrom({ inventoryLevel: null }); });

    label("errors");
    expectCode("TAKEN", () => { requireNoUserErrors([{ field: ["input", "code"], message: "taken", code: "TAKEN" }]); });
    expectCode("user_error", () => { requireNoUserErrors([{ message: "no" }]); });
    requireNoUserErrors([]);
    assert(graphQlFailure({}) === null, "no errors");
    const denied = graphQlFailure({ errors: [{ message: "denied", extensions: { code: "ACCESS_DENIED" } }] });
    assert(denied !== null && denied.code === "ACCESS_DENIED" && !denied.retryable, "a GraphQL error keeps its code");
    const throttled = graphQlFailure({
        errors: [{ message: "Throttled", extensions: { code: "THROTTLED" } }],
        extensions: { cost: { requestedQueryCost: 202, throttleStatus: { currentlyAvailable: 2, restoreRate: 100 } } },
    });
    assert(throttled !== null && throttled.code === "throttled" && throttled.retryable && throttled.retryAfterSeconds === 2, "a throttle says how long");
    assert(throttleWait(null) === null, "no cost, no wait");
    const http = httpFailure(401, "Unauthorized", "{\"errors\":\"Invalid API key\"}", null);
    assert(http.code === "unauthorized" && http.message.includes("Invalid API key"), "got " + http.message);
    assert(httpFailure(503, "Unavailable", "<html>", null).code === "server_error", "a body that is not JSON still gives a code");
    assert(httpFailure(429, "", "", 2).retryable, "429 can be retried");
    assert(retryAfterFrom("2.0") === 2, "seconds");
    assert(retryAfterFrom("") === null, "absent");

    label("invalid input stops before any secret or request");
    expectCode("invalid_id", () => { getOrder("abc"); });
    expectCode("invalid_id", () => { getProduct("abc"); });
    expectCode("invalid_id", () => { getCustomer("abc"); });
    expectCode("invalid_id", () => { listFulfillmentOrders("abc"); });
    expectCode("invalid_id", () => { getInventoryLevels("abc"); });
    expectCode("invalid_limit", () => { getInventoryLevels("1", { first: 16 }); });
    expectCode("invalid_text", () => { addOrderNote("1", " "); });
    expectCode("invalid_tags", () => { addOrderTags("1", []); });
    expectCode("invalid_reason", () => { cancelOrder("1", { reason: "BORED", restock: false, refund: false }); });
    expectCode("nothing_to_refund", () => { suggestRefund("1", {}); });
    expectCode("nothing_to_refund", () => { refundOrder("1", { idempotencyKey: KEY }); });
    expectCode("invalid_text", () => { refundOrder("1", { shipping: true, idempotencyKey: " " }); });
    expectCode("restock_needs_line_items", () => { refundOrder("1", { shipping: true, restockType: "RETURN", locationId: "1", idempotencyKey: KEY }); });
    expectCode("restock_needs_location", () => { refundOrder("1", { lineItems: [{ lineItemId: "2", quantity: 1 }], restockType: "RETURN", idempotencyKey: KEY }); });
    expectCode("invalid_restockType", () => { refundOrder("1", { shipping: true, restockType: "LEGACY_RESTOCK", idempotencyKey: KEY }); });
    expectCode("invalid_line_items", () => { createFulfillment("1", { fulfillmentOrderIds: [] }); });
    expectCode("empty_update", () => { updateTracking("1", {}); });
    expectCode("invalid_reason", () => { holdFulfillmentOrder("1", "BORED"); });
    expectCode("empty_update", () => { updateProduct("1", {}); });
    expectCode("invalid_status", () => { updateProduct("1", { status: "GONE" }); });
    expectCode("invalid_line_items", () => { updateVariantPrices("1", []); });
    expectCode("invalid_amount", () => { updateVariantPrices("1", [{ variantId: "2", price: -1 }]); });
    expectCode("invalid_delta", () => { adjustInventory("1", "2", 0, "correction", KEY); });
    expectCode("invalid_quantity", () => { setInventory("1", "2", -1, "correction", KEY); });
    expectCode("invalid_limit", () => { searchOrders({ first: 500 }); });
    expectCode("invalid_limit", () => { searchProducts({ first: 500 }); });
    expectCode("invalid_limit", () => { searchCustomers({ first: 500 }); });
    expectCode("invalid_limit", () => { listAbandonedCheckouts({ first: 500 }); });
    expectCode("invalid_sortKey", () => { searchOrders({ sortKey: "NOPE" }); });
    expectCode("invalid_discount", () => { createDiscountCode({ title: "t", code: "C", startsAt: "2026-10-01T00:00:00Z" }); });
    expectCode("invalid_discount", () => { createDiscountCode({ title: "t", code: "C", percentage: 10, amount: 5, startsAt: "2026-10-01T00:00:00Z" }); });
    expectCode("invalid_percentage", () => { createDiscountCode({ title: "t", code: "C", percentage: 101, startsAt: "2026-10-01T00:00:00Z" }); });
    expectCode("invalid_usage_limit", () => { createDiscountCode({ title: "t", code: "C", percentage: 10, usageLimit: 0, startsAt: "2026-10-01T00:00:00Z" }); });
    expectCode("invalid_line_items", () => { createDraftOrder({ lineItems: [] }); });
    expectCode("invalid_percentage", () => { createDraftOrder({ lineItems: [{ variantId: "1", quantity: 1 }], discount: { percentage: 0 } }); });
    expectCode("invalid_email", () => { sendDraftOrderInvoice("1", "x"); });

    if (secrets.get("SHOPIFY_SHOP") !== undefined) {
        label("skip: SHOPIFY_SHOP is bound");
        return;
    }
    label("valid input stops at the missing store");
    expectCode("missing_shop", () => { getShop(); });
    expectCode("missing_shop", () => { searchOrders(); });
    const error = expectException(() => { getOrder("1"); }, "ShopifyError");
    assert(error.message.includes("SHOPIFY_SHOP"), "the error names the secret");
}
