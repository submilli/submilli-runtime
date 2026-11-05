// Run through scripts/policy.test.mjs. See wrong-shop.yaml.
import { getOrder, searchOrders } from "@submilli/shopify-admin";

function deniedAtMain(capability: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    }
    assert(outcome === "main denied at " + capability, "expected a denial at " + capability + ", got " + outcome);
}

function main(): string {
    deniedAtMain("shopify.com/orders.search", () => { searchOrders(); });
    deniedAtMain("shopify.com/orders.get", () => { getOrder("1001"); });
    return "ok";
}
