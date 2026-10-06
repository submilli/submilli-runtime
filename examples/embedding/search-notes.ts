import * as embedding from "submilli:embedding";

const notes = [
    "Refunds are issued to the original payment method within five business days.",
    "The dashboard supports a dark theme under Settings, then Appearance.",
    "Invoices are generated on the first day of each month and emailed to the billing contact.",
];
const query = "How do I get my money back?";

function dot(a: number[], b: number[]): number {
    let sum = 0;
    for (let i = 0; i < a.length; i++) {
        sum += a[i] * b[i];
    }
    return sum;
}

function main(): string[] {
    const aliases = embedding.models();
    if (aliases.length === 0) {
        throw new Error("no embedding alias is available to this caller");
    }
    const alias = aliases[0];
    // Notes are text you search over, the query is text you search with.
    const documents = embedding.embed(alias.name, notes, "document");
    const asked = embedding.embed(alias.name, [query], "query");
    if (documents.identity !== asked.identity) {
        throw new Error("the vectors come from different embedding spaces");
    }

    // Every vector is unit length, so the dot product is the cosine similarity.
    const queryVector = asked.vector(0);
    const scored: { score: number; note: string }[] = [];
    for (let i = 0; i < documents.count; i++) {
        scored.push({ score: dot(queryVector, documents.vector(i)), note: notes[i] });
    }
    scored.sort((a, b) => b.score - a.score);

    const lines = [`identity: ${documents.identity}`];
    for (const item of scored) {
        lines.push(`${item.score.toFixed(3)}  ${item.note}`);
    }
    return lines;
}
