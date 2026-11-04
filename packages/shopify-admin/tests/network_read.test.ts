// Live reads against the store bound as SHOPIFY_ACCESS_TOKEN and SHOPIFY_SHOP.
// Read-only: nothing here writes. Skipped by `--skip-network` and when a secret is not bound.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getOrder, getProduct, getShop, listAbandonedCheckouts, listFulfillmentOrders, listLocations, searchCustomers, searchOrders, searchProducts } from "@submilli/shopify-admin";

const ABSENT = "999999999999";

function main(): void {
    if (secrets.get("SHOPIFY_ACCESS_TOKEN") === undefined || secrets.get("SHOPIFY_SHOP") === undefined) {
        label("skip: SHOPIFY_ACCESS_TOKEN and SHOPIFY_SHOP are not bound");
        return;
    }

    label("the store");
    assert(getShop().domain.endsWith(".myshopify.com"), "the store answers");
    listLocations();

    label("orders");
    assert(getOrder(ABSENT) === null, "an absent order is null");
    for (const order of searchOrders({ first: 1 }).items) {
        const found = getOrder(order.id);
        assert(found !== null && found.id === order.id, "the order reads back");
        listFulfillmentOrders(order.id);
    }

    label("catalog");
    assert(getProduct(ABSENT) === null, "an absent product is null");
    searchProducts({ first: 1 });

    label("customers and abandoned checkouts");
    searchCustomers({ first: 1 });
    listAbandonedCheckouts({ first: 1 });
}
