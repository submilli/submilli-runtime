# Shopify Admin package guide

Use `@submilli/shopify-admin` to work with one Shopify store: orders, refunds,
fulfillment, products, inventory, customers, abandoned checkouts, discount codes
and draft orders. Credentials and the store are supplied internally; never pass a
token or a shop name. Every call is synchronous.

- An id is a number or a `gid://shopify/Type/123` id, for the right type. Results
  hold the `gid://` form.
- Money is a number in the buyer's currency.
- Lists return `Page<T>`: `items` and `nextCursor`. Pass `nextCursor` as `after`
  for the next page, with the same `query`, while it is not null. Page size is
  1-100, default 50.
- `query` is Shopify's search syntax, such as `status:active vendor:Acme`.
- A single missing resource returns `null`.
- Errors are `ShopifyError`, with `code`, `status`, `retryable`,
  `retryAfterSeconds` and `field`. `status` 0 means the package refused the call
  itself. `status` 200 means Shopify answered and refused it. After `throttled`,
  stop and report; a program cannot sleep.
- `idempotencyKey` is a new unique string for each change, such as a UUID.
  Repeating a call with the same key makes one change.
- A `PermissionDeniedError` means the operation is forbidden, not mis-called.
  Do not look for another route.

<!-- submilli:if any="shopify.com/orders.search shopify.com/orders.get" -->

## Find orders

`searchOrders({ query, first, after, sortKey, reverse })` returns newest first by
default, for example with `query: "financial_status:paid created_at:>'2026-10-01'"`.
`getOrder(orderId)` returns one order with its first 100 lines, or null. A line's
`id` is what `refundOrder` takes. `total` is the current total, after refunds and
edits.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/orders.update" -->

## Notes and tags

`addOrderNote(orderId, note)` adds a line to the order's note for staff, and
`addOrderTags(orderId, tags)` adds tags. What is already there stays.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/orders.cancel" -->

## Cancel an order

`cancelOrder(orderId, { reason, restock, refund, notifyCustomer, staffNote })`
cancels the order. This cannot be undone. `reason` is `CUSTOMER`, `DECLINED`,
`FRAUD`, `INVENTORY`, `STAFF` or `OTHER`. `restock` and `refund` are required.
Shopify finishes in the background, so the result is a job, with `done` false
until then.

<!-- submilli:endif -->

<!-- submilli:if any="shopify.com/refunds.suggest shopify.com/refunds.create" -->

## Refunds

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/refunds.suggest" -->

`suggestRefund(orderId, { lineItems, shipping })` works out what a refund would
return, without refunding. `lineItems` holds `{ lineItemId, quantity }`.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/refunds.create" -->

`refundOrder(orderId, input)` refunds lines, the remaining shipping, or both.
Shopify works out the amount from what is being refunded; you cannot name an
amount. `restockType` is `NO_RESTOCK` (the default), `RETURN` for fulfilled items
or `CANCEL` for unfulfilled items, and either of the last two needs `locationId`.
`idempotencyKey` is required.

```ts
import { getOrder, refundOrder } from "@submilli/shopify-admin";

function main(): string {
    const order = getOrder("1001");
    if (order === null || order.lineItems.length === 0) return "nothing to refund";
    const line = order.lineItems[0];
    const refund = refundOrder(order.id, {
        lineItems: [{ lineItemId: line.id, quantity: 1 }],
        idempotencyKey: "3f5b6ebf-143c-4da5-8d0f-fb8553bfd85d",
    });
    return refund.id;
}
```

<!-- submilli:endif -->

<!-- submilli:if any="shopify.com/fulfillmentOrders.list shopify.com/fulfillments.create shopify.com/fulfillments.updateTracking shopify.com/fulfillmentOrders.hold shopify.com/fulfillmentOrders.release" -->

## Fulfillment

An order ships through its fulfillment orders, one per location.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/fulfillmentOrders.list" -->

`listFulfillmentOrders(orderId)` returns the first 25 of them with their status,
location, holds and first 20 lines, or null.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/fulfillments.create" -->

`createFulfillment(orderId, { fulfillmentOrderIds, tracking, notifyCustomer })`
records a shipment. Every id named must be open on this order and at one location;
with none named, every open one is fulfilled, and that fails with
`several_locations` if they are at different locations. `tracking` holds `company`,
`number` and an `https://` `url`.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/fulfillments.updateTracking" -->

`updateTracking(fulfillmentId, tracking, notifyCustomer)` changes a shipment's
tracking.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/fulfillmentOrders.hold" -->

`holdFulfillmentOrder(fulfillmentOrderId, reason, notes)` stops one from shipping.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/fulfillmentOrders.release" -->

`releaseFulfillmentOrderHold(fulfillmentOrderId, holdIds)` releases holds, or
every hold when `holdIds` is omitted.

<!-- submilli:endif -->

<!-- submilli:if any="shopify.com/products.search shopify.com/products.get" -->

## Find products

`searchProducts({ query, first, after })` lists products. `getProduct(productId)`
returns one with its first 100 variants, or null.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/products.update" -->

## Change a product

`updateProduct(productId, fields)` changes `title`, `descriptionHtml`, `status`,
`tags`, `vendor` and `productType`. Fields left out stay as they are; `tags`
replaces the list.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/variants.updatePrices" -->

## Change prices

`updateVariantPrices(productId, prices)` takes up to 100 of
`{ variantId, price, compareAtPrice }`.

<!-- submilli:endif -->

<!-- submilli:if any="shopify.com/locations.list shopify.com/inventory.get shopify.com/inventory.set shopify.com/inventory.adjust" -->

## Inventory

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/locations.list" -->

`listLocations()` returns the store's first 50 locations.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/inventory.get" -->

`getInventoryLevels(productId, { first, after })` returns stock by variant and
location, and each variant's `inventoryItemId`. The page size here is 1-15.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/inventory.set" -->

`setInventory(inventoryItemId, locationId, quantity, reason, idempotencyKey)` sets
the quantity available. It fails with `CHANGE_FROM_QUANTITY_STALE` if stock moved
while it ran; read the level again before retrying, with a new key.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/inventory.adjust" -->

`adjustInventory(inventoryItemId, locationId, delta, reason, idempotencyKey)` adds
to or takes from the quantity available.

<!-- submilli:endif -->

<!-- submilli:if any="shopify.com/customers.search shopify.com/customers.get" -->

## Customers

`searchCustomers({ query, first, after })` lists customers. `getCustomer(customerId)`
returns one with their last 10 orders, or null.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/abandonedCheckouts.list" -->

## Abandoned checkouts

`listAbandonedCheckouts({ query, first, after })` lists them, newest first. Each
has `url`, the link the buyer can use to finish it.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/discounts.create" -->

## Create a discount code

`createDiscountCode(input)` takes `title`, `code`, exactly one of `percentage` and
`amount`, and `startsAt`. `percentage: 20` means 20% off. `endsAt`, `usageLimit`,
`oncePerCustomer` and `minimumSubtotal` are optional.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/discounts.deactivate" -->

## Stop a discount code

`deactivateDiscountCode(discountId)` stops it from working.

<!-- submilli:endif -->

<!-- submilli:if any="shopify.com/draftOrders.create shopify.com/draftOrders.sendInvoice" -->

## Draft orders

A draft order's `invoiceUrl` is a link the buyer can pay at.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/draftOrders.create" -->

`createDraftOrder({ lineItems, email, note, discount })` takes up to 100 of
`{ variantId, quantity }`, and an optional `discount` of `{ percentage, title }`.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/draftOrders.sendInvoice" -->

`sendDraftOrderInvoice(draftOrderId, to, subject)` emails the invoice.

<!-- submilli:endif -->

<!-- submilli:if capability="shopify.com/shop.get" -->

## The store

`getShop()` returns its name, email, currency, time zone, domain and plan.

```ts
import { getShop } from "@submilli/shopify-admin";

function main(): string {
    const shop = getShop();
    return shop.name + " " + shop.currency;
}
```

<!-- submilli:endif -->
