// `Infinity` / `NaN` are the only identifiers accepted in default position:
// their value is known at signature time, so they fold to the same literal the
// host signatures already carry. That is what makes a lifted
// `Uint8Array.slice(start: number = 0, end: number = Infinity)` pasteable.

function slice(start: number = 0, end: number = Infinity): string {
    return `${start}:${end}`;
}

function precision(digits: number = NaN): boolean { return Number.isNaN(digits); }

function floor(bound: number = -Infinity): string { return `${bound}`; }

// `1e400` overflows to a non-finite, which is the other route to the value that
// must survive a package artifact's JSON round-trip.
function overflow(a: number = 1e400): boolean { return a === Infinity; }

class Range {
    to(end: number = Infinity): string { return `${end}`; }
}

export function main(): string {
    assert(slice() === "0:Infinity", "Infinity default applies");
    assert(slice(1, 2) === "1:2", "an explicit argument still wins");
    assert(precision(), "NaN default applies");
    assert(!precision(2), "an explicit argument still wins over NaN");
    assert(floor() === "-Infinity", "negated Infinity default applies");
    assert(new Range().to() === "Infinity", "a method default takes it too");
    assert(overflow(), "an overflowing literal default folds to Infinity");
    return "ok";
}
