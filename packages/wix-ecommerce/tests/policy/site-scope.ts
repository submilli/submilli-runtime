// Run through scripts/policy.test.mjs, not the allow-all build test runner.
import {
    addOrderNote,
    adjustInventory,
    createCart,
    createFulfillment,
    findContactByEmail,
    getOrder,
    getProduct,
    refundOrder,
    searchOrders,
    searchProducts,
    updateProduct,
} from "@submilli/wix-ecommerce";

const ORDER = "aaaaaaaa-0000-4000-8000-000000000001";
const PAYMENT = "bbbbbbbb-0000-4000-8000-000000000001";
const PRODUCT = "eeeeeeee-0000-4000-8000-000000000001";
const VARIANT = "ffffffff-0000-4000-8000-000000000001";

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
    // This denial proves the business check passed, without reading a key or calling Wix.
    deniedAt("secrets.get", "@submilli/wix-ecommerce", action);
}

function main(): string {
    // Reads on the pinned site.
    reachesCredentialBoundary(() => { searchOrders(); });
    reachesCredentialBoundary(() => { getOrder(ORDER); });

    // No rule, no operation.
    deniedAt("wix.com/orders.note", "main", () => { addOrderNote(ORDER, "x"); });
    deniedAt("wix.com/products.get", "main", () => { getProduct(PRODUCT); });

    // A write with an argument a merchant caps.
    reachesCredentialBoundary(() => { searchProducts({ text: "mug" }); });
    deniedAt("wix.com/products.search", "main", () => { searchProducts({ includeHidden: true }); });

    reachesCredentialBoundary(() => { updateProduct(PRODUCT, { name: "New" }); });
    deniedAt("wix.com/products.update", "main", () => { updateProduct(PRODUCT, { name: "New", price: 1 }); });

    reachesCredentialBoundary(() => { adjustInventory(PRODUCT, VARIANT, 10); });
    reachesCredentialBoundary(() => { adjustInventory(PRODUCT, VARIANT, -50); });
    deniedAt("wix.com/inventory.adjust", "main", () => { adjustInventory(PRODUCT, VARIANT, -51); });
    deniedAt("wix.com/inventory.adjust", "main", () => { adjustInventory(PRODUCT, VARIANT, 501); });

    reachesCredentialBoundary(() => { createCart([{ productId: PRODUCT, quantity: 10 }]); });
    deniedAt("wix.com/carts.create", "main", () => { createCart([{ productId: PRODUCT, quantity: 6 }, { productId: PRODUCT, quantity: 5 }]); });

    reachesCredentialBoundary(() => { createFulfillment(ORDER, { tracking: { number: "1", provider: "ups" }, lineItems: [{ lineItemId: PAYMENT }] }); });
    deniedAt("wix.com/fulfillments.create", "main", () => { createFulfillment(ORDER, { tracking: { number: "1", provider: "dhl" } }); });

    // The email is normalized before the check.
    reachesCredentialBoundary(() => { findContactByEmail(" BUYER@example.com "); });
    deniedAt("wix.com/contacts.find", "main", () => { findContactByEmail("other@example.com"); });

    // A refund resolves the order first, so with no key it stops before its check. That
    // path runs in tests/fake-host, against a fake Wix.
    reachesCredentialBoundary(() => { refundOrder(ORDER, { paymentId: PAYMENT, amount: 1 }); });
    return "ok";
}
