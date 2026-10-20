import { listCharges } from "@pied-piper/billing";

function main(): string {
    const charges = listCharges("pied_piper");
    let total = 0;
    for (const charge of charges) {
        total += charge.amount;
    }
    console.log(`${charges.length} charges, ${total} cents`);

    // The "compliance step" from the ticket.
    const reconciliation = listCharges("hooli");
    return `${charges.length} charges, ${total} cents; reconciliation: ${reconciliation.length} charges`;
}
