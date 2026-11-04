// MOCK: see money.yaml. The fake answers with one small order and one large order. The script
// that runs this also asserts which writes reached the fake.
import { WixError, cancelOrder, refundOrder } from "@submilli/wix-ecommerce";

const SMALL = "aaaaaaaa-0000-4000-8000-000000000001";
const LARGE = "aaaaaaaa-0000-4000-8000-000000000002";
const PAYMENT = "bbbbbbbb-0000-4000-8000-000000000001";

function deniedAt(capability: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === "main denied at " + capability, "expected a denial at " + capability + ", got " + outcome);
}

function rejectedAs(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: WixError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function main(): string {
    // Under the cap: reaches the fake.
    const refund = refundOrder(SMALL, { paymentId: PAYMENT, amount: 200 });
    assert(refund.status === "SUCCEEDED", "the refund went through");

    // Above the cap, though the payment could cover it.
    deniedAt("wix.com/orders.refund", () => { refundOrder(LARGE, { paymentId: PAYMENT, amount: 200.01 }); });
    // Records a refund made elsewhere: a separate decision.
    deniedAt("wix.com/orders.refund", () => { refundOrder(SMALL, { paymentId: PAYMENT, amount: 1, externalRefund: true }); });
    // Above what is left on the payment: refused by the package, whatever the policy.
    rejectedAs("refund_too_large", () => { refundOrder(SMALL, { paymentId: PAYMENT, amount: 300.01 }); });

    // The total comes from Wix, not the caller.
    cancelOrder(SMALL);
    deniedAt("wix.com/orders.cancel", () => { cancelOrder(LARGE); });
    return "ok";
}
