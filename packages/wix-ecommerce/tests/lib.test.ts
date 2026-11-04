// Offline tests. The normalizers, builders and mappers live in the package's
// internal modules, so these tests import them from `../src/`. The operations
// section checks that each operation validates its input before any request:
// invalid input fails with a local WixError (status 0). The last section stops
// at the missing site id, and is skipped when WIX_SITE_ID is bound, so this file
// never sends a request.
import { expectException, label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    WixError,
    addOrderNote,
    adjustInventory,
    cancelOrder,
    createCart,
    createFulfillment,
    findContactByEmail,
    getCatalogVersion,
    getCheckoutUrl,
    getContact,
    getInventory,
    getOrder,
    getProduct,
    listAbandonedCheckouts,
    listFulfillments,
    listPayments,
    refundOrder,
    searchOrders,
    searchProducts,
    updateProduct,
    updateTracking,
} from "@submilli/wix-ecommerce";
import {
    buildAdjustV1,
    buildCancelBody,
    buildOrderSearch,
    buildRefundBody,
    decimalText,
    normalizeAmount,
    normalizeDelta,
    normalizeEmail,
    normalizeId,
    normalizeInstant,
    normalizeTrackingLink,
    offsetFrom,
    pageWindow,
    productFields,
} from "../src/request";
import { decimalOrNull, failureFrom, nextCursorFrom, orderFrom, paymentsFrom, requireAmount, retryAfterFrom } from "../src/response";

const ORDER = "aaaaaaaa-0000-4000-8000-000000000001";
const PAYMENT = "bbbbbbbb-0000-4000-8000-000000000001";
const PRODUCT = "eeeeeeee-0000-4000-8000-000000000001";
const VARIANT = "ffffffff-0000-4000-8000-000000000001";

function expectCode(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: WixError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function main(): void {
    label("ids are trimmed and lowercased, so a filter sees one spelling");
    assert(normalizeId(" AAAAAAAA-0000-4000-8000-000000000001 ", "id") === ORDER, "id normalized");
    for (const bad of ["", "1", "aaaaaaaa00004000800000000000000100000", "gggggggg-0000-4000-8000-000000000001", "../../etc/passwd/xxxxxxxxxxxxxxxxxxx"]) {
        expectCode("invalid_id", () => { normalizeId(bad, "id"); });
    }

    label("emails");
    assert(normalizeEmail("  Buyer@Example.COM ") === "buyer@example.com", "email normalized");
    for (const bad of ["", "buyer", "@example.com", "buyer@", "a@b@c", "a b@c.com"]) {
        expectCode("invalid_email", () => { normalizeEmail(bad); });
    }

    label("amounts stay exact: anything a decimal string could not carry is refused");
    assert(normalizeAmount(19.99, "amount") === 19.99, "two places");
    assert(normalizeAmount(1.125, "amount") === 1.125, "three places");
    assert(decimalText(25.5) === "25.5", "decimal text");
    for (const bad of [0, -1, 0.1 + 0.2, 1.1234, 2000000000]) {
        expectCode("invalid_amount", () => { normalizeAmount(bad, "amount"); });
    }

    label("stock changes are whole and not zero");
    assert(normalizeDelta(-3) === -3, "negative allowed");
    for (const bad of [0, 1.5, 200000, -200000]) {
        expectCode("invalid_delta", () => { normalizeDelta(bad); });
    }

    label("dates and links");
    assert(normalizeInstant("2026-10-01T00:00:00Z", "at").startsWith("2026-10-01T00:00:00"), "instant kept");
    expectCode("invalid_date", () => { normalizeInstant("last week", "at"); });
    assert(normalizeTrackingLink(" https://track.example/1 ") === "https://track.example/1", "link trimmed");
    expectCode("invalid_tracking_link", () => { normalizeTrackingLink("javascript:alert(1)"); });

    label("page windows");
    assert(pageWindow({}).limit === 50, "default page size");
    assert(pageWindow({ limit: 100, cursor: "c" }).cursor === "c", "cursor kept");
    expectCode("invalid_limit", () => { pageWindow({ limit: 101 }); });
    expectCode("invalid_limit", () => { pageWindow({ limit: 1.5 }); });
    expectCode("invalid_cursor", () => { pageWindow({ cursor: "" }); });
    assert(offsetFrom("25") === 25, "digits are an offset");
    expectCode("invalid_cursor", () => { offsetFrom("abc"); });

    label("the field list a policy filters on is sorted and names what is set");
    const fields = productFields(true, false, true, false, true);
    assert(JSON.stringify(fields) === "[\"name\",\"price\",\"visible\"]", "got " + JSON.stringify(fields));
    expectCode("empty_update", () => { productFields(false, false, false, false, false); });

    label("request builders");
    const search = JSON.stringify(buildOrderSearch({ status: "approved" }, { limit: 5, cursor: null }));
    assert(search === "{\"search\":{\"cursorPaging\":{\"limit\":5},\"filter\":{\"status\":\"APPROVED\"}}}", "got " + search);
    const later = JSON.stringify(buildOrderSearch({ status: "approved" }, { limit: 5, cursor: "c" }));
    assert(later === "{\"search\":{\"cursorPaging\":{\"cursor\":\"c\",\"limit\":5}}}", "a cursor drops the filter, got " + later);
    const cancel = JSON.stringify(buildCancelBody(true, false, null));
    assert(cancel === "{\"restockAllItems\":true,\"sendOrderCanceledEmail\":false}", "got " + cancel);
    const refund = JSON.stringify(buildRefundBody(ORDER, PAYMENT, 5, [], false, false, false, null, null));
    assert(refund.includes("\"amount\":{\"amount\":\"5\"}"), "amount is a string, got " + refund);
    assert(!refund.includes("sideEffects"), "no side effects unless asked for");
    assert(JSON.stringify(buildAdjustV1(PRODUCT, VARIANT, -2)).includes("\"decrementBy\":2"), "a negative delta decrements a positive number");

    label("response mappers");
    assert(decimalOrNull("12.50") === 12.5, "decimal read");
    assert(decimalOrNull("") === null, "empty is null");
    assert(decimalOrNull(undefined) === null, "absent is null");
    assert(requireAmount("0", "x") === 0, "a real zero is kept");
    expectCode("unexpected_response", () => { requireAmount(undefined, "total"); });
    expectCode("unexpected_response", () => { requireAmount("n/a", "total"); });
    assert(orderFrom({ id: ORDER, number: 7 }).number === "7", "a number reads as text");
    assert(orderFrom({ number: "7" }).total === null, "an absent total is null");
    assert(nextCursorFrom(null) === null, "no paging");
    assert(nextCursorFrom({ hasNext: false, cursors: { next: "n" } }) === null, "the last page has no cursor");
    assert(nextCursorFrom({ hasNext: true, cursors: { next: "n" } }) === "n", "next cursor");
    const payments = paymentsFrom({
        orderTransactions: {
            payments: [{ id: PAYMENT, amount: { amount: "10" } }],
            refunds: [
                { transactions: [{ paymentId: PAYMENT, amount: { amount: "4" }, refundStatus: "PENDING" }] },
                { transactions: [{ paymentId: PAYMENT, amount: { amount: "6" }, refundStatus: "FAILED" }] },
            ],
        },
    });
    assert(payments[0].refundable === 6, "pending counts, failed does not");
    expectCode("unexpected_response", () => { paymentsFrom({ orderTransactions: { payments: [{ id: PAYMENT }] } }); });

    label("errors");
    const coded = failureFrom(400, "Bad Request", "{\"message\":\"nope\",\"details\":{\"applicationError\":{\"code\":\"BAD\"}}}");
    assert(coded.code === "BAD" && coded.message === "Wix 400 BAD: nope", "got " + coded.message);
    const plain = failureFrom(500, "Server Error", "<html>");
    assert(plain.code === "server_error", "a body that is not JSON still gives a code");
    assert(failureFrom(429, "", "").code === "rate_limited", "429");
    assert(retryAfterFrom("30") === 30, "seconds");
    assert(retryAfterFrom("") === null, "absent");
    assert(retryAfterFrom("Wed, 21 Oct 2026 07:28:00 GMT") === null, "not digits");

    label("invalid input stops before any secret or request");
    expectCode("invalid_id", () => { getOrder("1"); });
    expectCode("invalid_id", () => { listFulfillments("1"); });
    expectCode("invalid_id", () => { listPayments("1"); });
    expectCode("invalid_id", () => { getProduct("1"); });
    expectCode("invalid_id", () => { getInventory("1"); });
    expectCode("invalid_id", () => { getContact("1"); });
    expectCode("invalid_id", () => { getCheckoutUrl("1"); });
    expectCode("invalid_text", () => { addOrderNote(ORDER, " "); });
    expectCode("message_without_notification", () => { cancelOrder(ORDER, { message: "x" }); });
    expectCode("invalid_limit", () => { searchOrders({}, { limit: 500 }); });
    expectCode("invalid_limit", () => { listAbandonedCheckouts({}, { limit: 500 }); });
    expectCode("invalid_limit", () => { searchProducts({}, { limit: 500 }); });
    expectCode("invalid_email", () => { findContactByEmail("x"); });
    expectCode("invalid_amount", () => { refundOrder(ORDER, { paymentId: PAYMENT, amount: -1 }); });
    expectCode("restock_needs_line_items", () => { refundOrder(ORDER, { paymentId: PAYMENT, amount: 1, restock: true }); });
    expectCode("invalid_quantity", () => { refundOrder(ORDER, { paymentId: PAYMENT, amount: 1, lineItems: [{ lineItemId: PAYMENT }] }); });
    expectCode("invalid_delta", () => { adjustInventory(PRODUCT, VARIANT, 0); });
    expectCode("empty_update", () => { updateProduct(PRODUCT, {}); });
    expectCode("empty_update", () => { updateTracking(ORDER, PAYMENT, {}); });
    expectCode("invalid_line_items", () => { createCart([]); });
    expectCode("invalid_text", () => { createFulfillment(ORDER, { tracking: { number: " ", provider: "ups" } }); });

    if (secrets.get("WIX_SITE_ID") !== undefined) {
        label("skip: WIX_SITE_ID is bound");
        return;
    }
    label("valid input stops at the missing site id");
    expectCode("missing_site_id", () => { getCatalogVersion(); });
    expectCode("missing_site_id", () => { searchOrders(); });
    const error = expectException(() => { getOrder(ORDER); }, "WixError");
    assert(error.message.includes("WIX_SITE_ID"), "the error names the secret");
}
