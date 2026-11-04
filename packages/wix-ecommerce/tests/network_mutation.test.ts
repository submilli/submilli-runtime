// Live writes. Runs only with WIX_DISPOSABLE_ORDER_ID, the id of an order on a
// disposable test site: it adds a note to that order. Never point this at a real
// store. Skipped by `--skip-network` and when the variable is not bound.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { addOrderNote, getOrder } from "@submilli/wix-ecommerce";

function main(): void {
    const orderId = secrets.get("WIX_DISPOSABLE_ORDER_ID");
    if (orderId === undefined) {
        label("skip: WIX_DISPOSABLE_ORDER_ID is not bound");
        return;
    }
    label("a note can be added to the disposable order");
    assert(getOrder(orderId) !== null, "the disposable order exists");
    addOrderNote(orderId, "Package network test");
}
