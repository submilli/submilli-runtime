// MOCK: see money.yaml. The fake answers with a small order (1), a large order (2), and one in
// another currency (3). The script that runs this also asserts which writes reached the fake.
import { ShopifyError, adjustInventory, cancelOrder, getOrder, refundOrder, setInventory, updateTracking } from "@submilli/shopify-admin";

const KEY = "3f5b6ebf-143c-4da5-8d0f-fb8553bfd85d";
const LOCATION = "gid://shopify/Location/1";
const ITEM = "gid://shopify/InventoryItem/1";

function deniedAt(capability: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === "main denied at " + capability, "expected a denial at " + capability + ", got " + outcome);
}

function rejectedAs(code: string, action: () => void): void {
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

function refundOne(orderId: string): void {
    refundOrder(orderId, { lineItems: [{ lineItemId: "9", quantity: 1 }], idempotencyKey: KEY });
}

function main(): string {
    // Under the cap: reaches the fake. The amount comes from Shopify, not the caller.
    refundOne("1");
    deniedAt("shopify.com/refunds.create", () => { refundOne("2"); });
    deniedAt("shopify.com/refunds.create", () => { refundOne("3"); });
    deniedAt("shopify.com/refunds.create", () => {
        refundOrder("1", { lineItems: [{ lineItemId: "9", quantity: 1 }], restockType: "RETURN", locationId: "1", idempotencyKey: KEY });
    });

    // The total comes from Shopify.
    cancelOrder("1", { reason: "CUSTOMER", restock: false, refund: false });
    deniedAt("shopify.com/orders.cancel", () => { cancelOrder("2", { reason: "CUSTOMER", restock: false, refund: false }); });

    // The order is resolved from the fulfillment: this one belongs to order 2.
    updateTracking("1", { number: "1Z" });
    deniedAt("shopify.com/fulfillments.updateTracking", () => { updateTracking("2", { number: "1Z" }); });

    // The fake holds 10 available, so the delta is read from Shopify, not assumed.
    setInventory(ITEM, LOCATION, 12, "correction", KEY);
    deniedAt("shopify.com/inventory.set", () => { setInventory(ITEM, LOCATION, 16, "correction", KEY); });

    // The explicit null reaches the wire: the script reads this body.
    adjustInventory(ITEM, LOCATION, 1, "correction", KEY);

    // Shopify's HTTP errors hold a string in `errors`, which must not break the error path.
    rejectedAs("unauthorized", () => { getOrder("401"); });
    return "ok";
}
