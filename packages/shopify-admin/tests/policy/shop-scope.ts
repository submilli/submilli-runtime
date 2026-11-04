// Run through scripts/policy.test.mjs, not the allow-all build test runner.
import {
    addOrderNote,
    addOrderTags,
    adjustInventory,
    createDiscountCode,
    createDraftOrder,
    getOrder,
    getProduct,
    refundOrder,
    searchOrders,
    sendDraftOrderInvoice,
    updateProduct,
    updateVariantPrices,
} from "@submilli/shopify-admin";

const KEY = "3f5b6ebf-143c-4da5-8d0f-fb8553bfd85d";
const STARTS = "2026-10-01T00:00:00Z";

function deniedAt(capability: string, caller: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    const expected = caller + " denied at " + capability;
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function reachesCredentialBoundary(action: () => void): void {
    // This denial proves the business check passed, without reading a token or calling Shopify.
    deniedAt("secrets.get", "@submilli/shopify-admin", action);
}

function main(): string {
    // Reads on the pinned store.
    reachesCredentialBoundary(() => { searchOrders(); });
    reachesCredentialBoundary(() => { getOrder("1001"); });

    // No rule, no operation.
    deniedAt("shopify.com/products.get", "main", () => { getProduct("1"); });

    // One capability, two operations, told apart by the field they set.
    reachesCredentialBoundary(() => { addOrderNote("1001", "x"); });
    deniedAt("shopify.com/orders.update", "main", () => { addOrderTags("1001", ["x"]); });

    reachesCredentialBoundary(() => { updateProduct("1", { title: "New" }); });
    deniedAt("shopify.com/products.update", "main", () => { updateProduct("1", { title: "New", status: "ARCHIVED" }); });

    // The highest price in the batch is what is compared.
    reachesCredentialBoundary(() => { updateVariantPrices("1", [{ variantId: "2", price: 100 }]); });
    deniedAt("shopify.com/variants.updatePrices", "main", () => { updateVariantPrices("1", [{ variantId: "2", price: 1 }, { variantId: "3", price: 100.01 }]); });

    reachesCredentialBoundary(() => { adjustInventory("1", "2", 10, "correction", KEY); });
    reachesCredentialBoundary(() => { adjustInventory("1", "2", -50, "correction", KEY); });
    deniedAt("shopify.com/inventory.adjust", "main", () => { adjustInventory("1", "2", -51, "correction", KEY); });
    deniedAt("shopify.com/inventory.adjust", "main", () => { adjustInventory("1", "2", 501, "correction", KEY); });

    reachesCredentialBoundary(() => { createDiscountCode({ title: "t", code: "c", percentage: 20, usageLimit: 100, startsAt: STARTS }); });
    deniedAt("shopify.com/discounts.create", "main", () => { createDiscountCode({ title: "t", code: "c", percentage: 21, usageLimit: 100, startsAt: STARTS }); });
    // A code with no limit reads as 0, so a floor on usageLimit refuses it.
    deniedAt("shopify.com/discounts.create", "main", () => { createDiscountCode({ title: "t", code: "c", percentage: 10, startsAt: STARTS }); });
    // A fixed amount reads as percentage 0, so the rule must cap amount too.
    deniedAt("shopify.com/discounts.create", "main", () => { createDiscountCode({ title: "t", code: "c", amount: 1000, usageLimit: 1, startsAt: STARTS }); });

    reachesCredentialBoundary(() => { createDraftOrder({ lineItems: [{ variantId: "1", quantity: 10 }] }); });
    deniedAt("shopify.com/draftOrders.create", "main", () => { createDraftOrder({ lineItems: [{ variantId: "1", quantity: 6 }, { variantId: "2", quantity: 5 }] }); });
    // A draft order must not be a way around the discount cap.
    deniedAt("shopify.com/draftOrders.create", "main", () => { createDraftOrder({ lineItems: [{ variantId: "1", quantity: 1 }], discount: { percentage: 100 } }); });

    // The address is normalized before the check.
    reachesCredentialBoundary(() => { sendDraftOrderInvoice("1", " BUYER@example.com "); });
    deniedAt("shopify.com/draftOrders.sendInvoice", "main", () => { sendDraftOrderInvoice("1", "other@example.com"); });

    // A refund asks Shopify first, so with no token it stops before its check. That
    // path runs in tests/fake-host, against a fake Shopify.
    reachesCredentialBoundary(() => { refundOrder("1001", { shipping: true, idempotencyKey: KEY }); });
    return "ok";
}
