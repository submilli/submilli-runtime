import { label } from "submilli:test";
import { listCharges } from "@acme/billing";

function main(): void {
    label("lists a customer's own charges");
    const charges = listCharges("pied_piper");
    assert(charges.length === 2, "pied_piper has two charges in the fixture");
    assert(charges[0].amount === 4900, "the first is the 4900-cent charge");

    label("scopes the lookup to the customer asked for");
    assert(listCharges("hooli").length === 1, "hooli has one charge");
    assert(listCharges("cus_unknown").length === 0, "an unknown customer has none");
}
