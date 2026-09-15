import { label } from "submilli:test";
import { listCharges } from "@acme/billing";

function main(): void {
    label("lists a customer's own charges");
    const charges = listCharges("cus_northwind");
    assert(charges.length === 2, "cus_northwind has two charges in the fixture");
    assert(charges[0].amount === 4900, "the first is the 4900-cent charge");

    label("scopes the lookup to the customer asked for");
    assert(listCharges("cus_initech").length === 1, "cus_initech has one charge");
    assert(listCharges("cus_unknown").length === 0, "an unknown customer has none");
}
