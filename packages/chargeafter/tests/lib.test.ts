import {
    ChargeAfterError,
    getLink,
    listFundings,
    lookupConsumer,
    refundCharge,
    sendApplyLink,
    sendCheckoutLink,
    setChargeOrderId,
} from "@submilli/chargeafter";
import { consumerAccountsResponse } from "../src/response";

function expectInvalid(action: () => void): void {
    try {
        action();
        assert(false, "expected ChargeAfterError");
    } catch (error: ChargeAfterError) {
        assert(error.code === "invalid_argument", "expected invalid_argument, got " + error.code);
        assert(error.status === 0, "local validation has status 0");
        assert(!error.retryable, "local validation is not retryable");
    }
}

function main(): void {
    const nullableAccounts = consumerAccountsResponse({ accountsInfo: null });
    assert(nullableAccounts.accountsInfo === null, "nullable accountsInfo is accepted");
    const nestedAccount = consumerAccountsResponse({ accountsInfo: [{ account: { id: "account", creditInfo: null } }] });
    if (nestedAccount.accountsInfo === null || nestedAccount.accountsInfo === undefined) throw new Error("nested account response is absent");
    assert(nestedAccount.accountsInfo[0].account?.creditInfo === null, "nullable creditInfo is accepted");
    // These failures happen before credentials or HTTP: package tests never call ChargeAfter.
    expectInvalid(() => { getLink(" "); });
    expectInvalid(() => { sendApplyLink({
        consumer: { firstName: "A", lastName: "B" },
        deliveryMethod: "Email",
        storeId: "store",
        channel: "E_COMMERCE",
    }); });
    expectInvalid(() => { sendCheckoutLink({
        consumer: { firstName: "A", lastName: "B", email: "a@example.test" },
        deliveryMethod: "Email",
        storeId: "store",
        channel: "E_COMMERCE",
        cart: { items: [], totalAmount: 0 },
    }); });
    expectInvalid(() => { refundCharge("charge", 0); });
    expectInvalid(() => { setChargeOrderId("charge", ""); });
    expectInvalid(() => { listFundings({ fromDate: "2026/01/01", toDate: "2026-01-02" }); });
    expectInvalid(() => { listFundings({ fromDate: "2026-01-01", toDate: "2026-01-02", limit: 1001 }); });
    expectInvalid(() => { lookupConsumer({ firstName: "", lastName: "", email: "", shippingAddress: { state: "", city: "", line1: "", zipCode: "" }, billingAddress: { state: "", city: "", line1: "", zipCode: "" }, netMonthlyIncome: 0, dateOfBirth: "" }); });
}
