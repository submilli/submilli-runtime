// Live reads against the Wix site bound as WIX_API_KEY and WIX_SITE_ID. Read-only:
// nothing here writes. Skipped by `--skip-network` and when a secret is not bound.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getCatalogVersion, getOrder, listAbandonedCheckouts, listFulfillments, listPayments, searchOrders, searchProducts } from "@submilli/wix-ecommerce";

const ABSENT = "00000000-0000-4000-8000-000000000000";

function main(): void {
    if (secrets.get("WIX_API_KEY") === undefined || secrets.get("WIX_SITE_ID") === undefined) {
        label("skip: WIX_API_KEY and WIX_SITE_ID are not bound");
        return;
    }

    label("orders");
    assert(getOrder(ABSENT) === null, "an absent order is null");
    const orders = searchOrders({}, { limit: 1 });
    for (const order of orders.items) {
        const found = getOrder(order.id);
        assert(found !== null && found.id === order.id, "the order reads back");
        listFulfillments(order.id);
        listPayments(order.id);
    }

    label("abandoned checkouts");
    listAbandonedCheckouts({}, { limit: 1 });

    label("catalog");
    const version = getCatalogVersion();
    assert(version === "V1_CATALOG" || version === "V3_CATALOG" || version === "STORES_NOT_INSTALLED", "got " + version);
    if (version !== "STORES_NOT_INSTALLED") searchProducts({}, { limit: 1 });
}
