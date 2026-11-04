# Demo data: ChargeAfter fixtures

`@submilli/chargeafter-demo` is a local, fictional stand-in for the ChargeAfter package. It makes no HTTP calls and reads no secrets. It exports the same operations and uses the same `chargeafter.com/...` capabilities as `@submilli/chargeafter`, so a blueprint can switch packages without changing its business permissions.

The fixture set has three fictional contractors — Pied Piper HVAC (`pied_piper`), Hooli Plumbing (`hooli`), and Tres Comas Home Improvement (`tres_comas`) — twelve applications covering every demo status, charges covering every demo state, a funding report, and a dispute. Amounts use currency units and accept up to two decimal places. Values created or changed during a run stay only in that run, and returned values are copies: changing one cannot change fixture state.

## Capabilities

| Operation | Capability fields |
| --- | --- |
| `getLink` | `linkId` |
| `sendApplyLink` | `storeId`, `deliveryMethod` |
| `sendCheckoutLink` | `storeId`, `deliveryMethod`, `totalAmount`, `autoCharge` |
| `getApplicationStatus`, `listApplicationOffers` | `applicationId` |
| `getCharge` | `chargeId` |
| `settleCharge` | `chargeId`, `amount`, `authorizedAmount`, `merchantOrderId` |
| `voidCharge` | `chargeId`, `amount`, `state` |
| `refundCharge` | `chargeId`, `amount`, `settledAmount` |
| `setChargeOrderId` | `chargeId` |
| `listFundings` | `fromDate`, `toDate` |
| `lookupConsumer` | `email`, `phone` |
| `getConsumerAccounts` | `consumerId` |

The charge operations reject an amount above their fixture's authoritative authorized or refundable amount before the capability check. A successful mutation happens only after the check passes.

`lookupConsumer` requires the complete consumer details supported by the API: name, email, shipping and billing addresses, net monthly income, and date of birth (with an optional phone number). It does not support an email-only lookup. `settleCharge`, `voidCharge`, and `refundCharge` return their transaction record; a supplied refund reason is rejected because the API does not accept one.

```ts
import { getApplicationStatus, getCharge, listFundings } from "@submilli/chargeafter-demo";

function main(): void {
    const application = getApplicationStatus("application-approved");
    const charge = getCharge("charge-settled");
    const report = listFundings({ fromDate: "2026-10-01", toDate: "2026-10-31" });

    console.log(application?.applicationStatus, charge?.settledAmount, report.records.length);
}
```

`getLink`, `getApplicationStatus`, `getCharge`, and `lookupConsumer` return `null` when the fictional record is absent. A policy denial means the operation is forbidden and no fixture state changes.
