# @submilli/shopify-admin

A synchronous, typed client for one Shopify store, over the GraphQL Admin API,
version `2026-10`: orders, refunds, fulfillment, products, inventory, customers,
discount codes, draft orders and abandoned checkouts. It has no delete operations,
and it cannot create products or customers. It uses GraphQL only.

## Blueprint setup

Add the package and bind an access token and the store:

```yaml
packages:
  - "@submilli/shopify-admin"

secrets:
  SHOPIFY_ACCESS_TOKEN:
    store: SHOPIFY_ACCESS_TOKEN
  SHOPIFY_SHOP:
    store: SHOPIFY_SHOP
```

`SHOPIFY_SHOP` is the store's subdomain, `acme-store` for
`acme-store.myshopify.com`. The package refuses anything that is not a single
label, so its host is always `<label>.myshopify.com`.

Use an offline access token from an app installed on the store. Give the app the
scopes the operations need:

| Scope | Needed by |
| --- | --- |
| `read_orders`, `write_orders` | orders, notes, tags, cancels, refunds, abandoned checkouts |
| `read_all_orders` | orders older than 60 days |
| `read_products`, `write_products` | catalog reads and writes, prices |
| `read_inventory`, `write_inventory`, `read_locations` | inventory and locations |
| `read_customers` | customers |
| `read_merchant_managed_fulfillment_orders`, `write_merchant_managed_fulfillment_orders`, `read_fulfillments` | fulfillment orders, fulfillments, tracking, holds |
| `read_discounts`, `write_discounts` | discount codes |
| `read_draft_orders`, `write_draft_orders` | draft orders |

Reading names, emails, phones and addresses is protected customer data. A public
app needs Shopify's approval for it.

### The host rule

The host holds the store's name, so `build check` cannot derive a host filter. It
warns once, at the one `http.post` in `src/lib.ts`, and `--deny-warnings` fails on
that warning alone. `capabilities.yaml` lists `http.post` with no filter, and
`submilli blueprint add-package` copies that.

Always replace it with a rule for your store. Without one the package may post to
any host, though it only ever builds this one:

```yaml
permissions:
  '@submilli/shopify-admin':
    - capability: http.post
      filter: host == "acme-store.myshopify.com"
      action: allow
    - capability: secrets.get
      filter: name == "SHOPIFY_ACCESS_TOKEN"
      action: allow
    - capability: secrets.get
      filter: name == "SHOPIFY_SHOP"
      action: allow
```

## What a blueprint can constrain

Grant each capability under `main`. Do not grant `main` raw `http.*` to the store:
that reopens every argument the package constrains. Every capability has `shop`,
the bound store, so `shop == "..."` pins one.

| Capability | Other fields | Notes |
| --- | --- | --- |
| `shopify.com/orders.search` | | |
| `shopify.com/orders.get` | `orderId` | |
| `shopify.com/orders.update` | `orderId`, `fields` | `addOrderNote` sets `["note"]`, `addOrderTags` sets `["tags"]` |
| `shopify.com/orders.cancel` | `orderId`, `totalAmount`, `currency`, `restock`, `refund` | The total comes from Shopify. Cancelling cannot be undone. With `refund`, this returns money; with `restock`, stock |
| `shopify.com/refunds.suggest` | `orderId` | Writes nothing |
| `shopify.com/refunds.create` | `orderId`, `amount`, `currency`, `restock` | Shopify works out the amount; the caller cannot name one. Money is the buyer's currency, so cap `amount` together with `currency` |
| `shopify.com/fulfillmentOrders.list` | `orderId` | |
| `shopify.com/fulfillments.create` | `orderId`, `notifyCustomer` | Each fulfillment order named must be open on this order |
| `shopify.com/fulfillments.updateTracking` | `orderId`, `fulfillmentId` | The order comes from Shopify |
| `shopify.com/fulfillmentOrders.hold` | `fulfillmentOrderId`, `reason` | |
| `shopify.com/fulfillmentOrders.release` | `fulfillmentOrderId` | |
| `shopify.com/products.search` | | |
| `shopify.com/products.get` | `productId` | |
| `shopify.com/products.update` | `productId`, `fields` | Sorted field names. Prices are a separate capability |
| `shopify.com/variants.updatePrices` | `productId`, `variantIds`, `maxPrice` | `maxPrice` is the highest price in the call |
| `shopify.com/locations.list` | | |
| `shopify.com/inventory.get` | `productId` | |
| `shopify.com/inventory.set` | `inventoryItemId`, `locationId`, `quantity`, `delta` | `delta` is worked out from the level Shopify reports |
| `shopify.com/inventory.adjust` | `inventoryItemId`, `locationId`, `delta` | |
| `shopify.com/customers.search` | | |
| `shopify.com/customers.get` | `customerId` | |
| `shopify.com/abandonedCheckouts.list` | | The answer holds checkout links |
| `shopify.com/discounts.create` | `code`, `percentage`, `amount`, `usageLimit` | Uppercased code. The kind not used is 0 and no limit is 0, so see the example below |
| `shopify.com/discounts.deactivate` | `discountId` | |
| `shopify.com/draftOrders.create` | `itemCount`, `email`, `discountPercentage` | `itemCount` is total units. Its discount is capped here, not by `discounts.create` |
| `shopify.com/draftOrders.sendInvoice` | `draftOrderId`, `to` | |
| `shopify.com/shop.get` | | |

For example, a store assistant that can refund small amounts and hand out a
limited discount:

```yaml
default: deny
permissions:
  main:
    - capability: shopify.com/orders.search
      action: allow
    - capability: shopify.com/refunds.create
      filter: amount <= 200 and currency == "USD" and restock == false
      action: allow
    - capability: shopify.com/discounts.create
      filter: percentage <= 20 and amount == 0 and usageLimit >= 1 and usageLimit <= 100
      action: allow
```

The limits above are examples. Choose your own. Keep in mind:

- `percentage <= 20` alone would allow any fixed `amount`, and a code with no usage
  limit.
- `orders.cancel` can refund, so a cap on `refunds.create` does not cover it. Filter
  `refund == false` to keep refunds to one capability.
- `restock` moves stock without `shopify.com/inventory.adjust`, so limits on `delta`
  do not cover it. Filter `restock == false` on refunds and cancels if they should.
- A cap is per call. Three refunds of 100 pass a cap of 200.

## Behavior to know

- Shopify limits by query cost. `first` is at most 100, not Shopify's 250, because
  a single query may cost at most 1000 points; `getInventoryLevels` takes at most
  15 for the same reason. A throttled call throws `ShopifyError` with `retryable`
  and `retryAfterSeconds`. The package does not sleep or retry.
- `refundOrder`, `setInventory` and `adjustInventory` take a required
  `idempotencyKey`, which this API version requires.
- `setInventory` sends the level it read as `changeFromQuantity`, so a write that
  races another fails with `CHANGE_FROM_QUANTITY_STALE`. `adjustInventory` sends
  null, because a change by a delta cannot overwrite a newer count.
- `addOrderNote` and `addOrderTags` read, merge and write, because `orderUpdate`
  replaces both fields. Two writers at once can lose one write.
- Not wrapped: product creation, order edits, returns, bulk operations, webhooks.

## Tests

`build test` needs no token or network and skips the live tests with
`--skip-network`. It reports the host warning above:

```sh
submilli build test -p @submilli/shopify-admin --skip-network
```

Two Node suites cover what that cannot. None needs a token or the network:

```sh
# The requests each operation sends, against a mocked host.
node --test packages/shopify-admin/scripts/contract.test.mjs
# The allowed and denied matrix under the real policy engine, with an isolated
# SUBMILLI_HOME and fake secrets. tests/fake-host talks to a mock Shopify on 127.0.0.1.
SUBMILLI_BIN=submilli node --test packages/shopify-admin/scripts/policy.test.mjs
```

Live reads run with `--env-var SHOPIFY_ACCESS_TOKEN --env-var SHOPIFY_SHOP`. The
mutation test also needs `SHOPIFY_DISPOSABLE_ORDER_ID`, and must only point at a
disposable development store.
