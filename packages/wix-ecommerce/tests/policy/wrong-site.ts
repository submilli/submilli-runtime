// Run through scripts/policy.test.mjs. See wrong-site.yaml.
import { getOrder, searchOrders } from "@submilli/wix-ecommerce";

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
    deniedAtMain("wix.com/orders.search", () => { searchOrders(); });
    deniedAtMain("wix.com/orders.get", () => { getOrder("aaaaaaaa-0000-4000-8000-000000000001"); });
    return "ok";
}
