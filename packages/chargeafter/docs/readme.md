# ChargeAfter merchant financing

`@submilli/chargeafter` provides typed operations for a single ChargeAfter
merchant credential. It reads `CHARGEAFTER_PRIVATE_KEY` inside the Package and
uses the configured sandbox or production API host. Programs cannot read the
key or choose the host.

A credential represents one merchant. Some provider responses contain a store
identifier, but the public read capabilities accept only IDs or date ranges;
they do not expose a trusted store field to Blueprint filters. The link-creation
operations carry `storeId`, which a Blueprint can filter.

## Operations and capabilities

| Operation | Capability fields | Result |
| --- | --- | --- |
| `getLink(linkId)` | `linkId` | Link data or `null` when absent |
| `sendApplyLink(input)` | `storeId`, `deliveryMethod` | Generated application link |
| `sendCheckoutLink(input)` | `storeId`, `deliveryMethod`, `totalAmount`, `autoCharge` | Generated checkout link |
| `getApplicationStatus(applicationId)` | `applicationId` | Application status or `null` |
| `listApplicationOffers(applicationId)` | `applicationId` | Flattened lender offers |
| `getCharge(chargeId)` | `chargeId` | Charge or `null` |
| `settleCharge`, `voidCharge` | charge id, amount, authoritative charge fields | Transaction |
| `refundCharge` | charge id, amount, settled amount | Transaction; a supplied reason is rejected because the API has no reason field |
| `setChargeOrderId` | `chargeId` | Updated charge |
| `listFundings(input)` | `fromDate`, `toDate` | Funding records and page values |
| `lookupConsumer(input)` | `email`, `phone` | Full documented consumer details are required; returns application id and accounts, or `null` |
| `getConsumerAccounts(consumerId)` | `consumerId` | Consumer accounts |

Charge amounts returned by the API are authoritative. `Charge.authorizedAmount`
normalizes ChargeAfter's `totalAmount` so the amount policy sees a stable name.
`settleCharge` and `voidCharge` refuse a request above that amount before their
business capability check. `refundCharge` refuses an amount above the settled
amount less prior refunds before that check. The preliminary read is permitted
to occur. A denied business check always prevents the prohibited write.

## Pagination and absence

`listFundings` takes inclusive `fromDate` and `toDate` values in `YYYY-MM-DD`
form. `offset` defaults to `0`, and `limit` defaults to `100` and cannot exceed
`1000`. `getLink`, `getApplicationStatus`, `getCharge`, and `lookupConsumer`
return `null` for ChargeAfter code `2001` or HTTP `404`. Other API failures
throw `ChargeAfterError`.

## Errors

`ChargeAfterError.code` is `unauthorized` for HTTP 401, `forbidden` for HTTP
403, `validation_failed` for HTTP 422, `rate_limited` for HTTP 429, and
`server_error` for 5xx responses. API codes such as `2006` are preserved. A
local validation failure has code `invalid_argument`. The Package does not
retry requests. A policy denial means the operation is forbidden, not that the
remote API refused it.

## Example

```ts
import { getApplicationStatus, getCharge, listFundings } from "@submilli/chargeafter";

function main(): string {
    const application = getApplicationStatus("application-123");
    const charge = getCharge("charge-123");
    const fundings = listFundings({ fromDate: "2026-10-01", toDate: "2026-10-31", limit: 25 });
    return (application?.applicationStatus ?? "missing") + " "
        + (charge?.state ?? "missing") + " " + fundings.records.length.toString();
}
```
