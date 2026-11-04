# @submilli/wix-ecommerce

A synchronous, typed client for one Wix site's store, over the Wix REST APIs at
`www.wixapis.com`: eCommerce orders, fulfillments, payments and refunds, Cart V2
checkout links, abandoned checkouts, Stores catalog V1 and V3, inventory, and
contacts. It has no delete operations, and it cannot create orders or products.

## Blueprint setup

Add the package and bind an API key and a site id:

```yaml
packages:
  - "@submilli/wix-ecommerce"

secrets:
  WIX_API_KEY:
    store: WIX_API_KEY
  WIX_SITE_ID:
    store: WIX_SITE_ID
```

Create the key in the site owner's API Keys Manager. API keys are not available
to third-party Wix apps. Give it these scopes:

| Scope | Needed by |
| --- | --- |
| `SCOPE.DC-STORES.READ-ORDERS` | order, fulfillment, payment and abandoned checkout reads |
| `SCOPE.DC-STORES.MANAGE-ORDERS` | notes, cancels, fulfillments, refunds |
| `SCOPE.ECOM.READ-CARTS-V2`, `SCOPE.ECOM.WRITE-CARTS-V2` | `createCart`, `getCheckoutUrl` |
| `SCOPE.DC-STORES.READ-PRODUCTS`, `SCOPE.DC-STORES.MANAGE-PRODUCTS` | catalog V1 reads and writes, V1 inventory |
| `SCOPE.STORES.PRODUCT_READ`, `SCOPE.STORES.PRODUCT_WRITE` | catalog V3 reads and writes |
| `SCOPE.STORES.INVENTORY_ITEM_READ`, `SCOPE.STORES.INVENTORY_ITEM_WRITE` | catalog V3 inventory |
| `SCOPE.DC-CONTACTS.READ-CONTACTS` | `findContactByEmail`, `getContact` |

The package reads both secrets itself and sends them as the `Authorization` and
`wix-site-id` headers. The site id is read before every check, because every
check carries it. `submilli blueprint add-package @submilli/wix-ecommerce` writes
the package's own grants from `capabilities.yaml`:

```yaml
permissions:
  '@submilli/wix-ecommerce':
    - capability: http.get
      filter: host == "www.wixapis.com"
      action: allow
    - capability: http.patch
      filter: host == "www.wixapis.com"
      action: allow
    - capability: http.post
      filter: host == "www.wixapis.com"
      action: allow
    - capability: secrets.get
      filter: name == "WIX_API_KEY"
      action: allow
    - capability: secrets.get
      filter: name == "WIX_SITE_ID"
      action: allow
```

## What a blueprint can constrain

Grant each capability under `main`. Do not grant `main` raw `http.*` to
`www.wixapis.com`: that reopens every argument the package constrains. Every
capability has `siteId`, the bound site, so `siteId == "..."` pins one.

| Capability | Other fields | Notes |
| --- | --- | --- |
| `wix.com/orders.search` | | |
| `wix.com/orders.get`, `orders.note` | `orderId` | |
| `wix.com/orders.cancel` | `orderId`, `totalAmount`, `paymentStatus`, `restock` | The order is fetched first; total and status come from Wix |
| `wix.com/orders.refund` | `orderId`, `amount`, `currency`, `restock`, `externalRefund` | The currency comes from Wix. An amount above what is left on the payment never reaches the check |
| `wix.com/fulfillments.list` | `orderId` | |
| `wix.com/fulfillments.create` | `orderId`, `shippingProvider` | |
| `wix.com/fulfillments.update` | `orderId`, `fulfillmentId` | |
| `wix.com/payments.list` | `orderId` | |
| `wix.com/abandonedCheckouts.list` | | |
| `wix.com/abandonedCheckouts.get` | `abandonedCheckoutId` | The answer holds a checkout link |
| `wix.com/carts.create` | `itemCount` | Total units across lines |
| `wix.com/carts.checkoutUrl` | `cartId` | |
| `wix.com/catalog.version` | | |
| `wix.com/products.search` | `includeHidden` | A cursor carries its first page's filter, so `includeHidden` describes the request, not a cursor from another search |
| `wix.com/products.get` | `productId` | |
| `wix.com/products.update` | `productId`, `fields` | Sorted field names: `not (fields contains "price")` forbids price changes |
| `wix.com/inventory.get` | `productId` | |
| `wix.com/inventory.adjust` | `productId`, `variantId`, `delta` | |
| `wix.com/contacts.find` | `email` | Lowercased |
| `wix.com/contacts.get` | `contactId` | |

For example, a store assistant that can refund small amounts and never edit a
price:

```yaml
default: deny
permissions:
  main:
    - capability: wix.com/orders.search
      action: allow
    - capability: wix.com/orders.refund
      filter: amount <= 200 and externalRefund == false
      action: allow
    - capability: wix.com/products.update
      filter: not (fields contains "price")
      action: allow
```

The limits above are examples. Choose your own.

## Behavior to know

- Wix does not publish rate limits. A 429 throws `WixError` with `retryable`;
  the package does not sleep or retry.
- A site uses one Stores catalog. Each catalog operation reads the version first,
  one extra request per call.
- Only Cart V2 is wrapped. Wix removes Cart V1 and Checkout V1 on 2027-02-01.
- Webhooks are not wrapped. Create Order, Charge Order, product creation and
  deletion are not wrapped either.

## Tests

`build test` needs no key or network and skips the live tests with
`--skip-network`:

```sh
submilli build test --deny-warnings -p @submilli/wix-ecommerce --skip-network
```

Two Node suites cover what that cannot. None needs a key or the network:

```sh
# The requests each operation sends, against a mocked host.
node --test packages/wix-ecommerce/scripts/contract.test.mjs
# The allowed and denied matrix under the real policy engine, with an isolated
# SUBMILLI_HOME and fake secrets. tests/fake-host talks to a mock Wix on 127.0.0.1.
SUBMILLI_BIN=submilli node --test packages/wix-ecommerce/scripts/policy.test.mjs
```

Live reads run with `--env-var WIX_API_KEY --env-var WIX_SITE_ID`. The mutation
test also needs `WIX_DISPOSABLE_ORDER_ID`, and must only point at a disposable
test site.
