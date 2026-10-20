import { listCharges } from "@acme/billing";

function main(): string {
    const charges = listCharges("pied_piper");
    let total = 0;
    for (const charge of charges) {
        total += charge.amount;
    }
    return `${charges.length} charges, ${total} cents`;
}
