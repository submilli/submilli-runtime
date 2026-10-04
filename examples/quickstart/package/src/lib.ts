// A charge lookup for a customer, standing in for a real billing API.

import { check } from "submilli:security";

/** One charge on a customer's account. */
export interface Charge {
    /** The customer the charge belongs to. */
    customerId: string;
    /** Charge identifier, as the billing system issued it. */
    id: string;
    /** Amount in cents. */
    amount: number;
}

const LEDGER: Charge[] = [
    { customerId: "cus_northwind", id: "ch_a1", amount: 4900 },
    { customerId: "cus_northwind", id: "ch_a2", amount: 1250 },
    { customerId: "cus_initech", id: "ch_a3", amount: 39900 },
];

/**
 * List the charges on one customer's account.
 * @param customerId Billing customer ID, such as `cus_northwind`.
 * @returns The customer's charges; empty when they have none.
 * @capability acme.com/charges.list { customerId: string }
 */
export function listCharges(customerId: string): Charge[] {
    check("acme.com/charges.list", { customerId });

    // In Production, this would call an API endpoint.
    const found: Charge[] = [];
    for (const charge of LEDGER) {
        if (charge.customerId === customerId) {
            found.push(charge);
        }
    }
    return found;
}
