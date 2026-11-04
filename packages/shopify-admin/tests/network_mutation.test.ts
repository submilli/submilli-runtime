// Live writes. Runs only with SHOPIFY_DISPOSABLE_ORDER_ID, the id of an order on a
// disposable development store: it adds a note and a tag to that order. Never point
// this at a real store. Skipped by `--skip-network` and when the variable is not bound.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { addOrderNote, addOrderTags, getOrder } from "@submilli/shopify-admin";

function main(): void {
    const orderId = secrets.get("SHOPIFY_DISPOSABLE_ORDER_ID");
    if (orderId === undefined) {
        label("skip: SHOPIFY_DISPOSABLE_ORDER_ID is not bound");
        return;
    }
    label("a note and a tag can be added to the disposable order");
    assert(getOrder(orderId) !== null, "the disposable order exists");
    addOrderNote(orderId, "Package network test");
    assert(addOrderTags(orderId, ["package-network-test"]).includes("package-network-test"), "the tag is there");
}
