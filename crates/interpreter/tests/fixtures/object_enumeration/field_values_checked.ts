// Values read from fields are checked as `as T[]` checks them. A record is
// covariant in its value type and an object keeps fields its type leaves out,
// so a field can hold a value of another type at run time: that throws a
// TypeError at the call, as a record read does, instead of reaching code typed
// for `T`. Array elements aren't checked.

type Prices = { apple: number; pear: number };

function main(): void {
    const parsed = { apple: 1, pear: 2, currency: "USD" };
    const prices: Prices = parsed;
    let threw = false;
    try {
        Object.values(prices);
    } catch (e: TypeError) {
        threw = e.message.includes("expected number, got string");
    }
    assert(threw);

    const byName: Record<string, number> = prices;
    threw = false;
    try {
        Object.entries(byName);
    } catch (e: TypeError) {
        threw = e.message.includes("expected number, got string");
    }
    assert(threw);

    // A present optional field holding `undefined` is listed, and passes.
    const counts: { a?: number } = {};
    counts.a = undefined;
    assert(Object.values(counts)[0] === undefined);

    // Seen through a record, a present optional field holding `undefined` is
    // outside the record's value type, as tsc's `number[]` says it isn't.
    const optional: { a?: number; b: number } = { a: undefined, b: 1 };
    const viaRecord: Record<string, number> = optional;
    threw = false;
    try {
        Object.values(viaRecord);
    } catch (e: TypeError) {
        threw = e.message.includes("expected number, got undefined");
    }
    assert(threw);

    const exact: Prices = { apple: 1, pear: 2 };
    assert(Object.values(exact).reduce((total, n) => total + n, 0) === 3);
    const xs: number[] = [1, 2];
    assert(Object.values(xs).length === 2);
}
