# Wix eCommerce package guide

Use `@submilli/wix-ecommerce` to work with one Wix site's store: orders,
fulfillments, payments and refunds, abandoned checkouts, checkout links, the
Stores catalog, inventory and contacts. Credentials and the site are supplied
internally; never pass a key or a site id. Every call is synchronous.

- Ids are GUIDs. They are trimmed and lowercased.
- Money is a number in the order's currency, with at most three decimal places.
- Lists return `Page<T>`: `items` and `nextCursor`. Pass `nextCursor` as `cursor`
  for the next page, while it is not null. Page size is 1-100, default 50.
- A single missing resource returns `null`.
- Errors are `WixError`, with `code`, `status`, `retryable` and
  `retryAfterSeconds`. `status` 0 means the package refused the call itself
  before Wix applied it. After a 429, stop and report; Wix asks for about a
  minute's wait, and a program cannot sleep.
- A `PermissionDeniedError` means the operation is forbidden, not mis-called.
  Do not look for another route.

<!-- submilli:if any="wix.com/orders.search wix.com/orders.get" -->

## Find orders

`searchOrders(query, page)` filters by `status`, `paymentStatus`,
`fulfillmentStatus`, `buyerEmail`, `createdAfter` and `number`, newest first
unless `sort` says otherwise. `PENDING` and `REJECTED` orders appear only when
`status` asks for them. `getOrder(orderId)` returns one order or null.

An `Order` has `id`, `number`, `status`, `paymentStatus`, `fulfillmentStatus`,
`currency`, `total`, `buyerEmail`, `contactId`, `createdDate` and `lineItems`.

<!-- submilli:endif -->

<!-- submilli:if capability="wix.com/orders.note" -->

## Add a note

`addOrderNote(orderId, message)` records a note for the merchant, up to 5000
characters. Buyers do not see it.

<!-- submilli:endif -->

<!-- submilli:if capability="wix.com/orders.cancel" -->

## Cancel an order

`cancelOrder(orderId, { restock, notifyCustomer, message })` cancels the order.
`restock` and `notifyCustomer` default to false, and `message` needs
`notifyCustomer`. Cancelling does not refund: use `refundOrder` for that.

<!-- submilli:endif -->

<!-- submilli:if any="wix.com/fulfillments.list wix.com/fulfillments.create wix.com/fulfillments.update" -->

## Fulfillments

`listFulfillments(orderId)` lists them. `createFulfillment(orderId, { tracking,
lineItems })` records a shipment of an `APPROVED` order; `tracking` is required
and omitting `lineItems` fulfills every line. Wix may email the buyer, depending
on the site's settings. `updateTracking(orderId, fulfillmentId, tracking)`
changes the fields you set. A tracking `link` must be `https://`.

<!-- submilli:endif -->

<!-- submilli:if any="wix.com/payments.list wix.com/orders.refund" -->

## Payments and refunds

`listPayments(orderId)` gives each payment's `id`, `amount`, `refunded` and
`refundable`.

`refundOrder(orderId, input)` refunds one payment. Give `paymentId` and `amount`;
an amount above `refundable` fails with `refund_too_large` and refunds nothing.
`lineItems` says what is being refunded and is required for `restock`.
`notifyCustomer` sends the buyer an email, with an optional `message`.
`externalRefund` records a refund already made at the payment provider, without
moving money.

```ts
import { listPayments, refundOrder } from "@submilli/wix-ecommerce";

function main(): string {
    const orderId = "aaaaaaaa-0000-4000-8000-000000000001";
    const payments = listPayments(orderId);
    if (payments.length === 0) return "no payments";
    const payment = payments[0];
    const refund = refundOrder(orderId, { paymentId: payment.id, amount: 10, reason: "Damaged in transit" });
    return refund.id + " " + refund.status;
}
```

<!-- submilli:endif -->

<!-- submilli:if any="wix.com/abandonedCheckouts.list wix.com/abandonedCheckouts.get" -->

## Abandoned checkouts

`listAbandonedCheckouts({ status, buyerEmail, createdAfter }, page)` lists them,
newest first. `getAbandonedCheckout(id)` returns one, with `checkoutUrl`, the
link the buyer can use to finish it.

<!-- submilli:endif -->

<!-- submilli:if any="wix.com/carts.create wix.com/carts.checkoutUrl" -->

## Checkout links

`createCart(items, customer)` creates a cart of Stores products, and
`getCheckoutUrl(cartId)` returns a link the buyer opens to pay. A product with
variants needs a `variantId`, from `getInventory`.

<!-- submilli:endif -->

<!-- submilli:if any="wix.com/catalog.version wix.com/products.search wix.com/products.get wix.com/products.update" -->

## Catalog

A site uses Stores catalog V1 or V3; the operations follow it.
`getCatalogVersion()` says which, and `STORES_NOT_INSTALLED` means there is no
store. `searchProducts({ text, includeHidden }, page)` and `getProduct(productId)`
return a `Product` with `id`, `name`, `slug`, `visible`, `description`, `price`
(the lowest variant price on V3), `currency` and `catalog`.

`updateProduct(productId, fields)` changes `name`, `description` (HTML) and
`visible`. `ribbon` and `price` work on V1 only; on V3 they fail with
`unsupported_on_catalog`.

<!-- submilli:endif -->

<!-- submilli:if any="wix.com/inventory.get wix.com/inventory.adjust" -->

## Inventory

`getInventory(productId)` returns one `InventoryLevel` per variant, and per
location on V3, with `variantId`, `quantity` (null when only in-stock status is
tracked) and `inStock`. `adjustInventory(productId, variantId, delta)` adds a
positive delta or removes a negative one. On V3, a variant stocked in several
locations fails with `ambiguous_inventory_item`.

<!-- submilli:endif -->

<!-- submilli:if any="wix.com/contacts.find wix.com/contacts.get" -->

## Contacts

`findContactByEmail(email)` and `getContact(contactId)` return a `Contact` with
`id`, `email`, `phone`, `firstName`, `lastName`, `company` and `createdDate`, or
null.

<!-- submilli:endif -->

<!-- submilli:if capability="wix.com/orders.search" -->

## Example

```ts
import { searchOrders } from "@submilli/wix-ecommerce";

function main(): string[] {
    const page = searchOrders({ paymentStatus: "PAID", fulfillmentStatus: "NOT_FULFILLED" }, { limit: 20 });
    const lines: string[] = [];
    for (const order of page.items) {
        lines.push("#" + order.number + " " + (order.buyerEmail ?? "no email"));
    }
    return lines;
}
```

<!-- submilli:endif -->
