import { label } from "submilli:test";
import {
    getApplicationStatus,
    getCharge,
    getLink,
    listFundings,
    lookupConsumer,
    refundCharge,
    sendApplyLink,
    settleCharge,
    voidCharge,
} from "@submilli/chargeafter-demo";

function throws(action: () => void): boolean {
    try { action(); } catch { return true; }
    return false;
}

function main(): void {
    label("fictional fixture coverage includes every application state and absent records");
    const approved = getApplicationStatus("application-approved");
    assert(approved?.applicationStatus === "Approved", "approved application exists");
    assert(approved?.storeId === "pied_piper", "application includes its fixture store");
    assert(getApplicationStatus("application-disputed")?.applicationStatus === "Disputed", "disputed application exists");
    assert(getApplicationStatus("application-missing") === null, "unknown application is null");

    label("returned values cannot mutate the authoritative fixture");
    const first = getCharge("charge-settled");
    assert(first !== null, "settled charge exists");
    if (first !== null) first.merchantOrderId = "caller-mutated";
    assert(getCharge("charge-settled")?.merchantOrderId === "order-tres-1003", "charge is copied");

    label("sequential settlements accept decimal currency and retain state");
    const partial = settleCharge("charge-authorized", 864.93);
    assert(partial.amount === 864.93 && partial.status === "Settled", "decimal settlement recorded");
    const complete = settleCharge("charge-authorized", 1535.07);
    assert(complete.amount === 1535.07 && complete.status === "Settled", "second settlement completes charge");
    assert(throws(() => { settleCharge("charge-authorized", 1); }), "cannot settle beyond authorized amount");
    settleCharge("charge-decimal", 0.1);
    settleCharge("charge-decimal", 0.1);
    assert(settleCharge("charge-decimal", 0.1).amount === 0.1, "final decimal remainder settles exactly");
    voidCharge("charge-decimal-void", 0.1);
    voidCharge("charge-decimal-void", 0.1);
    voidCharge("charge-decimal-void", 0.1);
    assert(getCharge("charge-decimal-void")?.state === "Voided", "final decimal remainder voids exactly");

    label("sequential refunds retain the remaining authoritative limit");
    const firstRefund = refundCharge("charge-settled", 1000);
    assert(firstRefund.amount === 1000 && firstRefund.status === "Refunded", "first refund recorded");
    const finalRefund = refundCharge("charge-settled", 2200);
    assert(finalRefund.amount === 2200 && finalRefund.status === "Refunded", "second refund completes charge");
    assert(throws(() => { refundCharge("charge-settled", 1); }), "cannot refund beyond settled amount");

    label("links, funding, and consumer fixtures are fictional and usable");
    const link = sendApplyLink({ consumer: { firstName: "Rory", lastName: "Demo", email: "rory@example.test" }, deliveryMethod: "Email", storeId: "pied_piper", channel: "IN_HOME" });
    assert(link.linkId.startsWith("link-demo-") && link.url !== null, "application link generated");
    assert(getLink(link.linkId)?.linkType === "Apply" && getLink(link.linkId)?.storeId === "pied_piper", "created link can be read back with its store");
    assert(getLink("link-apply-ada")?.linkType === "Application", "fixture link exists");
    assert(listFundings({ fromDate: "2026-10-01", toDate: "2026-10-31", showMissing: true }).records.length === 3, "funding report includes missing record when requested");
    const address = { state: "CA", city: "San Jose", line1: "1 Demo Way", zipCode: "95113" };
    assert(lookupConsumer({ firstName: "Ada", lastName: "Morris", email: "ada@example.test", shippingAddress: address, billingAddress: address, netMonthlyIncome: 5000, dateOfBirth: "1990-01-01", phone: "+15550000001" })?.accounts.length === 1, "consumer lookup returns accounts");
    assert(lookupConsumer({ firstName: "Ben", lastName: "Ortiz", email: "ben@example.test", shippingAddress: address, billingAddress: address, netMonthlyIncome: 5000, dateOfBirth: "1990-01-01", phone: "+15550000002" })?.accounts.length === 1, "each fixture identity can be looked up");
    assert(throws(() => { lookupConsumer({ firstName: "", lastName: "Morris", email: "ada@example.test", shippingAddress: address, billingAddress: address, netMonthlyIncome: 5000, dateOfBirth: "1990-01-01" }); }), "consumer lookup requires complete PII");
    const funding = listFundings({ fromDate: "2026-10-01", toDate: "2026-10-31", showMissing: true }).records[0];
    assert(funding.transactionAmount === getCharge(funding.chargeId ?? "")?.settledAmount, "funding amount matches its charge");
    assert(getCharge("charge-authorized")?.storeId === "pied_piper", "charge includes its fixture store");
}
