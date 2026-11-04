// Opt-in live read. It is skipped by --skip-network and unless both secrets are bound.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getCharge } from "@submilli/chargeafter";

function main(): void {
    if (secrets.get("CHARGEAFTER_PRIVATE_KEY") === undefined) {
        label("skip: CHARGEAFTER_PRIVATE_KEY is not bound");
        return;
    }
    const chargeId = secrets.get("CHARGEAFTER_TEST_CHARGE_ID");
    if (chargeId === undefined) {
        label("skip: CHARGEAFTER_TEST_CHARGE_ID is not bound");
        return;
    }
    const charge = getCharge(chargeId);
    assert(charge !== null, "configured ChargeAfter test charge exists");
    if (charge !== null) {
        assert(charge.id === chargeId, "returned charge ID matches");
        assert(charge.authorizedAmount >= 0, "charge total is non-negative");
    }
}
